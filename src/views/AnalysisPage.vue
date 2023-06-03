<script setup lang="ts">
import Sidebar from "@/components/AppSideBar.vue";
import AppBtn from "@/components/AppBtn.vue";
import AppCopyBtn from "@/components/AppCopyBtn.vue";
import EngineStats from "@/components/Analysis/EngineStats.vue";
import EngineButtons from "@/components/Analysis/EngineButtons.vue";
import FenBox from "@/components/Analysis/FenBox.vue";
import EngineLines from "@/components/Analysis/EngineLines.vue";
import PgnBox from "@/components/Analysis/PgnBox.vue";
import ChessGroundBoard from "@/components/Analysis/Board/BigBoard.vue";

import { extractScore } from "@/ts/ExtractData";
import { filterUCIInfo } from "@/ts/UciParsing";
import { getPV } from "@/ts/PrincipalVariation";
import ChessProcess from "@/ts/ChessProcess";

import type { EngineInfo } from "@/ts/UciParsing";
import type { PV } from "@/ts/PrincipalVariation";
import { ref } from "vue";
import { computed } from "vue";
import { onMounted } from "vue";
import { onBeforeMount } from "vue";
import { onUnmounted } from "vue";

const startpos = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

const chessProcess = ref<ChessProcess | null>(null);
const activeTabIndex = ref(0);

const smallNavbar = [
	{
		id: "engine-lines",
		name: "Engine Lines",
	},
	{
		id: "prompt",
		name: "Prompt",
	},
];

const engine_info = ref<EngineInfo>({
	score: "0",
	nodes: "0",
	nps: "0",
	depth: "0",
	time: "0",
	tbhits: "0",
	hashfull: "0",
});

const chessGroundBoardRef = ref();

const moveHistoryLan = ref<string[]>([]);
const moveHistorySan = ref<string[]>([]);

const engineLines = ref<Map<string, PV>>(new Map());

const startFen = ref(startpos);
const currentFen = ref(startpos);

const status = ref("IDLE");
const sideToMove = ref("white");

const evalHistory = ref<number[]>([]);
const graphTimer = ref<number | null>(null);

const series = ref([
	{
		name: "series-1",
		data: [] as number[],
	},
]);

const options = ref({
	colors: ["#1D4ED8"],
	stroke: {
		curve: "straight",
		width: 2.5,
	},
	markers: {
		size: 0,
		hover: {
			size: null,
			sizeOffset: 0,
		},
	},
	chart: {
		toolbar: {
			show: false,
		},
		zoom: {
			enabled: false,
		},
		animations: {
			enabled: false,
		},
	},
	legend: {
		show: false,
		onItemHover: {
			highlightDataSeries: false,
		},
	},
	tooltip: {
		enabled: false,
	},
	xaxis: {
		labels: {
			show: false,
		},
		type: "numeric",
	},
	yaxis: {
		tickAmount: 2,
		min: -5,
		max: 5,
		labels: {
			show: false,
		},
		opacity: 0,
	},
});

const activeTab = computed(() => {
	return smallNavbar[activeTabIndex.value].id;
});

const updateAnalysisStatus = (newStatus: string) => {
	if (newStatus === "" || newStatus === "IDLE") {
		if (chessProcess.value?.getIsRunning()) {
			return "ANALYSIS";
		} else if (chessProcess.value?.getIsAlive()) {
			return "READY";
		}
	} else {
		sendEngineCommand("stop");
	}

	return newStatus;
};

const currentPgn = computed(() => {
	return (chessGroundBoardRef.value as any).getPgn();
});

onMounted(() => {
	initEngine();

	graphTimer.value = setInterval(() => {
		const copy = [...evalHistory.value];
		series.value[0].data = copy;
	}, 50);

	window.addEventListener("keydown", handleKeydown);
});

onUnmounted(() => {
	if (graphTimer.value !== null) {
		clearInterval(graphTimer.value!);
	}

	window.removeEventListener("keydown", handleKeydown);

	sendEngineCommand("quit");
});

