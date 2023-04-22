<template>
  <AppBtn @click="copyToClipBoard(copy)" :text="'copy pgn'" />
</template>

<script lang="ts">
import { defineComponent } from "vue";

import AppBtn from "@/components/AppBtn.vue";

export default defineComponent({
  name: "AppCopyBtn",
  components: {
    AppBtn,
  },
  props: {
    copy: {
      type: String,
      required: true,
    },
  },
  methods: {
    copyToClipBoard(copy: string) {
      navigator.clipboard.writeText(copy);
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
