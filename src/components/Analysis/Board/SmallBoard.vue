<template>
  <div id="hover-board" ref="board"></div>
</template>

<script lang="ts">
import { defineComponent } from "vue";

import { Chessground } from "chessground";

type ChessgroundInstance = ReturnType<typeof Chessground>;

export default defineComponent({
  name: "SmallBoard",
  props: ["fen"],
  data() {
    return {
      cg: null as ChessgroundInstance | null,
    };
  },
  mounted() {
    // Initialize the Chessground board with the given FEN string
    this.cg = Chessground(this.$refs.board as HTMLElement, {
      fen: this.fen,
      coordinates: false,
      viewOnly: true,
      addPieceZIndex: true,
    });

    const boardWrap = document.querySelector(
      "#hover-board.cg-wrap"
    ) as HTMLElement;

    boardWrap.style.width = 100 + "px";
    boardWrap.style.height = 100 + "px";
    // boardWrap.style.zIndex = 4 + "";
    document.body.dispatchEvent(new Event("chessground.resize"));

    this.cg.redrawAll();
  },
  beforeUnmount() {
    this.cg!.destroy();
  },
});
</script>

<style scoped>
#hover-board coords {
  display: none;
}

#hover-board {
  position: absolute;
  top: 100%;
  left: 0;
}
</style>