const parsePgnFromClipboard = () => {
	navigator.clipboard.readText().then((text) => {
		parsePgn(text);
	});
};

const evalFunction = (x: number) => {
	return (5 - Math.pow(2, -(Math.abs(x) - 2.319281))) * (x < 0 ? -1 : 1);
};

const normalizePerspectiveScore = (score: number) => {
	if (sideToMove.value === "black") {
		return -score;
	}
	return score;
};

const normalizeScoreStr = (score: string | undefined) => {
	if (score === undefined) return "";
	let norm = "";
	if (score.startsWith("cp")) {
		const cp = Number(score.slice(2));
		norm = "cp " + normalizePerspectiveScore(cp);
	} else if (score.startsWith("mate")) {
		const mateIn = Number(score.slice(4));
		norm = "mate " + normalizePerspectiveScore(mateIn);
	}
	return norm;
};

const getUciMoves = () => {
	return moveHistoryLan.value.join(" ");
};

const updatedBoard = async (data: any) => {
	sideToMove.value = data.sideToMove;
	status.value = data.status;

	moveHistoryLan.value = data.moveHistoryLan;
	moveHistorySan.value = data.moveHistorySan;

	let wasRunning = false;

	if (chessProcess.value?.getIsRunning()) {
		wasRunning = true;
		await sendEngineCommand("stop");
	}

	const score = extractScore(engine_info.value.score, sideToMove.value) / 100;
	evalHistory.value.push(evalFunction(normalizePerspectiveScore(score)));

	shiftInfoStats();

	let activeLine: PV = null as any;

	for (const value of engineLines.value.entries()) {
		if (value[1].active) {
			activeLine = value[1];
			break;
		}
	}

	if (activeLine && activeLine.pv.length > 0) {
		(chessGroundBoardRef.value as any).drawMoveStr(
			activeLine.pv[0].substring(0, 2),
			activeLine.pv[0].substring(2, 4)
		);
	}

	currentFen.value = data.fen;

	if (wasRunning) {
		sendEngineCommand("go");
	}
};

const handleKeydown = async (event: KeyboardEvent) => {
	if (
		event.key === "g" &&
		event.ctrlKey &&
		!chessProcess.value?.getIsRunning()
	) {
		event.preventDefault();
		sendEngineCommand("go");
	} else if (event.key === "h" && event.ctrlKey) {
		event.preventDefault();
		sendEngineCommand("stop");
	} else if (
		event.key === "r" &&
		event.ctrlKey &&
		!chessProcess.value?.getIsRunning()
	) {
		event.preventDefault();
		sendEngineCommand("restart");
	} else if (event.key === "n" && event.ctrlKey) {
		event.preventDefault();
		sendEngineCommand("stop");
		newPosition(startpos);
	} else if (event.key === "ArrowLeft") {
		(chessGroundBoardRef.value as any).undo();
	}
};

const clearInfoStats = () => {
	engineLines.value.clear();

	engine_info.value = {
		nodes: "0",
		nps: "0",
		depth: "0",
		time: "0",
		tbhits: "0",
		hashfull: "0",
	};
};

const updateInfoStats = async (line: string) => {
	if (
		!chessProcess.value!.getIsAlive() ||
		!chessProcess.value!.getIsRunning() ||
		!line.startsWith("info")
	) {
		return;
	}

	const filtered = filterUCIInfo(line);
	if (Object.keys(filtered).length === 0) {
		return;
	}

	filtered.score = normalizeScoreStr(filtered.score);

	if (filtered.score === "") {
		filtered.score = engine_info.value.score;
	}

	// only update changed values
	engine_info.value = { ...engine_info.value, ...filtered };

	if (
		engine_info.value.pv &&
		engine_info.value.pv.length > 0 &&
		engine_info.value.pv[0].orig !== "" &&
		engine_info.value.pv[0].dest !== "" &&
		chessProcess.value!.getIsAlive()
	) {
		(chessGroundBoardRef.value as any).drawMove(engine_info.value.pv[0]);
	}

	if (engine_info.value.score) {
		const score = extractScore(engine_info.value.score, sideToMove.value) / 100;
		const lastIndex = Math.max(0, evalHistory.value.length - 1);
		evalHistory.value[lastIndex] = evalFunction(
			normalizePerspectiveScore(score)
		);
	}

	const lines = getPV(line);
	lines.score = normalizeScoreStr(lines.score);

	if (lines.pv[0]) {
		for (const value of engineLines.value.entries()) {
			if (value[1].active) {
				value[1].active = false;
				break;
			}
		}

		lines.active = true;
		engineLines.value.set(lines.pv[0], lines);
	}
};

