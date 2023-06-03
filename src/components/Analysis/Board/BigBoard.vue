<script setup lang="ts">
import { ref, onMounted, onBeforeMount } from "vue";

import { Chessground } from "chessground";
import { Chess, SQUARES } from "chess.js";

import type { MoveStr } from "@/ts/UciParsing";
import type { Square, Move } from "chess.js";
import type { Color, Key } from "chessground/types";
import { onUnmounted } from "vue";

type ChessgroundInstance = ReturnType<typeof Chessground>;

const startpos = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

const showPromotion = ref(false);
const promotionMove = ref({ origin: "", destination: "" });

const game = ref(new Chess());
const cg = ref<ChessgroundInstance>();

const moveHistoryLan = ref<string[]>([]);
const moveHistorySan = ref<string[]>([]);

const board = ref<HTMLElement>();
const boardSpace = ref();

const emit = defineEmits(["updated-board"]);

onMounted(() => {
	const config = {
		movable: {
			color: "white" as Color,
			free: false,
			dests: toDests(),
		},
		draggable: {
			showGhost: true,
		},
		events: {
			move: makeMove,
		},
		highlight: {
			lastMove: true,
			check: true,
		},
		drawable: {
			enabled: false,
			eraseOnClick: false,
		},
	};

	cg.value = Chessground(board.value!, config);

	calculateSquareSize();
	window.addEventListener("resize", calculateSquareSize);

	newPositionFen(startpos);
});

onUnmounted(() => {
	window.removeEventListener("resize", calculateSquareSize);
});

const calculateSquareSize = () => {
	const bs = boardSpace.value as HTMLElement;

	// Lets get the width/height without padding and borders
	const cs = window.getComputedStyle(boardSpace.value);

	const paddingX = parseFloat(cs.paddingLeft) + parseFloat(cs.paddingRight);
	const paddingY = parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom);

	const borderX =
		parseFloat(cs.borderLeftWidth) + parseFloat(cs.borderRightWidth);
	const borderY =
		parseFloat(cs.borderTopWidth) + parseFloat(cs.borderBottomWidth);

	let size = Math.min(
		bs.offsetWidth - paddingX - borderX,
		bs.offsetHeight - paddingY - borderY
	);

	size -= 7; // adjust for borders and padding
	// fix chrome alignment errors; https://github.com/ornicar/lila/pull/3881
	size -= size % 32; // ensure the size is a multiple of 32

	size = Math.min(size, 800);

	const boardWrap = document.querySelector(".board.cg-wrap") as HTMLElement;

	boardWrap.style.width = size + "px";
	boardWrap.style.height = size + "px";
	document.body.dispatchEvent(new Event("chessground.resize"));
};

const getPgn = () => {
	return game.value.pgn();
};

const clearLastMove = () => {
	cg.value?.set({
		lastMove: [],
	});
};

const updateCG = () => {
	cg.value?.set({
		fen: game.value.fen(),
		turnColor: toColor(),
		movable: {
			color: toColor(),
			dests: toDests(),
		},
	});

	// set check highlighting
	if (game.value.inCheck()) {
		cg.value?.set({
			check: toColor(),
		});
	} else {
		cg.value?.set({
			check: undefined,
		});
	}
};

const drawMove = async (move: MoveStr) => {
	cg.value?.setShapes([
		{
			orig: move.orig as Key,
			dest: move.dest as Key,
			brush: "paleBlue",
		},
	]);
};

const drawMoveStr = async (origin: string, dest: string) => {
	cg.value?.setShapes([
		{
			orig: origin as Key,
			dest: dest as Key,
			brush: "paleBlue",
		},
	]);
};

const toColor = () => {
	return game.value.turn() === "w" ? "white" : "black";
};

const toDests = () => {
	const dests = new Map();
	SQUARES.forEach((s) => {
		const moves = game.value.moves({ square: s });
		if (moves.length) {
			dests.set(
				s,
				moves.map((m) => {
					// horrible hack to get the destination square
					// all because chess.js verbose printer is so slow,
					// this is a 30x speedup to the previous approach
					let to;
					if (m.includes("=")) {
						const index = m.indexOf("=");
						to = m.slice(index - 2, index);
					} else if (m.includes("x")) {
						const index = m.indexOf("x");
						to = m.slice(index + 1, index + 3);
					} else if (m === "O-O" || m === "O-O-O") {
						if (m === "O-O") {
							to = s === "e1" ? "g1" : "g8";
						} else {
							to = s === "e1" ? "c1" : "c8";
						}
					} else if (m.endsWith("+") || m.endsWith("#")) {
						to = m.slice(m.length - 3, m.length - 1);
					} else if (m.length == 2) {
						to = m;
					} else if (m.length == 3) {
						to = m.slice(1);
					} else {
						to = m.slice(m.length - 2, m.length);
					}
					return to;
				})
			);
		}
	});
	return dests;
};

