export function formatNumber(numStr: string): string {
    const num = parseFloat(numStr);
    if (num >= 1e12) {
        return (num / 1e12).toFixed(1) + "T";
    } else if (num >= 1e9) {
        return (num / 1e9).toFixed(1) + "B";
    } else if (num >= 1e6) {
        return (num / 1e6).toFixed(1) + "M";
    } else if (num >= 1e3) {
        return (num / 1e3).toFixed(1) + "K";
    } else {
        return num.toFixed(0);
    }
}

export function formatTime(timeInMs: string): string {
    const time = parseInt(timeInMs);
    const hours = Math.floor(time / 3600000);
    const minutes = Math.floor((time - hours * 3600000) / 60000);
    const seconds = Math.floor(
        (time - hours * 3600000 - minutes * 60000) / 1000
    );
    return `${(hours < 10 ? "0" : "") + hours}:${(minutes < 10 ? "0" : "") + minutes
        }:${(seconds < 10 ? "0" : "") + seconds}`;
}

export function formatEval(evaluation: string, color: string): string {
    if (!evaluation) return "N/A";

    if (evaluation.startsWith("cp")) {
        let cp = Number(evaluation.slice(2));
        if (color === "black") {
            cp = -cp;
        }
        return (cp > 0 ? "+" : "-") + (cp / 100).toString();
    } else if (evaluation.startsWith("mate")) {
        let mateIn = Number(evaluation.slice(4));
        if (color === "black") {
            mateIn = -mateIn;
        }
        return (mateIn > 0 ? "+" : "-") + "M" + Math.abs(mateIn);
    } else {
        return evaluation;
    }
}