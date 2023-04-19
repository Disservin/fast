<script lang="ts">
import { defineComponent } from "vue";

import { Chessground } from "chessground";
import { Chess, SQUARES } from "chess.js";

import type { Move } from "@/ts/UciFilter";
import type { Square } from "chess.js";
import type { Color, Key } from "chessground/types";

type ChessgroundInstance = ReturnType<typeof Chessground>;

const startpos = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

export default defineComponent({
  data() {
    return {
      showPromotion: false,
      promotionMove: { origin: "", destination: "" },

      cg: null as ChessgroundInstance | null,
      game: new Chess(),

      moveHistoryLan: [] as string[],
      moveHistorySan: [] as string[],
    };
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

    this.newPositionFen(startpos);
  },
  beforeUnmount() {
    window.removeEventListener("resize", this.calculateSquareSize);
  },
  methods: {
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
      size -= size % 32; // ensure the size is a multiple of 32

      size = Math.min(size, 800);

      const boardWrap = document.querySelector(".board.cg-wrap") as HTMLElement;

      boardWrap.style.width = size + "px";
      boardWrap.style.height = size + "px";
      document.body.dispatchEvent(new Event("chessground.resize"));
    },
    clearLastMove() {
      this.cg?.set({ lastMove: undefined });
    },
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

      // emit new fen
      this.$emit("updated-cg", this.game.fen());
    },
    async drawMove(move: Move) {
      this.cg?.setShapes([
        {
          orig: move.orig as Key,
          dest: move.dest as Key,
          brush: "paleBlue",
        },
      ]);
    },
    async drawMoveStr(origin: string, dest: string) {
      this.cg?.setShapes([
        {
          orig: origin as Key,
          dest: dest as Key,
          brush: "paleBlue",
        },
      ]);
    },
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
    undo() {
      this.game.undo();

      this.moveHistoryLan.pop();
      this.moveHistorySan.pop();

      this.sendUpdates();
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
    async sendUpdates() {
      this.$emit("updated-move", {
        moveHistoryLan: this.moveHistoryLan,
        moveHistorySan: this.moveHistorySan,
      });

      this.updateCG();

      let status = "";

      if (this.game.isCheckmate()) {
        status = "CHECKMATE";
      } else if (this.game.isStalemate()) {
        status = "STALEMATE";
      } else if (this.game.isThreefoldRepetition()) {
        status = "THREEFOLD REPETITION";
      } else if (this.game.isInsufficientMaterial()) {
        status = "INSUFFICIENT MATERIAL";
      }

      this.$emit("updated-status", status);

      this.$emit("updated-sidetomove", this.toColor());
    },
    async updateMove(move: any) {
      if (move === null) {
        return "snapback";
      }

      this.moveHistoryLan.push(move.lan);
      this.moveHistorySan.push(move.san);

      this.sendUpdates();
    },
    async playMoves(moves: string) {
      const movesArray = moves.trim().split(" ");

      this.clearLastMove();

      for (let i = 0; i < movesArray.length; i++) {
        const move = movesArray[i];
        const chessMove = this.game.move(move);

        if (chessMove === null) {
          this.updateCG();

          return;
        }

        this.moveHistoryLan.push(chessMove.lan);
        this.moveHistorySan.push(chessMove.san);
      }

      this.sendUpdates();
    },
    async newPositionFen(fen: string) {
      this.game.load(fen);

      this.moveHistoryLan = [];
      this.moveHistorySan = [];

      this.clearLastMove();
      this.sendUpdates();
    },
    async newPositionPgn(pgn: string) {
      this.game.loadPgn(pgn);

      let history = this.game.history({ verbose: true });

      this.moveHistoryLan = [];
      this.moveHistorySan = [];

      history.forEach((move) => {
        this.moveHistoryLan.push(move.lan);
        this.moveHistorySan.push(move.san);
      });

      this.clearLastMove();
      this.sendUpdates();
    },
  },
});
</script>

<template>
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
</template>

<style>
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
</style>
