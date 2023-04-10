<template>
  <div class="pgn-display">
    <div class="pgn-move" v-for="move in formattedPgn" :key="move">
      {{ move }}
    </div>
  </div>
</template>

<script lang="ts">
export default {
  props: {
    gamePgn: {
      type: String,
      required: true,
    },
  },
  computed: {
    formattedPgn() {
      const moves: string[] = this.gamePgn.split(" ");
      let formatted: string[] = [];

      let new_move = "";
      moves.forEach((move, index) => {
        if (move.endsWith(".")) {
          new_move = " " + move;
          return;
        }

        new_move += " " + move;

        formatted.push(new_move);
        new_move = "";
      });

      return formatted;
    },
  },
};
</script>

<style scoped>
.pgn-display {
  font-family: monospace;
  background-color: var(--bg-secondary);
  border-radius: 5px;
}

.pgn-move {
  padding: 5px;
  user-select: none;
  display: inline-block;
  transition: transform 0.2s ease-in-out;
  border-radius: 5px;
}

.pgn-move:hover {
  cursor: pointer;
  color: #34d399;
  background-color: #1e1e24;
  transform: scale(1.2) perspective(10px);
}
</style>
