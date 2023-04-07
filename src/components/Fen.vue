<template>
  <div>
    <div class="fen-input">
      <v-text-field
        id="fen-input"
        v-model="fen"
        @input="updatePosition"
      ></v-text-field>
    </div>
  </div>
</template>

<script>
export default {
  data() {
    return {
      fen: "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    };
  },
  methods: {
    updatePosition() {
      // Code to update chess board with new FEN goes here
    },
  },
};
</script>

<style scoped>
@import "../assets/styles/variables.css";

.fen-input {
  margin-top: 5px;
  margin-bottom: 5px;
}
</style>
