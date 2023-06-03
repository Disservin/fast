<template>
	<div id="hover-board" ref="smallBoard"></div>
</template>

<script setup lang="ts">
// Utilities
import { onMounted, ref, onBeforeUnmount } from "vue";

// Libraries
import { Chessground } from "chessground";

type ChessgroundInstance = ReturnType<typeof Chessground>;

const props = defineProps<{
	fen: string;
}>();

const cg = ref<ChessgroundInstance | null>(null);

onMounted(() => {
	cg.value = Chessground(
		document.querySelector("#hover-board") as HTMLElement,
		{
			fen: props.fen,
			coordinates: false,
			viewOnly: true,
			addPieceZIndex: true,
		}
	);

	const boardWrap = document.querySelector(
		"#hover-board.cg-wrap"
	) as HTMLElement;

	const outerDiv = document.querySelector(".nav-main-content") as HTMLElement;
	const innerDiv = boardWrap;
	const innerRect = innerDiv.getBoundingClientRect();
	const outerRect = outerDiv.getBoundingClientRect();

	const position = {
		top: innerRect.top - outerRect.top,
		left: innerRect.left - outerRect.left,
	};

	const width = Math.min(
		Math.max(window.screen.width * 0.1, outerRect.height / 4),
		outerRect.height / 2
	);

	boardWrap.style.width = width + "px";
	boardWrap.style.height = width + "px";

	if (
		(
			document.querySelector("#hover-board") as HTMLElement
		).getBoundingClientRect().right > window.screen.width
	) {
		boardWrap.style.left = -(width + 10) + "px";
	}

	if (outerRect.height - position.top + 10 < width) {
		boardWrap.style.removeProperty("top");
		boardWrap.style.setProperty("bottom", "100%");
	} else {
		boardWrap.style.removeProperty("bottom");
		boardWrap.style.setProperty("top", "100%");
	}

	document.body.dispatchEvent(new Event("chessground.resize"));
});

onBeforeUnmount(() => {
	cg.value!.destroy();
});
</script>

<style>
#hover-board coords {
	display: none;
}

#hover-board {
	position: absolute;
	right: 0;
}
</style>
