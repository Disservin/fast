import type { MoveStr } from "@/ts/UciParsing";

export function extractMove(move: string): MoveStr {
  let promotion = null;

  const origin = move.substring(0, 2);
  const destination = move.substring(2, 4);

  if (move.length == 5) {
    promotion = move.substring(4, 5);
  }

  return {
    orig: origin,
    dest: destination,
    prom: promotion,
  };
}

export function extractScore(
  score: string | undefined,
  sideToMove: string
): number {
  if (score === undefined) {
    return 0;
  }

  if (score.startsWith("cp")) {
    let cp = Number(score.slice(2));
    if (sideToMove === "black") {
      cp = -cp;
    }
    return cp;
  } else if (score.startsWith("mate")) {
    let mateIn = Number(score.slice(4));

    if (sideToMove === "black") {
      mateIn = -mateIn;
    }
    return mateIn > 0 ? 500 : -500;
  } else {
    return 0;
  }
}