const shiftInfoStats = () => {
	// remove all other lines and shift the line with the played move to the right
	let correctLine: PV = null as any;
	const moves = moveHistoryLan.value;

	const playedMove = moves[moves.length - 1];

	engineLines.value.forEach((pv, key) => {
		if (key === playedMove) {
			correctLine = pv;
			correctLine.pv.shift();
		}
	});

	engineLines.value.clear();

	if (correctLine && correctLine.pv.length > 0) {
		engineLines.value.set(correctLine.pv[0], correctLine);
	}
};

const newPosition = async (fen: string) => {
	clearInfoStats();
	startFen.value = fen;

	moveHistoryLan.value = [];
	moveHistorySan.value = [];

	evalHistory.value = [];

	(chessGroundBoardRef.value as any).newPositionFen(fen);

	if (chessProcess.value?.getIsRunning()) {
		await sendEngineCommand("stop");
		await chessProcess.value?.ucinewgame();
	}
};

const playMoves = async (moves: string) => {
	const n = moves.trim().split(" ").length;
	const lastEval = evalHistory.value[evalHistory.value.length - 1];

	for (let i = 0; i < n; i++) {
		evalHistory.value.push(lastEval);
	}

	await sendEngineCommand("stop");
	(chessGroundBoardRef.value as any).playMoves(moves);
};

const parsePgn = async (pgn: string) => {
	await sendEngineCommand("stop");
	(chessGroundBoardRef.value as any).newPositionPgn(pgn);
};

const initEngine = async () => {
	const enginesData = localStorage.getItem("engines");
	const engines = enginesData ? JSON.parse(enginesData) : [];

	if (engines.length === 0) {
		return;
	}

	if (chessProcess.value) {
		await sendEngineCommand("quit");
	}

	chessProcess.value = new ChessProcess(engines[0].path, (line) => {
		updateInfoStats(line);
	});

	await chessProcess.value.start();
	await chessProcess.value.sendOptions(engines[0].settings);
};

const sendEngineCommand = async (command: string) => {
	if (command === "go" && (status.value === "" || status.value === "IDLE")) {
		clearInfoStats();

		if (!chessProcess.value!.getIsAlive()) {
			await initEngine();
		}

		if (startFen.value === startpos && getUciMoves() === "") {
			await chessProcess.value?.sendStartpos();
		} else if (startFen.value === startpos) {
			await chessProcess.value?.sendStartposMoves(getUciMoves());
		} else {
			await chessProcess.value?.sendPositionMoves(
				startFen.value,
				getUciMoves()
			);
		}
		await chessProcess.value?.sendGo();
	} else if (command === "stop") {
		await chessProcess.value?.sendStop();
	} else if (command === "quit") {
		await chessProcess.value?.sendStop();
		await chessProcess.value?.sendQuit();
	} else if (command === "restart") {
		clearInfoStats();

		await chessProcess.value?.sendStop();
		await chessProcess.value?.sendQuit();
		await initEngine();
	}
	localStorage.setItem("status", chessProcess.value!.getIsRunning().toString());
};
</script>

