<script lang="ts">
import { defineComponent } from "vue";

import Sidebar from "@/components/AppSideBar.vue";
import EngineStats from "@/components/Analysis/EngineStats.vue";
import EngineButtons from "@/components/Analysis/EngineButtons.vue";
import Fen from "@/components/Analysis/Fen.vue";
import EngineLines from "@/components/Analysis/EngineLines.vue";
import Pgn from "@/components/Analysis/Pgn.vue";
import ChessGroundBoard from "@/components/Analysis/ChessGroundBoard.vue";

import type { EngineInfo } from "@/ts/UciFilter";
import type { PV } from "@/ts/PrincipalVariation";

import { filterUCIInfo, extractMove } from "@/ts/UciFilter";
import { extractPV } from "@/ts/PrincipalVariation";

import ChessProcess from "@/ts/ChessProcess";

const startpos = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

export default defineComponent({
  name: "app",
  components: {
    Sidebar: Sidebar,
    EngineStats: EngineStats,
    Fen: Fen,
    EngineButtons: EngineButtons,
    EngineLines: EngineLines,
    Pgn: Pgn,
    ChessGroundBoard: ChessGroundBoard,
  },
  data() {
    return {
      chessProcess: null as ChessProcess | null,

      activeTabIndex: 0,
      smallNavbar: [
        {
          id: "engine-lines",
          name: "Engine Lines",
        },
        {
          id: "prompt",
          name: "Prompt",
        },
        {
          id: "settings",
          name: "Settings",
        },
      ],

      engine_info: {
        score: "0",
        nodes: "0",
        nps: "0",
        depth: "0",
        time: "0",
        tbhits: "0",
        hashfull: "0",
      } as EngineInfo,

      isEngineAlive: false,
      isRunning: false,

      moveHistoryLan: [] as string[],
      moveHistorySan: [] as string[],

      engineLines: new Map<string, PV>(),

      startFen: startpos,
      currentFen: startpos,

      status: "IDLE",
      sideToMove: "white",
    };
  },
  computed: {
    activeTab(): string {
      return this.smallNavbar[this.activeTabIndex].id;
    },
    updateAnalysisStatus(): string {
      let status = this.status;

      if (status === "" || status === "IDLE") {
        if (this.isRunning) {
          status = "ANALYSIS";
        } else if (this.isEngineAlive) {
          status = "READY";
        }
      } else {
        status = this.status;

        this.sendEngineCommand("stop");
      }

      return status;
    },
  },
  mounted() {
    this.initEngine();

    window.addEventListener("keydown", this.handleKeydown);
  },
  beforeUnmount() {
    window.removeEventListener("keydown", this.handleKeydown);

    this.sendEngineCommand("quit");
  },
  methods: {
    handleKeydown(event: KeyboardEvent) {
      if (event.key === "g" && event.ctrlKey && !this.isRunning) {
        event.preventDefault();
        this.sendEngineCommand("go");
      } else if (event.key === "h" && event.ctrlKey) {
        event.preventDefault();
        this.sendEngineCommand("stop");
      } else if (event.key === "r" && event.ctrlKey && !this.isRunning) {
        event.preventDefault();
        this.sendEngineCommand("restart");
      } else if (event.key === "n" && event.ctrlKey) {
        event.preventDefault();
        this.sendEngineCommand("stop");
        this.newPosition(startpos);
      }
    },
    updateInfoStats(line: string) {
      if (!this.isEngineAlive || !this.isRunning || !line.startsWith("info")) {
        return;
      }

      const filtered = filterUCIInfo(line);
      if (Object.keys(filtered).length === 0) {
        return;
      }

      // only update changed values
      this.engine_info = { ...this.engine_info, ...filtered };

      if (
        this.engine_info.pv &&
        this.engine_info.pv.length > 0 &&
        this.engine_info.pv[0].orig !== "" &&
        this.engine_info.pv[0].dest !== "" &&
        this.isEngineAlive
      ) {
        (this.$refs.chessGroundBoardRef as any).drawMove(
          this.engine_info.pv[0]
        );
      }

      let lines = extractPV(line);

      const evaluation = lines.score;

      if (evaluation.startsWith("cp")) {
        let cp = Number(evaluation.slice(2));
        if (this.sideToMove === "black") {
          cp = -cp;
        }
        lines.score = "cp " + cp;
      } else if (evaluation.startsWith("mate")) {
        let mateIn = Number(evaluation.slice(4));
        if (this.sideToMove === "black") {
          mateIn = -mateIn;
        }
        lines.score = "mate " + mateIn;
      }

      if (lines.pv[0]) {
        // reset active pvs
        this.engineLines.forEach((pv) => {
          pv.active = false;
        });

        lines.active = true;
        this.engineLines.set(lines.pv[0], lines);
      } else {
        this.engineLines.forEach((pv) => {
          if (pv.active) {
            lines.active = true;
            // keep other displayed values
            lines.pv = pv.pv;
            lines.score = pv.score;

            this.engineLines.set(lines.pv[0], lines);
          }
        });
      }
      console.log(this.engineLines);
    },
    clearAnalysisInfo() {
      this.engineLines.clear();

      this.engine_info = {
        nodes: "0",
        nps: "0",
        depth: "0",
        time: "0",
        tbhits: "0",
        hashfull: "0",
      };
    },
    shiftAnalysisInfo() {
      // remove all other lines and shift the line with the played move to the right
      let correctLine: PV = null as any;
      const moves = this.moveHistoryLan;

      const playedMove = moves[moves.length - 1];

      this.engineLines.forEach((pv, key) => {
        if (key === playedMove) {
          correctLine = pv;
          correctLine.pv.shift();
        }
      });

      this.engineLines.clear();

      if (correctLine && correctLine.pv.length > 0) {
        this.engineLines.set(correctLine.pv[0], correctLine);
      }
    },
    getUciMoves() {
      return this.moveHistoryLan.join(" ");
    },
    async initEngine() {
      const enginesData = localStorage.getItem("engines");
      const engines = enginesData ? JSON.parse(enginesData) : [];

      if (engines.length === 0) {
        return;
      }

      if (this.chessProcess) {
        this.sendEngineCommand("quit");
      }

      this.isEngineAlive = true;

      this.chessProcess = new ChessProcess(engines[0].path, (line) => {
        this.updateInfoStats(line);
      });

      await this.chessProcess.start();
      this.chessProcess.sendOptions(engines[0].settings);
    },
    async sendEngineCommand(command: string) {
      if (command === "go" && (this.status === "" || this.status === "IDLE")) {
        this.clearAnalysisInfo();

        if (!this.isEngineAlive) {
          await this.initEngine();
        }

        if (this.startFen === startpos && this.getUciMoves() === "") {
          this.chessProcess?.sendStartpos();
        } else if (this.startFen === startpos) {
          this.chessProcess?.sendStartposMoves(this.getUciMoves());
        } else {
          this.chessProcess?.sendPositionMoves(
            this.startFen,
            this.getUciMoves()
          );
        }
        this.isRunning = true;
        this.chessProcess?.sendGo();
      } else if (command === "stop") {
        this.isRunning = false;
        await this.chessProcess?.sendStop();
      } else if (command === "quit") {
        this.isEngineAlive = false;
        this.isRunning = false;
        await this.chessProcess?.sendStop();
        await this.chessProcess?.sendQuit();
      } else if (command === "restart") {
        this.isRunning = false;
        this.isEngineAlive = false;

        this.clearAnalysisInfo();

        await this.chessProcess?.sendStop();
        await this.chessProcess?.sendQuit();
        await this.initEngine();
      }

      localStorage.setItem("status", this.isRunning.toString());
    },
    updatedSideToMove(side: string) {
      this.sideToMove = side;
    },
    updatedStatus(status: string) {
      this.status = status;
    },
    updatedMove(moves: any) {
      this.moveHistoryLan = moves["moveHistoryLan"];
      this.moveHistorySan = moves["moveHistorySan"];

      this.shiftAnalysisInfo();

      if (this.isRunning) {
        this.sendEngineCommand("stop");
        this.sendEngineCommand("go");
      }
    },
    updatedCg(fen: string) {
      this.currentFen = fen;

      let activeLine: PV = null as any;

      this.engineLines.forEach((pv) => {
        if (pv.active) {
          activeLine = pv;
        }
      });

      if (activeLine && activeLine.pv.length > 0) {
        (this.$refs.chessGroundBoardRef as any).drawMoveStr(
          activeLine.pv[0].substring(0, 2),
          activeLine.pv[0].substring(2, 4)
        );
      }
    },
    async newPosition(fen: string) {
      this.engineLines.clear();
      this.startFen = fen;

      this.moveHistoryLan = [];
      this.moveHistorySan = [];

      (this.$refs.chessGroundBoardRef as any).newPositionFen(fen);

      if (this.isRunning) {
        await this.sendEngineCommand("stop");
        this.chessProcess?.write("ucinewgame");
      }
    },
    async playMoves(moves: string) {
      await this.sendEngineCommand("stop");
      (this.$refs.chessGroundBoardRef as any).playMoves(moves);
    },
    async parsePgn(pgn: string) {
      await this.sendEngineCommand("stop");
      (this.$refs.chessGroundBoardRef as any).newPositionPgn(pgn);
    },
  },
});
</script>

