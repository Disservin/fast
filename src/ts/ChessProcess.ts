import { Child, Command } from "@tauri-apps/api/shell";

import type { Option } from "@/ts/FastTypes";

class ChessProcess {
	private command: Command;
	private callback: (data: string) => void;
	private child: Child | undefined;
	private isRunning: boolean = false;
	private isAlive: boolean = false;

	constructor(cmd: string, callback: (data: string) => void) {
		this.callback = callback;
		this.command = new Command(cmd);
		this.command.on("close", (data: string) => {});
		this.command.on("error", (error: string) =>
			console.error(`command error: "${error}"`)
		);
		this.command.stdout.on("data", (line: string) => this.callback(line));
		this.command.stderr.on("data", (line: string) =>
			console.error(`command stderr: "${line}"`)
		);
	}

	private async delay(milliseconds: number) {
		return new Promise((resolve) => {
			setTimeout(resolve, milliseconds);
		});
	}

	private async write(data: string) {
		await this.child!.write(data + "\n");
	}

	public getIsRunning(): boolean {
		return this.isRunning;
	}

	public getIsAlive(): boolean {
		return this.isAlive;
	}

	async start(): Promise<void> {
		this.isAlive = true;
		this.child = await this.command.spawn();

		await this.write("uci");
	}

	async ucinewgame() {
		await this.write("ucinewgame");
	}

	async sendGo() {
		this.isRunning = true;
		await this.write("go infinite");
	}

	async sendOption(name: string, value: string) {
		await this.write(`setoption name ${name} value ${value}`);
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
		await this.write("position startpos");
	}

	async sendStartposMoves(moves: string) {
		await this.write(`position startpos moves ${moves}`);
	}

	async sendPosition(fen: string) {
		await this.write(`position fen ${fen}`);
	}

	async sendPositionMoves(fen: string, moves: string) {
		await this.write(`position fen ${fen} moves ${moves}`);
	}

	async sendStop() {
		this.isRunning = false;
		this.write("stop");
		await this.delay(10);
	}

	async sendQuit() {
		this.isAlive = false;
		this.isRunning = false;
		await this.write("quit");
		await this.delay(100);
		await this.child!.kill();
	}
}

export default ChessProcess;
