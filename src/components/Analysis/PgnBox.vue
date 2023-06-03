<template>
	<div class="pgn-display">
		<div
			class="pgn-move"
			v-for="(move, index) in formattedPgn"
			:key="move.id"
			@click="sendPGNMoves(index)"
		>
			{{ move.value }}
		</div>
	</div>
</template>

<script setup lang="ts">
// JS
import { formatPv } from "@/ts/FormatData";

// Utilities
import { computed } from "vue";

const props = defineProps<{
	movehistory: string[];
}>();

const emit = defineEmits(["send-pgn-moves"]);

const formattedPgn = computed(() => {
	return formatPv(props.movehistory as string[]);
});

const sendPGNMoves = (moveIndex: number) => {
	const formatted = formatPv(props.movehistory as string[]);
	let pgn = "";
	formatted.forEach((move, index) => {
		if (index > moveIndex) {
			return;
		}
		pgn += move.value.trim() + " ";
	});
	pgn = pgn.trim();
	emit("send-pgn-moves", pgn);
};
</script>

<style scoped>
.pgn-display {
	font-family: monospace;
	background-color: var(--bg-secondary);
	border-radius: 5px;
}

.pgn-move {
	padding: 5px;
	user-select: none;
	display: inline-block;
	transition: transform 0.2s ease-in-out;
	border-radius: 5px;
}

.pgn-move:hover {
	cursor: pointer;
	color: #34d399;
	background-color: #1e1e24;
	transform: scale(1.2) perspective(10px);
}
</style>