<template>
  <div>
    <main class="analysis">
      <div class="game">
        <ChessGroundBoard
          ref="chessGroundBoardRef"
          @updated-sidetomove="updatedSideToMove"
          @updated-status="updatedStatus"
          @updated-move="updatedMove"
          @updated-cg="updatedCg"
        />
      </div>
      <div class="analysis-info">
        <Fen
          :fen="currentFen"
          :key="currentFen"
          @update-position="newPosition"
        />
        <div class="engine-status">
          <span class="engine-stat-value" :class="{ active: isRunning }">{{
            updateAnalysisStatus
          }}</span>
        </div>
        <EngineStats :engineInfo="engine_info" :sideToMove="sideToMove" />

        <div style="margin-top: 5px; margin-bottom: 5px">
          <v-tabs v-model="activeTabIndex" class="info-nav">
            <v-tab v-for="element in smallNavbar">
              {{ element.name }}
            </v-tab>
          </v-tabs>
        </div>
        <div class="info-content">
          <div class="nav-main-content">
            <EngineLines
              v-show="activeTab == 'engine-lines'"
              @send-moves="playMoves"
              :engineLines="engineLines"
              :fen="currentFen"
            />
            <EngineButtons
              v-if="activeTab == 'prompt'"
              @engine-command="sendEngineCommand"
              :go="status === '' || status === 'IDLE'"
              :status="isRunning"
              :key="isRunning.toString()"
            />
          </div>
          <div class="nav-secondary-content">
            <Pgn
              class="game-pgn"
              @send-pgn-moves="parsePgn"
              :movehistory="moveHistorySan"
              :key="currentFen"
            />
            <div class="analysis-graph"></div>
          </div>
        </div>
      </div>
    </main>
    <Sidebar />
  </div>
