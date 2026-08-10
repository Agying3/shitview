from __future__ import annotations

import argparse
from pathlib import Path

from peep_hole_pro.plugin import open_peep_hole


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="peep-hole-pro")
    parser.add_argument("root", nargs="?", default=".", help="Project root folder")
    parser.add_argument("--interval", type=float, default=1.0, help="Polling interval in seconds")
    return parser


def run(argv: list[str] | None = None) -> None:
    args = build_parser().parse_args(argv)
    root = Path(args.root).expanduser().resolve()
    open_peep_hole(root=root, polling_interval=args.interval)

