#!/usr/bin/env python3
"""Regenerates crates/app/third_party.json, the library list in Settings > Licenses.

Run after changing dependencies:  python3 scripts/third_party.py
A test in crates/app/src/licenses.rs fails when the list and Cargo.lock disagree.
"""
import json
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parent.parent
metadata = json.loads(
    subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--locked"], cwd=root
    )
)
workspace = set(metadata["workspace_members"])
crates = sorted(
    (
        {
            "name": package["name"],
            "version": package["version"],
            "license": package.get("license") or "See its license file",
            "repository": package.get("repository") or package.get("homepage"),
        }
        for package in metadata["packages"]
        if package["id"] not in workspace
    ),
    key=lambda crate: (crate["name"].lower(), crate["version"]),
)
out = root / "crates/app/third_party.json"
out.write_text(json.dumps(crates, indent=1, ensure_ascii=False) + "\n")
print(f"wrote {len(crates)} crates to {out.relative_to(root)}")
