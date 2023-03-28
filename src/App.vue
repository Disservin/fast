<script setup lang="ts">
import Sidebar from "@/components/SideBar.vue";
</script>

<template>
  <div id="app">
    <router-view />
    <Sidebar />
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
