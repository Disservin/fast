import { Child, Command } from "@tauri-apps/api/shell";

import type { Option } from "@/ts/FastTypes";

class ChessProcess {
	private command: Command;
	private callback: (data: string) => void;
	private child: Child | undefined;

	constructor(cmd: string, callback: (data: string) => void) {
		this.callback = callback;
		this.command = new Command(cmd);
		this.command.on("close", (data) => {});
		this.command.on("error", (error) =>
			console.error(`command error: "${error}"`)
		);
		this.command.stdout.on("data", (line) => this.callback(line));
		this.command.stderr.on("data", (line) =>
			console.error(`command stderr: "${line}"`)
		);
	}

	async delay(milliseconds: number) {
		return new Promise((resolve) => {
			setTimeout(resolve, milliseconds);
		});
	}

	async start(): Promise<void> {
		this.child = await this.command.spawn();

		this.write("uci");
	}

	async write(data: string) {
		await this.child!.write(data + "\n");
	}

	async sendGo() {
		this.write("go infinite");
	}

	async sendGoDepth(depth: number) {
		this.write(`go depth ${depth}`);
	}

	async sendGoNodes(nodes: number) {
		this.write(`go nodes ${nodes}`);
	}

	async sendGoMovetime(movetime: number) {
		this.write(`go movetime ${movetime}`);
	}

	async sendOption(name: string, value: string) {
		this.write(`setoption name ${name} value ${value}`);
	}

	async sendOptions(options: Option[]) {
		options.forEach(async (option: Option) => {
			if (
				option.value === "" ||
				option.name === "" ||
				option.value === undefined ||
				option.name === undefined
			) {
				return;
			}
			await this.sendOption(option.name, option.value);
		});
	}

	async sendStartpos() {
		this.write("position startpos");
	}

	async sendStartposMoves(moves: string) {
		this.write(`position startpos moves ${moves}`);
	}

	async sendPosition(fen: string) {
		this.write(`position fen ${fen}`);
	}

	async sendPositionMoves(fen: string, moves: string) {
		this.write(`position fen ${fen} moves ${moves}`);
	}

	async sendStop() {
		this.write("stop");
		await this.delay(10);
	}

	async sendQuit() {
		this.write("quit");
		await this.delay(100);
		await this.child!.kill();
	}

	async kill() {
		this.child!.kill();
		this.command.removeAllListeners();
	}
}

export default ChessProcess;
