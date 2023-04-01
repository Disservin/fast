<script lang="ts">
import Sidebar from "@/components/SideBar.vue";

export default {
  name: "app",
  components: {
    Sidebar: Sidebar,
  },
  mounted() {
    this.calculateSquareSize();
    window.addEventListener("resize", this.calculateSquareSize);
  },
  beforeDestroy() {
    window.removeEventListener("resize", this.calculateSquareSize);
  },
  methods: {
    calculateSquareSize() {
      const board_space = this.$refs.boardSpace as HTMLElement;

      let width = board_space.offsetWidth - 7; // for the numbers on the side of the ground
      width -= width % 8; // fix chrome alignment errors; https://github.com/ornicar/lila/pull/3881

      let height = board_space.offsetHeight - 7;
      height -= height % 8;

      const max_size = Math.min(height, width);
      console.log("maxSize", max_size);

      const cgw = document.querySelector(".cg-board-wrap") as HTMLElement;

      cgw.style.width = max_size + "px";
      cgw.style.height = max_size + "px";

      document.body.dispatchEvent(new Event("chessground.resize"));
    },
  },
};
</script>

<template>
  <div>
    <main>
      <div class="game">
        <div class="board-space" ref="boardSpace">
          <chessboard class="board"></chessboard>
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
main {
  margin-left: 15rem;
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
