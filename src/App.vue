<script lang="ts">
import Sidebar from "@/components/AppSideBar.vue";
import { exit } from "@tauri-apps/api/process";

export default {
  components: {
    Sidebar,
  },
  mounted() {
    window.addEventListener("keydown", this.handleKeydown);
  },
  beforeUnmount() {
    window.removeEventListener("keydown", this.handleKeydown);
  },
  methods: {
    async handleKeydown(event: KeyboardEvent) {
      if (event.key === "w" && event.ctrlKey) {
        await exit(0);
      }
    },
  },
};
</script>

<template>
  <div id="app">
    <Sidebar />
    <router-view />
  </div>
</template>

<style scoped></style>
