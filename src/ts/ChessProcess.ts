import { Command } from "@tauri-apps/api/shell";

class ChessProcess {
    private command: Command;
    private callback: (data: string) => void;
    private child: any;

    constructor(cmd: string, callback: (data: string) => void) {
        this.callback = callback;
        this.command = new Command(cmd);
        this.command.on("close", (data) => {
            console.log(
                `command finished with code ${data.code} and signal ${data.signal}`
            );
        });
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
        console.log("pid:", this.child.pid);
    }

    write(data: string): void {
        this.child.write(data);
    }

    sendGo(): void {
        this.child.write("go\n");
    }

    sendGoDepth(depth: number): void {
        this.child.write(`go depth ${depth}\n`);
    }

    sendGoNodes(nodes: number): void {
        this.child.write(`go nodes ${nodes}\n`);
    }

    sendGoMovetime(movetime: number): void {
        this.child.write(`go movetime ${movetime}\n`);
    }

    sendOption(name: string, value: string): void {
        this.child.write(`setoption name ${name} value ${value}\n`);
    }

    sendStartpos(): void {
        this.child.write("position startpos\n");
    }

    sendStartposMoves(moves: string): void {
        this.child.write(`position startpos moves ${moves}\n`);
    }

    sendPosition(fen: string): void {
        this.child.write(`position fen ${fen}\n`);
    }

    sendPositionMoves(fen: string, moves: string): void {
        this.child.write(`position fen ${fen} moves ${moves}\n`);
    }

    sendStop(): void {
        this.child.write("stop\n");
    }

    async sendQuit() {
        this.child.write("quit\n");
        await this.delay(500);
        await this.child.kill();
    }

    kill(): void {
        this.child.kill();
    }
}

export default ChessProcess;
