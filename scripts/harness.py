"""Shared helpers for the scripts that run rsort next to the system sort
(bench.py, compat.py, fuzz.py)."""

from __future__ import annotations

import argparse
import os
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def clean_env(**set_vars: str) -> dict[str, str]:
    """The current environment without the variables that pick a locale or
    change sort's output (LC_*, LANG, GNUSORT_*), plus `set_vars`."""
    env = {k: v for k, v in os.environ.items() if not k.startswith(("LC_", "LANG", "GNUSORT_"))}
    env.update(set_vars)
    return env


def version(binary: str) -> str:
    """The first line of `binary --version`, or `?`."""
    try:
        out = subprocess.run([binary, "--version"], capture_output=True, text=True).stdout
    except OSError:
        return "?"
    return out.splitlines()[0].strip() if out.strip() else "?"


def table(headers: list[str], rows: list, right: set[int]) -> str:
    """An ASCII table; columns whose index is in `right` are right-aligned."""
    widths = [max(len(h), *(len(row[i]) for row in rows)) for i, h in enumerate(headers)]
    rule = "+" + "+".join("-" * (w + 2) for w in widths) + "+"

    def line(cells):
        return "| " + " | ".join(
            c.rjust(w) if i in right else c.ljust(w) for i, (c, w) in enumerate(zip(cells, widths))
        ) + " |"

    return "\n".join([rule, line(headers), rule.replace("-", "="), *map(line, rows), rule])


def add_tool_args(p: argparse.ArgumentParser, out_dir: Path, show: int) -> None:
    """The options the differential scripts share."""
    p.add_argument("--jobs", type=int, default=os.cpu_count(), help="parallel runs")
    p.add_argument("--sort", default="/usr/bin/sort", help="reference sort")
    p.add_argument("--rsort", default=str(ROOT / "target" / "release" / "rsort"), help="rsort binary")
    p.add_argument("--show", type=int, default=show, help="mismatches to print")
    p.add_argument(
        "--mismatches",
        type=Path,
        default=out_dir / "mismatches.txt",
        help="where to list every mismatch",
    )


# Per worker process: the tools and a scratch directory of its own.
worker: dict = {}


def init_worker(root: str, sort: str, rsort: str) -> None:
    worker.update(dir=Path(tempfile.mkdtemp(prefix="w", dir=root)), sort=sort, rsort=rsort)