</template>

<style>
@import "@/assets/styles/chessground-theme.css";
@import "@/assets/styles/chessground-pieces.css";
@import "@/assets/styles/chessground.css";

main.analysis {
  padding: 1rem;
  color: aliceblue;
  height: 100vh;

  display: flex;
  box-sizing: border-box;
}

h1 {
  font-size: 2rem;
  font-weight: 400;
  color: rgb(252, 241, 222);
}

.game {
  display: flex;
  flex-direction: column;
  flex: 0 0 66%;
  box-sizing: border-box;
  max-width: 66%;
}

@media only screen and (min-width: 600px) {
  .board-space {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    width: 100%;
    height: 100%;
    padding-left: 5rem;
  }
}

@media only screen and (max-width: 600px) {
  .board-space {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    width: 100%;
    height: 100%;
  }
}

.analysis-info {
  flex: 1;
  display: flex;
  flex-direction: column;
}

.engine-status {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-evenly;
  align-items: center;
  background-color: var(--bg-tertiary);
  color: white;
  padding: 10px;
  margin-bottom: 5px;
  box-sizing: border-box;
  border-radius: 5px;
}

.engine-status .active {
  color: #22c55e;
}

.engine-status {
  color: #f43f5e;
}

.info-content {
  margin-top: 10px;
  flex: 1;
  display: flex;
  flex-wrap: nowrap;
  flex-direction: column;
}

.nav-main-content {
  display: flex;
  flex-direction: column;
  overflow-y: scroll;
  overflow-x: hidden;
  flex-grow: 0;
  height: calc(50vh - 100px);
  background-color: var(--bg-secondary);
  border-radius: 5px;
  margin-bottom: 5px;
}

.nav-secondary-content {
  display: flex;
  flex-direction: column;
  flex-grow: 0 !important;
  height: 50%;
  margin-bottom: 20px;
  gap: 10px;
}

.game-pgn {
  flex-basis: calc(50% - 5px);
  overflow-y: scroll;
  overflow-x: hidden;
}

