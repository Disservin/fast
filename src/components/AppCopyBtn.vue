<template>
	<AppBtn @click="copyToClipBoard(copy)" :text="'copy pgn'" />
</template>

<script setup lang="ts">
// Components
import AppBtn from "@/components/AppBtn.vue";

defineProps<{
	copy: string;
}>();

const copyToClipBoard = (copy: string) => {
	navigator.clipboard.writeText(copy);
};
</script>

<style></style>
