<script lang="ts">
import { defineComponent } from "vue";

import Sidebar from "@/components/AppSideBar.vue";
import EngineStats from "@/components/Analysis/EngineStats.vue";
import EngineButtons from "@/components/Analysis/EngineButtons.vue";
import Fen from "@/components/Analysis/Fen.vue";
import EngineLines from "@/components/Analysis/EngineLines.vue";
import Pgn from "@/components/Analysis/Pgn.vue";

import { Chessground } from "chessground";
import { Chess, SQUARES } from "chess.js";

import type { Square } from "chess.js";
import type { Color, Key } from "chessground/types";

import type { EngineInfo } from "@/ts/UciFilter";
import type { Option, Engine } from "@/ts/FastTypes";
import type { PV } from "@/ts/PrincipalVariation";

import { filterUCIInfo } from "@/ts/UciFilter";
import { extractPV } from "@/ts/PrincipalVariation";

import ChessProcess from "../ts/ChessProcess";

type ChessgroundInstance = ReturnType<typeof Chessground>;

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

      game: new Chess(),
      cg: null as ChessgroundInstance | null,

      showPromotion: false,
      promotionMove: { origin: "", destination: "" },

      engine_info: {
        score: "0",
        nodes: "0",
        nps: "0",
        depth: "0",
        time: "0",
        tbhits: "0",
        hashfull: "0",
      } as EngineInfo,

      activeEngine: null as null | Engine,
      isEngineAlive: false,
      isRunning: false,

      // keep track of the played moves, in san/uci notation
      engineMoves: "",
      // keep track of the played moves, in algebraic notation
      moveHistory: [] as string[],

      engineLines: new Map<string, PV>(),

      startFen: startpos,
      currentFen: startpos,
    };
  },
  computed: {
    activeTab(): string {
      return this.smallNavbar[this.activeTabIndex].id;
    },
    updateAnalysisStatus(): string {
      let status = "IDLE";

      if (this.isRunning) {
        status = "ANALYSIS";
      } else if (this.isEngineAlive) {
        status = "READY";
      }

      if (this.game.isCheckmate()) {
        status = "CHECKMATE";
      } else if (this.game.isStalemate()) {
        status = "STALEMATE";
      } else if (this.game.isThreefoldRepetition()) {
        status = "THREEFOLD REPETITION";
      } else if (this.game.isInsufficientMaterial()) {
        status = "INSUFFICIENT MATERIAL";
      } else if (this.game.isDraw()) {
        // shouldnt happen
        status = "DRAW";
      }

      if (this.game.isGameOver()) {
        this.sendEngineCommand("stop");
      }

      return status;
    },
  },
  mounted() {
    const config = {
      movable: {
        color: "white" as Color,
        free: false,
        dests: this.toDests(),
      },
      draggable: {
        showGhost: true,
      },
      events: {
        move: this.makeMove,
      },
      highlight: {
        lastMove: true,
        check: true,
      },
      drawable: {
        enabled: false,
        eraseOnClick: false,
      },
    };

    const board = this.$refs.board as HTMLElement;
    this.cg = Chessground(board, config);

    this.calculateSquareSize();
    window.addEventListener("resize", this.calculateSquareSize);
    window.addEventListener("keydown", this.handleKeydown);

    this.newPosition(startpos);
  },
  beforeUnmount() {
    this.isEngineAlive = false;
    this.isRunning = false;

    window.removeEventListener("resize", this.calculateSquareSize);

    this.chessProcess?.sendStop();
    this.chessProcess?.sendQuit();
  },
  methods: {
    updateCG() {
      this.cg?.set({
        fen: this.game.fen(),
        turnColor: this.toColor(),
        movable: {
          color: this.toColor(),
          dests: this.toDests(),
        },
      });

      // set check highlighting
      if (this.game.inCheck()) {
        this.cg?.set({
          check: this.toColor(),
        });
      } else {
        this.cg?.set({
          check: undefined,
        });
      }

      this.currentFen = this.game.fen();
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
    getPlayedMoves() {
      return this.engineMoves.trim();
    },
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
      if (
        !this.isEngineAlive ||
        !this.isRunning ||
        line === "" ||
        !line.startsWith("info")
      ) {
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
        this.drawAnalysisMove(
          this.engine_info.pv[0].orig,
          this.engine_info.pv[0].dest
        );
      }

      let lines = extractPV(line);

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
    },
    drawAnalysisMove(origin: string, destination: string) {
      this.cg?.setShapes([
        {
          orig: origin as Key,
          dest: destination as Key,
          brush: "paleBlue",
        },
      ]);
    },
    // CHESSBOARD METHODS
    toColor(): Color {
      return this.game.turn() === "w" ? "white" : "black";
    },
    // movable destionations for a piece
    toDests() {
      const dests = new Map();
      SQUARES.forEach((s) => {
        const moves = this.game.moves({ square: s });
        if (moves.length) {
          dests.set(
            s,
            moves.map((m) => {
              // horrible hack to get the destination square
              // all because chess.js verbose printer is so slow,
              // this is a 30x speedup to the previous approach
              let to;
              if (m.includes("=")) {
                const index = m.indexOf("=");
                to = m.slice(index - 2, index);
              } else if (m.includes("x")) {
                const index = m.indexOf("x");
                to = m.slice(index + 1, index + 3);
              } else if (m === "O-O" || m === "O-O-O") {
                if (m === "O-O") {
                  to = s === "e1" ? "g1" : "g8";
                } else {
                  to = s === "e1" ? "c1" : "c8";
                }
              } else if (m.endsWith("+") || m.endsWith("#")) {
                to = m.slice(m.length - 3, m.length - 1);
              } else if (m.length == 2) {
                to = m;
              } else if (m.length == 3) {
                to = m.slice(1);
              } else {
                to = m.slice(m.length - 2, m.length);
              }
              return to;
            })
          );
        }
      });
      return dests;
    },
    calculateSquareSize() {
      const boardSpace = this.$refs.boardSpace as HTMLElement;

      // Lets get the width/height without padding and borders
      const cs = window.getComputedStyle(boardSpace);

      const paddingX = parseFloat(cs.paddingLeft) + parseFloat(cs.paddingRight);
      const paddingY = parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom);

      const borderX =
        parseFloat(cs.borderLeftWidth) + parseFloat(cs.borderRightWidth);
      const borderY =
        parseFloat(cs.borderTopWidth) + parseFloat(cs.borderBottomWidth);

      let size = Math.min(
        boardSpace.offsetWidth - paddingX - borderX,
        boardSpace.offsetHeight - paddingY - borderY
      );

      size -= 7; // adjust for borders and padding
      // fix chrome alignment errors; https://github.com/ornicar/lila/pull/3881
      size -= size % 8; // ensure the size is a multiple of 8

      size = Math.min(size, 800);

      const boardWrap = document.querySelector(".board.cg-wrap") as HTMLElement;

      boardWrap.style.width = size + "px";
      boardWrap.style.height = size + "px";
      document.body.dispatchEvent(new Event("chessground.resize"));
    },
    /*
        Async functions
    */
    async playMoves(moves: string) {
      await this.sendEngineCommand("stop");

      this.engineLines.clear();

      const movesArray = moves.trim().split(" ");

      for (let i = 0; i < movesArray.length; i++) {
        if (this.game.isGameOver()) {
          this.currentFen = this.game.fen();

          return;
        }

        const move = movesArray[i];
        const chessMove = this.game.move(move);

        if (chessMove === null) {
          this.currentFen = this.game.fen();

          return;
        }

        this.engineMoves += move + " ";
        this.moveHistory.push(chessMove.san);
      }

      this.updateCG();
      this.cg?.set({ lastMove: undefined }); // clear last move
    },
    async newPgnMoves(pgn: string) {
      await this.sendEngineCommand("stop");
      this.clearAnalysisInfo();

      this.game.loadPgn(pgn);

      let history = this.game.history({ verbose: true });

      this.engineMoves = "";
      this.moveHistory = [];

      history.forEach((move) => {
        this.engineMoves += move.lan + " ";
        this.moveHistory.push(move.san);
      });

      this.updateCG();
      this.cg?.set({ lastMove: undefined }); // clear last move
    },
    async setupEngine() {
      const enginesData = localStorage.getItem("engines");
      const engines = enginesData ? JSON.parse(enginesData) : [];

      this.isEngineAlive = true;
      this.activeEngine = engines[0];

      if (this.chessProcess) {
        this.chessProcess.sendStop();
        await this.chessProcess.sendQuit();
      }

      this.chessProcess = new ChessProcess(engines[0].path, (line) => {
        this.updateInfoStats(line);
      });

      await this.chessProcess.start();
      this.chessProcess.sendOptions(this.activeEngine!.settings);
    },
    // button methods for engine
    async sendEngineCommand(command: string) {
      if (command === "go") {
        this.clearAnalysisInfo();

        if (!this.isEngineAlive) {
          await this.setupEngine();
        }

        if (this.startFen === startpos && this.getPlayedMoves() === "") {
          this.chessProcess?.sendStartpos();
        } else if (this.startFen === startpos) {
          this.chessProcess?.sendStartposMoves(this.getPlayedMoves());
        } else {
          this.chessProcess?.sendPositionMoves(
            this.startFen,
            this.getPlayedMoves()
          );
        }

        this.isRunning = true;
        this.chessProcess?.sendGo();
      } else if (command === "stop") {
        this.isRunning = false;
        this.chessProcess?.sendStop();
      } else if (command === "restart") {
        this.isRunning = false;
        this.activeEngine = null;
        this.isEngineAlive = false;

        this.clearAnalysisInfo();

        this.chessProcess?.sendStop();
        await this.chessProcess?.sendQuit();
        await this.setupEngine();
      }

      localStorage.setItem("status", this.isRunning.toString());

      // force update of buttons
      this.currentFen = this.game.fen();
    },
    async makePromotionMove(piece: string) {
      this.showPromotion = false;

      // do move
      const promotionMove = this.game.move({
        from: this.promotionMove.origin,
        to: this.promotionMove.destination,
        promotion: piece,
      });

      this.updateMove(promotionMove);
    },
    async makeMove(origin: string, destination: string) {
      // is promotion?
      if (
        this.game.get(origin as Square)?.type === "p" &&
        (destination[1] === "1" || destination[1] === "8")
      ) {
        this.showPromotion = true;
        this.promotionMove = { origin, destination };

        // user has to select a promotion piece and makePromotionMove() will be called
        return;
      }

      // do move
      const move = this.game.move({
        from: origin,
        to: destination,
      });

      this.updateMove(move);
    },
    async updateMove(move: any) {
      if (move === null) {
        return "snapback";
      }

      this.engineMoves += move.lan + " ";
      this.moveHistory.push(move.san);

      this.clearAnalysisInfo();
      this.updateCG();

      if (this.isRunning) {
        await this.sendEngineCommand("stop");
        await this.sendEngineCommand("go");
      }
    },
    async newPosition(fen: string) {
      this.engineLines.clear();
      this.game.load(fen);

      this.updateCG();
      this.cg!.set({ lastMove: undefined });

      this.engineMoves = "";
      this.moveHistory = [];
      this.startFen = this.game.fen();

      await this.sendEngineCommand("stop");
      this.chessProcess?.write("ucinewgame");
    },
  },
});
</script>

