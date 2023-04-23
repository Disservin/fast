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
  const seconds = Math.floor((time - hours * 3600000 - minutes * 60000) / 1000);
  return `${(hours < 10 ? "0" : "") + hours}:${
    (minutes < 10 ? "0" : "") + minutes
  }:${(seconds < 10 ? "0" : "") + seconds}`;
}

export function formatEval(evaluation: string): string {
  if (!evaluation) return "N/A";

  if (evaluation.startsWith("cp")) {
    const cp = Number(evaluation.slice(2));
    return (cp >= 0 ? "+" : "-") + Math.abs(cp / 100).toFixed(2);
  } else if (evaluation.startsWith("mate")) {
    const mateIn = Number(evaluation.slice(4));

    return (mateIn > 0 ? "+" : "-") + "M" + Math.abs(mateIn);
  } else {
    return evaluation;
  }
}

export function formatPv(pv: string[]): string[] {
  // add move numbers
  const pvWithMoveNumbers = [];
  for (let i = 0; i < pv.length; i++) {
    let move = pv[i];
    if (i % 2 === 0) {
      move = (i / 2 + 1).toString() + ". " + move;
    }
    pvWithMoveNumbers.push(move);
  }
  return pvWithMoveNumbers;
}
