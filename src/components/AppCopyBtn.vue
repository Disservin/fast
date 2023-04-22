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

<style></style>
