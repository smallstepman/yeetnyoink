#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SOURCE = ROOT / "site"
DEFAULT_OUTPUT = ROOT / "site-dist"

EXCLUDED_DIRECTORIES = {"remotion", "__pycache__"}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build the static GitHub Pages artifact for the docs site.")
    parser.add_argument("--source-dir", default=str(DEFAULT_SOURCE), help="Source directory containing the site files.")
    parser.add_argument("--out-dir", default=str(DEFAULT_OUTPUT), help="Output directory for the built artifact.")
    parser.add_argument(
        "--require-rendered-videos",
        action="store_true",
        help="Fail if the generated Remotion MP4 assets are missing.",
    )
    return parser.parse_args()


def load_manifest() -> dict[str, list[str]]:
    manifest_script = ROOT / "scripts" / "docs-manifest.mjs"
    try:
        completed = subprocess.run(
            ["node", str(manifest_script)],
            check=True,
            capture_output=True,
            text=True,
            cwd=ROOT,
        )
    except FileNotFoundError as exc:
        raise SystemExit("Node.js is required to resolve the docs manifest.") from exc
    except subprocess.CalledProcessError as exc:
        raise SystemExit(
            "Failed to load docs manifest:\n"
            + (exc.stderr or exc.stdout or str(exc)).strip()
        ) from exc

    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise SystemExit("Docs manifest did not return valid JSON.") from exc


def ensure_files_exist(source_dir: Path, required_files: list[str]) -> None:
    missing = [relative_path for relative_path in required_files if not (source_dir / relative_path).exists()]
    if missing:
        raise SystemExit(
            "Missing required docs files:\n" + "\n".join(f"- {relative_path}" for relative_path in missing)
        )


def ignore_entries(_directory: str, entries: list[str]) -> list[str]:
    return [entry for entry in entries if entry in EXCLUDED_DIRECTORIES]


def copy_site(source_dir: Path, output_dir: Path) -> None:
    if output_dir.exists():
        shutil.rmtree(output_dir)
    shutil.copytree(source_dir, output_dir, ignore=ignore_entries)
    (output_dir / ".nojekyll").write_text("", encoding="utf-8")


def main() -> None:
    args = parse_args()
    source_dir = Path(args.source_dir).resolve()
    output_dir = Path(args.out_dir).resolve()
    manifest = load_manifest()

    if not source_dir.exists():
        raise SystemExit(f"Docs source directory does not exist: {source_dir}")

    ensure_files_exist(source_dir, manifest["pages"] + manifest["staticAssets"])
    if args.require_rendered_videos:
        ensure_files_exist(source_dir, manifest["videoAssets"])

    copy_site(source_dir, output_dir)
    print(f"Built docs artifact at {output_dir}")


if __name__ == "__main__":
    main()