<template>
  <div>
    <main class="analysis">
      <div class="game">
        <div class="board-space" ref="boardSpace">
          <div class="board" ref="board"></div>
          <div class="promotion-options" v-if="showPromotion">
            <div id="promotion-select">
              <button
                class="piece white-queen"
                @click="makePromotionMove('q')"
              ></button>
              <button
                class="piece white-rook"
                @click="makePromotionMove('r')"
              ></button>
              <button
                class="piece white-bishop"
                @click="makePromotionMove('b')"
              ></button>
              <button
                class="piece white-knight"
                @click="makePromotionMove('n')"
              ></button>
            </div>
          </div>
        </div>
      </div>
      <div class="analysis-info">
        <Fen
          :fen="game.fen()"
          :key="game.fen()"
          @update-position="newPosition"
        />
        <div class="engine-status">
          <span class="engine-stat-value" :class="{ active: isRunning }">{{
            updateAnalysisStatus
          }}</span>
        </div>
        <EngineStats :engineInfo="engine_info" :sideToMove="toColor()" />

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
              :key="currentFen"
              :color="toColor()"
            />
            <EngineButtons
              v-if="activeTab == 'prompt'"
              @engine-command="sendEngineCommand"
              :status="isRunning"
              :key="isRunning.toString()"
            />
          </div>
          <div class="nav-secondary-content">
            <Pgn
              class="game-pgn"
              @send-pgn-moves="newPgnMoves"
              :movehistory="moveHistory"
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
