import { extractMove } from "@/ts/ExtractData";

export interface MoveStr {
  orig: string;
  dest: string;
  prom?: string | null;
}

export interface EngineInfo {
  nodes?: string;
  nps?: string;
  pv?: MoveStr[];
  time?: string;
  hashfull?: string;
  score?: string;
  seldepth?: string;
  depth?: string;
  tbhits?: string;
}

export function filterUCIInfo(str: string): EngineInfo {
	const uciInfoStrings = [
		"nodes",
		"nps",
		"pv",
		"hashfull",
		"score",
		"seldepth",
		"depth",
		"tbhits",
	];
	const tokens = str.trim().split(" ");

	const engineInfo: EngineInfo = {};

	let keepPv = false;

	for (let i = 0; i < tokens.length; i++) {
		const token = tokens[i];
		if (token === "info") {
			continue;
		} else if (token === "depth") {
			engineInfo.depth = tokens[i + 1];
		} else if (token === "seldepth") engineInfo.seldepth = tokens[i + 1];
		else if (token === "score") {
			engineInfo.score = tokens[i + 1] + " " + tokens[i + 2];
		} else if (token === "lowerbound" || token === "upperbound") {
			keepPv = true;
		} else if (token === "time") {
			engineInfo.time = tokens[i + 1];
		} else if (token === "nodes") {
			engineInfo.nodes = tokens[i + 1];
		} else if (token === "nps") {
			engineInfo.nps = tokens[i + 1];
		} else if (token === "hashfull") {
			engineInfo.hashfull = Number(tokens[i + 1]) / 100 + "%";
		} else if (token === "tbhits") {
			engineInfo.tbhits = tokens[i + 1];
		} else if (token === "pv" && !keepPv) {
			engineInfo.pv = [];
			while (++i < tokens.length) {
				if (uciInfoStrings.includes(tokens[i])) break;
				else {
					const move = extractMove(tokens[i]);
					engineInfo.pv.push(move);
				}
			}
		}
	}
	return engineInfo;
}
