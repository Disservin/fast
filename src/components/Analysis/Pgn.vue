<template>
  <div class="pgn-display">
    <pre>{{ formattedPgn }}</pre>
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
      console.log(this.gamePgn);
      let format = "";

      moves.forEach((move, index) => {
        if (index > 0 && index % 3 == 0) {
          format += "\n";
        }
        format += move + " ";
      });

      return format;
    },
  },
};
</script>

<style scoped>
.pgn-display {
  font-family: monospace;
}
</style>
