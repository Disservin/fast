<script lang="ts">
import Sidebar from "@/components/SideBar.vue";

import { defineComponent } from "vue";

import type { Color, Key } from "chessground/types";
import { Chessground } from "chessground";

import { Chess, SQUARES, type Move, type Square } from "chess.js";
import { invoke } from "@tauri-apps/api";

type ChessgroundInstance = ReturnType<typeof Chessground>;

export default defineComponent({
  name: "app",
  components: {
    Sidebar: Sidebar,
  },
  async mounted() {
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
    };

    const board = this.$refs.board as HTMLElement;
    this.cg = Chessground(board, config);

    this.calculateSquareSize();
    window.addEventListener("resize", this.calculateSquareSize);

    const enginesData = localStorage.getItem("engines");
    const engines = enginesData ? JSON.parse(enginesData) : [];

    const activeEngine = engines[0];

    // await invoke("new", { command: activeEngine.path });
    // await invoke("go");

    this.timer = setInterval(() => {
      this.info();
    }, 1000);
  },
  async beforeDestroy() {
    window.removeEventListener("resize", this.calculateSquareSize);
    clearInterval(this.timer as number);

    // await invoke("stop");
    // await invoke("quit");
  },

  data() {
    return {
      game: new Chess(),
      cg: null as ChessgroundInstance | null,
      showPromotion: false,
      promotionMove: { origin: "", destination: "" },
      engine_info: "test",
      timer: null as number | null,
      small_navbar: ["Engine Lines", "Prompt", "Settings"],
      isActive: "Engine Lines",
    };
  },
  methods: {
    setActive(element: string, index: number) {
      this.isActive = element;
    },
    async info() {
      const info: string = await invoke("read_line");
      if (info != "") {
        this.engine_info = info;

        // draw engine pv move on board
        const pv = info.split(" pv ")[1];
        if (pv) {
          const pvMoves = pv.split(" ");
          const origin = pvMoves[0].substring(0, 2);
          const destination = pvMoves[0].substring(2, 4);
          console.log(origin, destination);

          this.drawMove(origin, destination);
        }
      }
    },
    drawMove(origin: string, destination: string) {
      this.cg?.setShapes([
        {
          orig: origin as Key,
          dest: destination as Key,
          brush: "paleBlue",
        },
      ]);
    },
    setPromotionPiece(piece: string) {
      this.showPromotion = false;

      // do move
      const promotionMove = this.game.move({
        from: this.promotionMove.origin,
        to: this.promotionMove.destination,
        promotion: piece,
      });

      // set check highlighting
      if (this.game.inCheck()) {
        this.cg?.set({
          check: this.toColor(),
        });
      }

      if (promotionMove === null) {
        return "snapback";
      }

      // update chessground board
      this.cg!.set({
        fen: this.game.fen(),
        turnColor: this.toColor(),
        movable: {
          color: this.toColor(),
          dests: this.toDests(),
        },
      });
    },
    makeMove(origin: string, destination: string) {
      const sq = origin as Square;

      // is promotion?
      if (
        this.game.get(sq)?.type === "p" &&
        (destination[1] === "1" || destination[1] === "8")
      ) {
        this.showPromotion = true;
        this.promotionMove = { origin, destination };

        // user has to select a promotion piece and setPromotionPiece() will be called
        return;
      } else {
        const move = this.game.move({ from: origin, to: destination });

        // check highlighting
        if (this.game.inCheck()) {
          this.cg?.set({
            check: this.toColor(),
          });
        }

        if (move === null) {
          return "snapback";
        }
      }

      // update chessground board
      this.cg!.set({
        turnColor: this.toColor(),
        movable: {
          color: this.toColor(),
          dests: this.toDests(),
        },
      });

      this.drawMove(origin, destination);
    },
    toColor(): Color {
      return this.game.turn() === "w" ? "white" : "black";
    },
    // movable destionations for a piece
    toDests(): Map<Key, Key[]> {
      const dests = new Map();
      SQUARES.forEach((s) => {
        const ms = this.game.moves({ square: s, verbose: true });
        if (ms.length)
          dests.set(
            s,
            ms.map((m) => m.to)
          );
      });
      return dests;
    },
    calculateSquareSize() {
      //   let width = board_space.offsetWidth - 7; // for the numbers on the side of the ground
      //   width -= width % 8; // fix chrome alignment errors; https://github.com/ornicar/lila/pull/3881
      const boardSpace = this.$refs.boardSpace as HTMLElement;
      const boardWrap = document.querySelector(".cg-wrap") as HTMLElement;
      const rect = boardSpace.getBoundingClientRect();
      let size = Math.min(rect.width, rect.height);
      size -= 7; // adjust for borders and padding
      size -= size % 8; // ensure the size is a multiple of 8
      boardWrap.style.width = size + "px";
      boardWrap.style.height = size + "px";
      document.body.dispatchEvent(new Event("chessground.resize"));
      // idk why i need to do this, but it works it also yeets the call stack
      window.dispatchEvent(new Event("resize"));
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
                @click="setPromotionPiece('q')"
              ></button>
              <button
                class="piece white-rook"
                @click="setPromotionPiece('r')"
              ></button>
              <button
                class="piece white-bishop"
                @click="setPromotionPiece('b')"
              ></button>
              <button
                class="piece white-knight"
                @click="setPromotionPiece('n')"
              ></button>
            </div>
          </div>
        </div>
        <div class="engine-stats">
          {{ engine_info }}
        </div>
        <div class="fen-input"></div>
      </div>
      <div class="analysis-info">
        <div class="info-nav">
          <ul>
            <li
              v-for="(element, index) in small_navbar"
              :class="{ active: isActive === element }"
              :key="element"
              @click="setActive(element, index)"
            >
              {{ element }}
            </li>
          </ul>
        </div>
        <div class="info-content">
          <div class="engine-lines"></div>
          <div class="game-pgn"></div>
          <div class="analysis-graph"></div>
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
}

.engine-stats {
  flex: 0 0 10%;
  padding: 10px;
  box-sizing: border-box;
  background-color: #843b3b;
}

.board-space {
  flex: 1;
  display: flex;
  justify-content: center;
  align-items: center;
  width: 100%;
  height: 100%;
}

.test {
  background-color: aquamarine;
}

.fen-input {
  flex: 0 0 10%;
  padding: 10px;
  box-sizing: border-box;
  background-color: #343434;
}

.analysis-info {
  flex: 1;
  display: flex;
  flex-direction: column;
}

.info-nav {
  background-color: #333;
  color: #fff;
  padding: 10px;
}

.info-nav ul {
  display: flex;
  justify-content: space-evenly;
  align-items: center;
  list-style: none;
  margin: 0;
  padding: 0;
}

.info-nav li {
  padding: 10px;
  cursor: pointer;
  flex-grow: 1;
  text-align: center;
}

.info-nav li.active {
  background-color: #fff;
  color: #333;
}

.info-nav li.active:hover {
  background-color: #fff;
  color: #333;
}

.info-nav li:hover {
  background-color: #5f5f5f;
}

.info-content {
  flex: 1;
  display: flex;
  flex-wrap: nowrap;
  flex-direction: column;
}

.engine-lines {
  background-color: #240000;
  flex-basis: 100%;
}

.game-pgn {
  flex-basis: calc(50% - 5px);
  background-color: #843b3b;
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

.piece {
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

.engine-stats {
  padding-left: 5rem;
}

.fen-input {
  padding-left: 5rem;
}
</style>
