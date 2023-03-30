<script setup lang="ts">
import Sidebar from "@/components/SideBar.vue";
</script>

<template>
  <div>
    <main>
      <div class="game">
        <div class="engine-stats"></div>
        <chessboard class="board"></chessboard>
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
  box-sizing: border-box; /* Add this line */
}

h1 {
  font-size: 2rem;
  font-weight: 400;
  color: rgb(252, 241, 222);
}

.game {
  flex: 0 0 66%;
  background-color: #ccc;
  padding: 10px;
  box-sizing: border-box;
}

.engine-stats {
  background-color: #eee;
  padding: 10px;
  box-sizing: border-box;
  flex-basis: 10%;
}

.cg-board-wrap {
  width: 640px;
  height: 640px;
  position: relative;
  z-index: 0;
  /* display: inline-block; */
}

.blue.merida.board {
  width: 640px;
  height: 640px;
  margin: auto;
}

coord {
  /* font-size: 1.5rem; */
  color: #000000;
  font-weight: bold;
  font-size: 12px;

  /* font-family: ; */
}

.cg-board-wrap coords.files {
  top: 630px;
  left: 0px;
  z-index: 2;
  text-align: left;
  text-transform: lowercase;
  padding-bottom: 0px;
  margin-bottom: 5px;
}

.cg-board-wrap coords.ranks coord {
  transform: none;
}

.cg-board-wrap coords.ranks {
  right: 0px;
  z-index: 2;
}

.fen-input {
  background-color: #343434;
  padding: 10px;
  box-sizing: border-box;
  flex-basis: 10%;
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
  flex-direction: column; /* Set the direction to column */
}

.engine-lines {
  background-color: #240000;
  flex-basis: 100%; /* Set the width to be equal to the container */
}

.game-pgn {
  flex-basis: calc(
    50% - 5px
  ); /* Set the width to be half the container minus some padding */
  background-color: #843b3b;
}

.analysis-graph {
  flex-basis: calc(
    50% - 5px
  ); /* Set the width to be half the container minus some padding */

  background-color: #ae9b9b;
}
</style>