.analysis-graph {
  flex-basis: calc(50% - 5px);
  background-color: #ae9b9b;
}

.promotion-options {
  position: absolute;
  width: 20%;
  height: auto;
  background-color: rgba(0, 0, 0, 0.5);
}

#promotion-select {
  display: flex;
  justify-content: space-evenly;
  align-items: center;
  min-width: 5%;
  padding: 5px;
}

#promotion-select button {
  background-color: transparent;
  border: none;
  outline: none;
  color: inherit;
  border: none;
  padding: 0;
  font: inherit;
  cursor: pointer;
  outline: inherit;
  z-index: 3;
}

#promotion-select .piece {
  width: 20%;
  height: 5rem;
  background-size: contain;
  background-repeat: no-repeat;
  display: inline-block;
}

.white-bishop {
  background-image: url("data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI0NSIgaGVpZ2h0PSI0NSI+PGcgZmlsbD0ibm9uZSIgZmlsbC1ydWxlPSJldmVub2RkIiBzdHJva2U9IiMwMDAiIHN0cm9rZS13aWR0aD0iMS41IiBzdHJva2UtbGluZWNhcD0icm91bmQiIHN0cm9rZS1saW5lam9pbj0icm91bmQiPjxnIGZpbGw9IiNmZmYiIHN0cm9rZS1saW5lY2FwPSJidXR0Ij48cGF0aCBkPSJNOSAzNmMzLjM5LS45NyAxMC4xMS40MyAxMy41LTIgMy4zOSAyLjQzIDEwLjExIDEuMDMgMTMuNSAyIDAgMCAxLjY1LjU0IDMgMi0uNjguOTctMS42NS45OS0zIC41LTMuMzktLjk3LTEwLjExLjQ2LTEzLjUtMS0zLjM5IDEuNDYtMTAuMTEuMDMtMTMuNSAxLTEuMzU0LjQ5LTIuMzIzLjQ3LTMtLjUgMS4zNTQtMS45NCAzLTIgMy0yeiIvPjxwYXRoIGQ9Ik0xNSAzMmMyLjUgMi41IDEyLjUgMi41IDE1IDAgLjUtMS41IDAtMiAwLTIgMC0yLjUtMi41LTQtMi41LTQgNS41LTEuNSA2LTExLjUtNS0xNS41LTExIDQtMTAuNSAxNC01IDE1LjUgMCAwLTIuNSAxLjUtMi41IDQgMCAwLS41LjUgMCAyeiIvPjxwYXRoIGQ9Ik0yNSA4YTIuNSAyLjUgMCAxIDEtNSAwIDIuNSAyLjUgMCAxIDEgNSAweiIvPjwvZz48cGF0aCBkPSJNMTcuNSAyNmgxME0xNSAzMGgxNW0tNy41LTE0LjV2NU0yMCAxOGg1IiBzdHJva2UtbGluZWpvaW49Im1pdGVyIi8+PC9nPjwvc3ZnPg==");
}

.white-knight {
  background-image: url("data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI0NSIgaGVpZ2h0PSI0NSI+PGcgZmlsbD0ibm9uZSIgZmlsbC1ydWxlPSJldmVub2RkIiBzdHJva2U9IiMwMDAiIHN0cm9rZS13aWR0aD0iMS41IiBzdHJva2UtbGluZWNhcD0icm91bmQiIHN0cm9rZS1saW5lam9pbj0icm91bmQiPjxwYXRoIGQ9Ik0yMiAxMGMxMC41IDEgMTYuNSA4IDE2IDI5SDE1YzAtOSAxMC02LjUgOC0yMSIgZmlsbD0iI2ZmZiIvPjxwYXRoIGQ9Ik0yNCAxOGMuMzggMi45MS01LjU1IDcuMzctOCA5LTMgMi0yLjgyIDQuMzQtNSA0LTEuMDQyLS45NCAxLjQxLTMuMDQgMC0zLTEgMCAuMTkgMS4yMy0xIDItMSAwLTQuMDAzIDEtNC00IDAtMiA2LTEyIDYtMTJzMS44OS0xLjkgMi0zLjVjLS43My0uOTk0LS41LTItLjUtMyAxLTEgMyAyLjUgMyAyLjVoMnMuNzgtMS45OTIgMi41LTNjMSAwIDEgMyAxIDMiIGZpbGw9IiNmZmYiLz48cGF0aCBkPSJNOS41IDI1LjVhLjUuNSAwIDEgMS0xIDAgLjUuNSAwIDEgMSAxIDB6bTUuNDMzLTkuNzVhLjUgMS41IDMwIDEgMS0uODY2LS41LjUgMS41IDMwIDEgMSAuODY2LjV6IiBmaWxsPSIjMDAwIi8+PC9nPjwvc3ZnPg==");
}

