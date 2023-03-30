<script setup lang="ts">
import Sidebar from "@/components/SideBar.vue";
</script>

<template>
  <div id="app">
    <Sidebar />
    <router-view />
  </div>
</template>

<style scoped>
.content {
  margin-left: 5rem;
  padding: 1rem;
  color: aliceblue;
}
main {
  margin-left: 5rem;
  padding: 1rem;
  color: aliceblue;
}
</style>
