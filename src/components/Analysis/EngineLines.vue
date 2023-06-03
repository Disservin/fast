<script setup lang="ts">
// Components
import SmallBoard from "@/components/Analysis/Board/SmallBoard.vue";

// JS
import { formatEval, formatPv } from "@/ts/FormatData";

// Libs
import { Chess } from "chess.js";

// Types
import type { PV } from "@/ts/PrincipalVariation";

// Utilities
import { ref, watch } from "vue";

const props = defineProps<{
	engineLines: Map<string, PV>;
	fen: string;
}>();

const emit = defineEmits(["send-moves"]);

const engineLinesSorted = ref([] as PV[]);
const showBoard = ref([-1, -1]);

watch(
	() => props.engineLines,
	(newVal) => {
		const lines: PV[] = Array.from(newVal.values());

		// sort others by depth
		lines.sort((a, b) => {
			if (Number(a.depth) > Number(b.depth)) {
				return -1;
			} else if (Number(a.depth) < Number(b.depth)) {
				return 1;
			} else {
				return 0;
			}
		});

		// move active line to top
		for (let i = 0; i < lines.length; i++) {
			if (lines[i].active) {
				// const line = lines[i];
				const line = lines.splice(i, 1);
				lines.unshift(line[0]);
				break;
			}
		}

		engineLinesSorted.value = lines;
	},
	{ deep: true }
);

const getFenForMove = (pvIndex: number, moveIndex: number) => {
	const fen = props.fen;
	const pv = engineLinesSorted.value[pvIndex].pv;

	const chess = new Chess(fen);

	for (let i = 0; i < moveIndex + 1; i++) {
		const move = pv[i].trim();

		if (chess.isGameOver()) {
			return chess.fen();
		}

		if (chess.move(move) === null) {
			return chess.fen();
		}
	}

	return chess.fen();
};

const sendMoves = (pvIndex: number, moveIndex: number) => {
	const pv = engineLinesSorted.value[pvIndex].pv;

	let moves = "";

	for (let i = 0; i <= moveIndex; i++) {
		if (i >= pv.length) {
			break;
		}

		moves += pv[i].trim() + " ";
	}

	moves.trim();

	emit("send-moves", moves);
};
</script>

<template>
  <div class="container">
    <div
      :class="{ active: value.active }"
      class="line"
      v-for="(value, index) in engineLinesSorted"
      :key="value.pv[0]"
    >
      <div class="eval-depth">
        <span
          :class="{ active: value.active }"
          class="stats"
        >
          {{ formatEval(value.score) }}/{{ value.depth }}
        </span>
      </div>
      <div class="pv">
        <span
          class="pv-move"
          v-for="(move, indexMove) in formatPv(value.pv)"
          :key="move.id"
          @click="sendMoves(index, indexMove)"
        >
          <span
            @mouseover="showBoard = [index, indexMove]"
            @mouseleave="showBoard = [-1, -1]"
          >
            {{ move.value }}&nbsp;
          </span>
          <SmallBoard
            v-if="showBoard[0] === index && showBoard[1] === indexMove"
            :fen="getFenForMove(index, indexMove)"
            style="left: 0"
          />
        </span>
      </div>
    </div>
    <div
      v-for="n in 5"
      :key="n"
      class="empty-line"
      style="height: 50px"
    />
  </div>
</template>

<style scoped>
@import "@/assets/styles/variables.css";
.container {
	overflow-y: scroll;
}

.line {
	background-color: var(--bg-tertiary);
	margin: 10px;
	border-radius: 5px;
	margin-bottom: 10px;
	padding: 10px;
}

.line span {
	font-family: Arial, sans-serif;
	font-size: 14px;
	line-height: 1.5;
}

.eval-depth {
	top: 0;
	left: 0;
	width: 10%;
	text-align: left;
	user-select: none;
	display: inline-block;
}

.eval-depth span.stats {
	font-size: 1rem;
	font-weight: bold;
}

.pv {
	display: inline-block;
	width: 90%;
	text-align: left;
	user-select: none;
	display: flex;
	flex-wrap: wrap;
}

.eval-depth span.active {
	color: #34d399;
}

.pv span.pv-move:hover {
	color: #34d399;
	background-color: #1e1e24;
	transform: scale(1);
	cursor: pointer;
}

.pv span.pv-move {
	padding: 5px;
	transition: transform 0.2s ease-in-out;
}
</style>
