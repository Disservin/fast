<script lang="ts">
import Sidebar from "@/components/SideBar.vue";

import { Chessground } from "chessground";
import { defineComponent } from "vue";

export default defineComponent({
  name: "app",
  components: {
    Sidebar: Sidebar,
  },
  mounted() {
    const config = {};
    const board = this.$refs.board as HTMLElement;
    const ground = Chessground(board, config);

    this.calculateSquareSize();
    window.addEventListener("resize", this.calculateSquareSize);
  },
  beforeDestroy() {
    window.removeEventListener("resize", this.calculateSquareSize);
  },
  created() {},
  methods: {
    calculateSquareSize() {
      //   const board_space = this.$refs.boardSpace as HTMLElement;
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
      window.dispatchEvent(new Event("resize"));
    },
  },
});
</script>

<template>
  <div>
    <main>
      <div class="game">
        <div class="board-space" ref="boardSpace">
          <div class="board" ref="board"></div>
        </div>
        <div class="engine-stats"></div>
        <div class="fen-input"></div>
      </div>
      <div class="analysis-info">
        <div class="info-nav"></div>
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

main {
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

/* .cg-board-wrap {
  width: 400px;
  height: 400px;
  max-width: 100%;
  max-height: 100%;
} */

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
</style>