const undo = () => {
	game.value.undo();

	moveHistoryLan.value.pop();
	moveHistorySan.value.pop();

	sendUpdates();
};

const makeMove = async (origin: string, destination: string) => {
	// is promotion?
	if (
		game.value.get(origin as Square)?.type === "p" &&
		(destination[1] === "1" || destination[1] === "8")
	) {
		showPromotion.value = true;
		promotionMove.value = { origin, destination };

		// user has to select a promotion piece and makePromotionMove() will be called
		return;
	}

	// do move
	const move = game.value.move({
		from: origin,
		to: destination,
	});

	updateMove(move);
};

const makePromotionMove = async (piece: string) => {
	showPromotion.value = false;

	// do move
	const move = game.value.move({
		from: promotionMove.value.origin,
		to: promotionMove.value.destination,
		promotion: piece,
	});

	updateMove(move);
};

const sendUpdates = async () => {
	let status = "";

	if (game.value.isCheckmate()) {
		status = "CHECKMATE";
	} else if (game.value.isStalemate()) {
		status = "STALEMATE";
	} else if (game.value.isThreefoldRepetition()) {
		status = "THREEFOLD REPETITION";
	} else if (game.value.isInsufficientMaterial()) {
		status = "INSUFFICIENT MATERIAL";
	}

	updateCG();

	emit("updated-board", {
		fen: game.value.fen(),
		moveHistoryLan: moveHistoryLan.value,
		moveHistorySan: moveHistorySan.value,
		status: status,
		sideToMove: toColor(),
	});
};

const updateMove = async (move: Move) => {
	if (move === null) {
		return "snapback";
	}

	moveHistoryLan.value.push(move.lan);
	moveHistorySan.value.push(move.san);

	sendUpdates();
};

const playMoves = async (moves: string) => {
	const movesArray = moves.trim().split(" ");

	clearLastMove();

	for (let i = 0; i < movesArray.length; i++) {
		const move = movesArray[i];
		const chessMove = game.value.move(move);

		if (chessMove === null) {
			updateCG();

			return;
		}

		moveHistoryLan.value.push(chessMove.lan);
		moveHistorySan.value.push(chessMove.san);
	}

	sendUpdates();
};

const newPositionFen = async (fen: string) => {
	game.value.load(fen);

	moveHistoryLan.value = [];
	moveHistorySan.value = [];

	clearLastMove();
	sendUpdates();
};

const newPositionPgn = async (pgn: string) => {
	game.value.loadPgn(pgn);

	const history = game.value.history({ verbose: true });

	moveHistoryLan.value = [];
	moveHistorySan.value = [];

	history.forEach((move) => {
		moveHistoryLan.value.push(move.lan);
		moveHistorySan.value.push(move.san);
	});

	clearLastMove();
	sendUpdates();
};

defineExpose({
	newPositionFen,
	newPositionPgn,
	playMoves,
	drawMove,
	drawMoveStr,
	getPgn,
	undo,
});
</script>

<template>
	<div class="board-space" ref="boardSpace">
		<div class="board" ref="board"></div>
		<div class="promotion-options" v-if="showPromotion">
			<div id="promotion-select">
				<button
					class="piece white-queen"
					@click="makePromotionMove('q')"
				></button>
				<button
					class="piece white-rook"
					@click="makePromotionMove('r')"
				></button>
				<button
					class="piece white-bishop"
					@click="makePromotionMove('b')"
				></button>
				<button
					class="piece white-knight"
					@click="makePromotionMove('n')"
				></button>
			</div>
		</div>
	</div>
</template>

<style>
@media only screen and (min-width: 600px) {
	.board-space {
		flex: 1;
		display: flex;
		flex-direction: column;
		justify-content: center;
		align-items: center;
		width: 100%;
		height: 100%;
		padding-left: 5rem;
	}
}

@media only screen and (max-width: 600px) {
	.board-space {
		flex: 1;
		display: flex;
		flex-direction: column;
		justify-content: center;
		align-items: center;
		width: 100%;
		height: 100%;
	}
}
</style>
