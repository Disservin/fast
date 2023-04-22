<template>
  <div class="btn-wrap">
    <v-btn class="btn" color="primary" @click="copyToClipBoard(text)">
      <font-awesome-icon icon="fa-solid fa-clipboard" />
      <span>Copy PGN</span>
    </v-btn>
  </div>
</template>

<script lang="ts">
import { defineComponent } from "vue";
export default defineComponent({
  name: "AppCopyBtn",
  props: {
    text: {
      type: String,
      required: true,
    },
  },
  methods: {
    copyToClipBoard(text: string) {
      navigator.clipboard.writeText(text);
    },
  },
});
</script>

<style scoped>
.btn {
  border-radius: 5px;
  padding: 0.5rem;
  margin: 0.5rem;
  width: 100%;
}
.btn-wrap {
  display: flex;
  justify-content: center;
  background-color: var(--bg-tertiary);
  margin: 5px;
}
</style>
