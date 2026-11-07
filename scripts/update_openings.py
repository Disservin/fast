#!/usr/bin/env python3
"""Refresh the bundled Lichess openings, then test classification and imports."""

import argparse
import pathlib
import re
import subprocess
import tempfile


ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCE = "https://github.com/lichess-org/chess-openings"
DEST = ROOT / "crates/chess_core/data/chess-openings"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ref", default="master", help="Upstream branch, tag, or commit (default: master)")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="fast-openings-") as directory:
        checkout = pathlib.Path(directory)
        subprocess.run(["git", "init", "--quiet", directory], check=True)
        subprocess.run(
            ["git", "-C", directory, "fetch", "--depth=1", SOURCE, args.ref], check=True
        )
        subprocess.run(["git", "-C", directory, "checkout", "--quiet", "--detach", "FETCH_HEAD"], check=True)
        revision = subprocess.check_output(
            ["git", "-C", directory, "rev-parse", "HEAD"], text=True
        ).strip()
        files = {}
        codes = set()
        moves = set()
        count = 0
        for volume in "abcde":
            filename = f"{volume}.tsv"
            files[filename] = (checkout / filename).read_bytes()
            lines = files[filename].decode("utf-8").splitlines()
            if not lines or lines[0] != "eco\tname\tpgn":
                raise ValueError(f"{filename}: unexpected TSV header")
            for line in lines[1:]:
                fields = line.split("\t")
                if len(fields) != 3:
                    raise ValueError(f"{filename}: invalid row: {line}")
                eco, name, pgn = fields
                san = " ".join(token.rstrip("+#") for token in pgn.split() if not token.endswith("."))
                if not re.fullmatch(f"{volume.upper()}[0-9]{{2}}", eco) or not name or not san:
                    raise ValueError(f"{filename}: invalid opening: {line}")
                if san in moves:
                    raise ValueError(f"{filename}: duplicate SAN line: {pgn}")
                moves.add(san)
                codes.add(eco)
                count += 1
        if len(codes) != 500:
            raise ValueError(f"Expected all 500 ECO codes, found {len(codes)}")
        files["COPYING.txt"] = (checkout / "COPYING.txt").read_bytes()
        if not files["COPYING.txt"]:
            raise ValueError("Upstream license is empty")
        readme = (DEST / "README.md").read_text()
        readme = re.sub(r"Revision: `[^`]+`", f"Revision: `{revision}`", readme)
        readme = re.sub(r"all [\d,]+ opening lines", f"all {count:,} opening lines", readme)
        files["README.md"] = readme.encode()
        previous = {name: (DEST / name).read_bytes() for name in files}
        try:
            for name, content in files.items():
                (DEST / name).write_bytes(content)
            subprocess.run(["cargo", "test", "--locked", "-p", "chess_core", "-p", "db"], cwd=ROOT, check=True)
        except BaseException:
            for name, content in previous.items():
                (DEST / name).write_bytes(content)
            raise
        print(f"Updated {count:,} openings (500 ECO codes) at {revision}. Rebuild Fast to bundle them.")


if __name__ == "__main__":
    main()