<template>
	<div>
		<main class="analysis">
			<div class="game">
				<ChessGroundBoard
					ref="chessGroundBoardRef"
					@updated-board="updatedBoard"
				/>
			</div>
			<div class="analysis-info">
				<FenBox
					:fen="currentFen"
					:key="currentFen"
					@update-position="newPosition"
				/>
				<div class="engine-status">
					<span
						class="engine-stat-value"
						:class="{ active: chessProcess?.getIsRunning() }"
						>{{ updateAnalysisStatus(status) }}</span
					>
				</div>
				<EngineStats :engine-info="engine_info" :side-to-move="sideToMove" />

				<div style="margin-top: 5px; margin-bottom: 5px">
					<v-tabs v-model="activeTabIndex" class="info-nav">
						<v-tab v-for="element in smallNavbar" :key="element.id">
							{{ element.name }}
						</v-tab>
					</v-tabs>
				</div>
				<div class="info-content">
					<div class="nav-main-content">
						<EngineLines
							v-show="activeTab == 'engine-lines'"
							@send-moves="playMoves"
							:engine-lines="engineLines"
							:fen="currentFen"
						/>
						<EngineButtons
							v-if="activeTab == 'prompt'"
							@engine-command="sendEngineCommand"
							:go="status === '' || status === 'IDLE'"
							:status="chessProcess?.getIsRunning()"
							:key="chessProcess?.getIsRunning().toString()"
						/>
						<AppBtn
							:text="'paste pgn'"
							v-if="activeTab == 'prompt'"
							@click="parsePgnFromClipboard"
						/>
						<AppCopyBtn :copy="currentPgn" v-if="activeTab == 'prompt'" />
					</div>
					<div class="nav-secondary-content">
						<PgnBox
							class="game-pgn"
							@send-pgn-moves="parsePgn"
							:movehistory="moveHistorySan"
							:key="currentFen"
						/>
						<div class="analysis-graph">
							<apexchart
								height="100%"
								:options="options"
								:series="series"
								type="line"
							/>
						</div>
					</div>
				</div>
			</div>
		</main>
		<Sidebar />
	</div>
</template>

<style>
@import "@/assets/styles/chessground-theme.css";
@import "@/assets/styles/chessground-pieces.css";
@import "@/assets/styles/chessground.css";

main.analysis {
	padding: 1rem;
	color: aliceblue;
	height: 100vh;

	display: flex;
	box-sizing: border-box;
}

h1 {
	font-size: 2rem;
	font-weight: 400;
	color: rgb(252, 241, 222);
}

.game {
	display: flex;
	flex-direction: column;
	flex: 0 0 66%;
	box-sizing: border-box;
	max-width: 66%;
}

.analysis-info {
	flex: 1;
	display: flex;
	flex-direction: column;
}

.engine-status {
	display: flex;
	flex-wrap: wrap;
	justify-content: space-evenly;
	align-items: center;
	background-color: var(--bg-tertiary);
	color: white;
	padding: 10px;
	margin-bottom: 5px;
	box-sizing: border-box;
	border-radius: 5px;
}

.engine-status .active {
	color: #22c55e;
}

.engine-status {
	color: #f43f5e;
}

.info-content {
	margin-top: 10px;
	flex: 1;
	display: flex;
	flex-wrap: nowrap;
	flex-direction: column;
}

.nav-main-content {
	display: flex;
	flex-direction: column;
	overflow-y: scroll;
	overflow-x: hidden;
	flex-grow: 0;
	height: calc(50vh - 100px);
	background-color: var(--bg-secondary);
	border-radius: 5px;
	margin-bottom: 5px;
}

.nav-secondary-content {
	display: flex;
	flex-direction: column;
	flex-grow: 0 !important;
	height: calc(40vh - 100px);
	margin-bottom: 20px;
	gap: 10px;
}

.game-pgn {
	flex-basis: calc(50% - 5px);
	overflow-y: scroll;
	overflow-x: hidden;
}

.analysis-graph {
	flex-basis: calc(45% - 5px);
	/* background-color: #ae9b9b; */
}

.promotion-options {
	position: absolute;
	width: 20%;
	height: auto;
	background-color: rgba(0, 0, 0, 0.5);
}

#promotion-select {
	display: flex;
	justify-content: space-evenly;
	align-items: center;
	min-width: 5%;
	padding: 5px;
}

#promotion-select button {
	background-color: transparent;
	border: none;
	outline: none;
	color: inherit;
	border: none;
	padding: 0;
	font: inherit;
	cursor: pointer;
	outline: inherit;
	z-index: 3;
}

