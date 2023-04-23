export interface PV {
  depth: string;
  score: string;
  pv: string[];
  wdl: {
    win: string;
    draw: string;
    loss: string;
  };
  active: boolean;
}

export function extractPV(line: string) {
  const pv: PV = {
    depth: "0",
    score: "",
    pv: [],
    wdl: {
      win: "0",
      draw: "0",
      loss: "0",
    },
    active: false,
  };
  const tokens = line.split(" ");
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    if (token === "depth") {
      pv.depth = tokens[i + 1];
    } else if (token === "score") {
      pv.score = tokens[i + 1] + " " + tokens[i + 2];
    } else if (token === "pv") {
      while (++i < tokens.length) {
        pv.pv.push(tokens[i].trim());
      }
    } else if (token === "wdl") {
      pv.wdl.win = tokens[i + 1];
      pv.wdl.loss = tokens[i + 2];
      pv.wdl.draw = tokens[i + 3];
    }
  }
  return pv;
}
