<template>
  <div id="hover-board" ref="smallBoard"></div>
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
    this.cg = Chessground(this.$refs.smallBoard as HTMLElement, {
      fen: this.fen,
      coordinates: false,
      viewOnly: true,
      addPieceZIndex: true,
    });

    const boardWrap = document.querySelector(
      "#hover-board.cg-wrap"
    ) as HTMLElement;

    const width = window.screen.width * 0.05;

    boardWrap.style.width = width + "px";
    boardWrap.style.height = width + "px";
    document.body.dispatchEvent(new Event("chessground.resize"));

    if (
      (this.$refs.smallBoard as HTMLElement).getBoundingClientRect().right >
      window.screen.width
    ) {
      boardWrap.style.left = -(width + 10) + "px";
    }
  },
  beforeUnmount() {
    this.cg!.destroy();
  },
});
</script>

<style>
#hover-board coords {
  display: none;
}

#hover-board {
  position: absolute;
  /* top: 100%; */
  bottom: 100%;
  right: 0;
}
</style>