#promotion-select .piece {
	width: 20%;
	height: 5rem;
	background-size: contain;
	background-repeat: no-repeat;
	display: inline-block;
}

.white-bishop {
	background-image: url("data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI0NSIgaGVpZ2h0PSI0NSI+PGcgZmlsbD0ibm9uZSIgZmlsbC1ydWxlPSJldmVub2RkIiBzdHJva2U9IiMwMDAiIHN0cm9rZS13aWR0aD0iMS41IiBzdHJva2UtbGluZWNhcD0icm91bmQiIHN0cm9rZS1saW5lam9pbj0icm91bmQiPjxnIGZpbGw9IiNmZmYiIHN0cm9rZS1saW5lY2FwPSJidXR0Ij48cGF0aCBkPSJNOSAzNmMzLjM5LS45NyAxMC4xMS40MyAxMy41LTIgMy4zOSAyLjQzIDEwLjExIDEuMDMgMTMuNSAyIDAgMCAxLjY1LjU0IDMgMi0uNjguOTctMS42NS45OS0zIC41LTMuMzktLjk3LTEwLjExLjQ2LTEzLjUtMS0zLjM5IDEuNDYtMTAuMTEuMDMtMTMuNSAxLTEuMzU0LjQ5LTIuMzIzLjQ3LTMtLjUgMS4zNTQtMS45NCAzLTIgMy0yeiIvPjxwYXRoIGQ9Ik0xNSAzMmMyLjUgMi41IDEyLjUgMi41IDE1IDAgLjUtMS41IDAtMiAwLTIgMC0yLjUtMi41LTQtMi41LTQgNS41LTEuNSA2LTExLjUtNS0xNS41LTExIDQtMTAuNSAxNC01IDE1LjUgMCAwLTIuNSAxLjUtMi41IDQgMCAwLS41LjUgMCAyeiIvPjxwYXRoIGQ9Ik0yNSA4YTIuNSAyLjUgMCAxIDEtNSAwIDIuNSAyLjUgMCAxIDEgNSAweiIvPjwvZz48cGF0aCBkPSJNMTcuNSAyNmgxME0xNSAzMGgxNW0tNy41LTE0LjV2NU0yMCAxOGg1IiBzdHJva2UtbGluZWpvaW49Im1pdGVyIi8+PC9nPjwvc3ZnPg==");
}

.white-knight {
	background-image: url("data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI0NSIgaGVpZ2h0PSI0NSI+PGcgZmlsbD0ibm9uZSIgZmlsbC1ydWxlPSJldmVub2RkIiBzdHJva2U9IiMwMDAiIHN0cm9rZS13aWR0aD0iMS41IiBzdHJva2UtbGluZWNhcD0icm91bmQiIHN0cm9rZS1saW5lam9pbj0icm91bmQiPjxwYXRoIGQ9Ik0yMiAxMGMxMC41IDEgMTYuNSA4IDE2IDI5SDE1YzAtOSAxMC02LjUgOC0yMSIgZmlsbD0iI2ZmZiIvPjxwYXRoIGQ9Ik0yNCAxOGMuMzggMi45MS01LjU1IDcuMzctOCA5LTMgMi0yLjgyIDQuMzQtNSA0LTEuMDQyLS45NCAxLjQxLTMuMDQgMC0zLTEgMCAuMTkgMS4yMy0xIDItMSAwLTQuMDAzIDEtNC00IDAtMiA2LTEyIDYtMTJzMS44OS0xLjkgMi0zLjVjLS43My0uOTk0LS41LTItLjUtMyAxLTEgMyAyLjUgMyAyLjVoMnMuNzgtMS45OTIgMi41LTNjMSAwIDEgMyAxIDMiIGZpbGw9IiNmZmYiLz48cGF0aCBkPSJNOS41IDI1LjVhLjUuNSAwIDEgMS0xIDAgLjUuNSAwIDEgMSAxIDB6bTUuNDMzLTkuNzVhLjUgMS41IDMwIDEgMS0uODY2LS41LjUgMS41IDMwIDEgMSAuODY2LjV6IiBmaWxsPSIjMDAwIi8+PC9nPjwvc3ZnPg==");
}