.white-rook {
  background-image: url("data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI0NSIgaGVpZ2h0PSI0NSI+PGcgZmlsbD0iI2ZmZiIgZmlsbC1ydWxlPSJldmVub2RkIiBzdHJva2U9IiMwMDAiIHN0cm9rZS13aWR0aD0iMS41IiBzdHJva2UtbGluZWNhcD0icm91bmQiIHN0cm9rZS1saW5lam9pbj0icm91bmQiPjxwYXRoIGQ9Ik05IDM5aDI3di0zSDl2M3ptMy0zdi00aDIxdjRIMTJ6bS0xLTIyVjloNHYyaDVWOWg1djJoNVY5aDR2NSIgc3Ryb2tlLWxpbmVjYXA9ImJ1dHQiLz48cGF0aCBkPSJNMzQgMTRsLTMgM0gxNGwtMy0zIi8+PHBhdGggZD0iTTMxIDE3djEyLjVIMTRWMTciIHN0cm9rZS1saW5lY2FwPSJidXR0IiBzdHJva2UtbGluZWpvaW49Im1pdGVyIi8+PHBhdGggZD0iTTMxIDI5LjVsMS41IDIuNWgtMjBsMS41LTIuNSIvPjxwYXRoIGQ9Ik0xMSAxNGgyMyIgZmlsbD0ibm9uZSIgc3Ryb2tlLWxpbmVqb2luPSJtaXRlciIvPjwvZz48L3N2Zz4=");
}

.white-queen {
  background-image: url("data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI0NSIgaGVpZ2h0PSI0NSI+PGcgZmlsbD0iI2ZmZiIgZmlsbC1ydWxlPSJldmVub2RkIiBzdHJva2U9IiMwMDAiIHN0cm9rZS13aWR0aD0iMS41IiBzdHJva2UtbGluZWNhcD0icm91bmQiIHN0cm9rZS1saW5lam9pbj0icm91bmQiPjxwYXRoIGQ9Ik04IDEyYTIgMiAwIDEgMS00IDAgMiAyIDAgMSAxIDQgMHptMTYuNS00LjVhMiAyIDAgMSAxLTQgMCAyIDIgMCAxIDEgNCAwek00MSAxMmEyIDIgMCAxIDEtNCAwIDIgMiAwIDEgMSA0IDB6TTE2IDguNWEyIDIgMCAxIDEtNCAwIDIgMiAwIDEgMSA0IDB6TTMzIDlhMiAyIDAgMSAxLTQgMCAyIDIgMCAxIDEgNCAweiIvPjxwYXRoIGQ9Ik05IDI2YzguNS0xLjUgMjEtMS41IDI3IDBsMi0xMi03IDExVjExbC01LjUgMTMuNS0zLTE1LTMgMTUtNS41LTE0VjI1TDcgMTRsMiAxMnoiIHN0cm9rZS1saW5lY2FwPSJidXR0Ii8+PHBhdGggZD0iTTkgMjZjMCAyIDEuNSAyIDIuNSA0IDEgMS41IDEgMSAuNSAzLjUtMS41IDEtMS41IDIuNS0xLjUgMi41LTEuNSAxLjUuNSAyLjUuNSAyLjUgNi41IDEgMTYuNSAxIDIzIDAgMCAwIDEuNS0xIDAtMi41IDAgMCAuNS0xLjUtMS0yLjUtLjUtMi41LS41LTIgLjUtMy41IDEtMiAyLjUtMiAyLjUtNC04LjUtMS41LTE4LjUtMS41LTI3IDB6IiBzdHJva2UtbGluZWNhcD0iYnV0dCIvPjxwYXRoIGQ9Ik0xMS41IDMwYzMuNS0xIDE4LjUtMSAyMiAwTTEyIDMzLjVjNi0xIDE1LTEgMjEgMCIgZmlsbD0ibm9uZSIvPjwvZz48L3N2Zz4=");
}
</style>
