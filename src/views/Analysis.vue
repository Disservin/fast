<template>
  <div>
    <main>
      <h1>Analysis</h1>
      <div id="myBoard" style="width: 400px"></div>
    </main>
  </div>
</template>

<script lang="ts">
import { Chessboard2 } from "@chrisoakman/chessboard2/dist/chessboard2.min.mjs";
import "@chrisoakman/chessboard2/dist/chessboard2.min.css";
export default {
  mounted: function () {
    const config = {
      draggable: true,
      dropOffBoard: "snapback", // this is the default
      position: "start",
    };
    const board = Chessboard2("myBoard", config);
  },
};
</script>