.white-rook {
	background-image: url("data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI0NSIgaGVpZ2h0PSI0NSI+PGcgZmlsbD0iI2ZmZiIgZmlsbC1ydWxlPSJldmVub2RkIiBzdHJva2U9IiMwMDAiIHN0cm9rZS13aWR0aD0iMS41IiBzdHJva2UtbGluZWNhcD0icm91bmQiIHN0cm9rZS1saW5lam9pbj0icm91bmQiPjxwYXRoIGQ9Ik05IDM5aDI3di0zSDl2M3ptMy0zdi00aDIxdjRIMTJ6bS0xLTIyVjloNHYyaDVWOWg1djJoNVY5aDR2NSIgc3Ryb2tlLWxpbmVjYXA9ImJ1dHQiLz48cGF0aCBkPSJNMzQgMTRsLTMgM0gxNGwtMy0zIi8+PHBhdGggZD0iTTMxIDE3djEyLjVIMTRWMTciIHN0cm9rZS1saW5lY2FwPSJidXR0IiBzdHJva2UtbGluZWpvaW49Im1pdGVyIi8+PHBhdGggZD0iTTMxIDI5LjVsMS41IDIuNWgtMjBsMS41LTIuNSIvPjxwYXRoIGQ9Ik0xMSAxNGgyMyIgZmlsbD0ibm9uZSIgc3Ryb2tlLWxpbmVqb2luPSJtaXRlciIvPjwvZz48L3N2Zz4=");
}

.white-queen {
	background-image: url("data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI0NSIgaGVpZ2h0PSI0NSI+PGcgZmlsbD0iI2ZmZiIgZmlsbC1ydWxlPSJldmVub2RkIiBzdHJva2U9IiMwMDAiIHN0cm9rZS13aWR0aD0iMS41IiBzdHJva2UtbGluZWNhcD0icm91bmQiIHN0cm9rZS1saW5lam9pbj0icm91bmQiPjxwYXRoIGQ9Ik04IDEyYTIgMiAwIDEgMS00IDAgMiAyIDAgMSAxIDQgMHptMTYuNS00LjVhMiAyIDAgMSAxLTQgMCAyIDIgMCAxIDEgNCAwek00MSAxMmEyIDIgMCAxIDEtNCAwIDIgMiAwIDEgMSA0IDB6TTE2IDguNWEyIDIgMCAxIDEtNCAwIDIgMiAwIDEgMSA0IDB6TTMzIDlhMiAyIDAgMSAxLTQgMCAyIDIgMCAxIDEgNCAweiIvPjxwYXRoIGQ9Ik05IDI2YzguNS0xLjUgMjEtMS41IDI3IDBsMi0xMi03IDExVjExbC01LjUgMTMuNS0zLTE1LTMgMTUtNS41LTE0VjI1TDcgMTRsMiAxMnoiIHN0cm9rZS1saW5lY2FwPSJidXR0Ii8+PHBhdGggZD0iTTkgMjZjMCAyIDEuNSAyIDIuNSA0IDEgMS41IDEgMSAuNSAzLjUtMS41IDEtMS41IDIuNS0xLjUgMi41LTEuNSAxLjUuNSAyLjUuNSAyLjUgNi41IDEgMTYuNSAxIDIzIDAgMCAwIDEuNS0xIDAtMi41IDAgMCAuNS0xLjUtMS0yLjUtLjUtMi41LS41LTIgLjUtMy41IDEtMiAyLjUtMiAyLjUtNC04LjUtMS41LTE4LjUtMS41LTI3IDB6IiBzdHJva2UtbGluZWNhcD0iYnV0dCIvPjxwYXRoIGQ9Ik0xMS41IDMwYzMuNS0xIDE4LjUtMSAyMiAwTTEyIDMzLjVjNi0xIDE1LTEgMjEgMCIgZmlsbD0ibm9uZSIvPjwvZz48L3N2Zz4=");
}
</style>
