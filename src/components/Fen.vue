<template>
  <div>
    <div class="fen-input">
      <v-text-field
        id="fen-input"
        v-model="currentFen"
        @input="updatePosition"
      ></v-text-field>
    </div>
  </div>
</template>

<script>
export default {
  props: {
    fen: {
      type: String,
      required: true,
    },
  },
  data() {
    return {
      currentFen: this.fen,
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
