<script setup lang="ts">
import Sidebar from "@/components/AppSideBar.vue";
import { exit } from "@tauri-apps/api/process";
import { onMounted } from "vue";
import { onBeforeUnmount } from "vue";

const handleKeydown = async (event: KeyboardEvent) => {
	if (event.key === "w" && event.ctrlKey) {
		await exit(0);
	}
};

onMounted(() => {
	window.addEventListener("keydown", handleKeydown);
});

onBeforeUnmount(() => {
	window.removeEventListener("keydown", handleKeydown);
});
</script>

<template>
	<div id="app">
		<Sidebar />
		<router-view />
	</div>
</template>

<style scoped></style>
