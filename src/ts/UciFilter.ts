export interface Move {
    orig: string;
    dest: string;
    prom?: string | null;
}

export interface EngineInfo {
    nodes?: string;
    nps?: string;
    pv?: Move[];
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

    for (let i = 0; i < tokens.length; i++) {
        const token = tokens[i];
        if (token === "info") {
            continue;
        } else if (token === "depth") {
            engineInfo.depth = tokens[i + 1];
        } else if (token === "seldepth") engineInfo.seldepth = tokens[i + 1];
        else if (token === "score") {
            engineInfo.score = tokens[i + 1] + " " + tokens[i + 2];
        } else if (token === "time") {
            engineInfo.time = tokens[i + 1];
        } else if (token === "nodes") {
            engineInfo.nodes = tokens[i + 1];
        } else if (token === "nps") {
            engineInfo.nps = tokens[i + 1];
        } else if (token === "hashfull") {
            engineInfo.hashfull = tokens[i + 1];
        } else if (token === "tbhits") {
            engineInfo.tbhits = tokens[i + 1];
        } else if (token === "pv") {
            engineInfo.pv = [];
            while (++i < tokens.length) {
                if (uciInfoStrings.includes(tokens[i])) break;
                else {
                    const move = tokens[i];
                    const origin = move.substring(0, 2);
                    const destination = move.substring(2, 4);
                    let promotion = null;

                    if (move.length == 5) {
                        promotion = move.substring(4, 5);
                    }

                    engineInfo.pv.push({
                        orig: origin,
                        dest: destination,
                        prom: promotion,
                    });
                }
            }
        }
    }
    return engineInfo;
}
