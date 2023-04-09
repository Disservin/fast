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
        this.child.write(data + "\n");
    }

    sendGo(): void {
        this.write("go");
    }

    sendGoDepth(depth: number): void {
        this.write(`go depth ${depth}`);
    }

    sendGoNodes(nodes: number): void {
        this.write(`go nodes ${nodes}`);
    }

    sendGoMovetime(movetime: number): void {
        this.write(`go movetime ${movetime}`);
    }

    sendOption(name: string, value: string): void {
        this.write(`setoption name ${name} value ${value}`);
    }

    sendStartpos(): void {
        this.write("position startpos");
    }

    sendStartposMoves(moves: string): void {
        this.write(`position startpos moves ${moves}`);
    }

    sendPosition(fen: string): void {
        this.write(`position fen ${fen}`);
    }

    sendPositionMoves(fen: string, moves: string): void {
        this.write(`position fen ${fen} moves ${moves}`);
    }

    sendStop(): void {
        this.write("stop");
    }

    async sendQuit() {
        this.write("quit");
        await this.delay(500);
        await this.child.kill();
    }

    kill(): void {
        this.child.kill();
    }
}

export default ChessProcess;
