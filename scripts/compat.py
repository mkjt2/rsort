#!/usr/bin/env python3
"""Differential test against the system sort: a port of macOS sort's own
compatibility suite, sort/testsuite/run.sh in Apple's text_cmds.

Usage: scripts/compat.py [options]     (or `just compat [options]`, which builds first)

Like run.sh, it sorts tests/upstream/testsuite/sample.txt with every
combination of locale, ordering flags, field separator and key position
(-k and the obsolete +POS1 -POS2), compares rsort's output with the system
sort's byte for byte, and checks that `rsort -c` accepts rsort's output. With
--big it also sorts bigsample.txt (155 MB, unpacked into target/compat) once
per locale and flag set.

The matrix is run.sh's, with two changes:
  - run.sh's ru_RU.ISO-8859-5 is not a macOS locale (so it is the C fallback);
    the real ru_RU.ISO8859-5 is added next to it.
  - run.sh sorts bigsample.txt with --parallel 1 to 17. rsort ignores
    --parallel, so each big run uses one of those values instead.

The full sample sweep is about 9 million runs (about half an hour on 10
cores); --quick runs every 50th case. Mismatches are listed with a command
to reproduce them, and all of them go to target/compat/mismatches.txt.
Exits 1 on any mismatch. Needs only python3 (3.9+).
"""

from __future__ import annotations

import argparse
import concurrent.futures
import lzma
import os
import shlex
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "tests" / "upstream" / "testsuite"
OUT_DIR = ROOT / "target" / "compat"

LANGS = [
    "en_US.UTF-8",
    "C",
    "en_US.ISO8859-15",
    "zh_HK.Big5HKSCS",
    "ru_RU.KOI8-R",
    "ru_RU.ISO-8859-5",
    "ru_RU.ISO8859-5",
]
KEYS = ["-srh", "-sfrudb", "-Vs", "-sM", "-siz"]
F1 = range(1, 10)
C1 = [1, 2, 3, 4, 5, 10, 15, 20, 25, 30]
F2 = range(1, 11)
C2 = [0, 1, 2, 3, 4, 5, 10, 15, 20, 25, 30]


def skipped(lang: str, keys: str) -> bool:
    # run.sh: numeric sorting in ru_RU.KOI8-R (space as the thousands
    # separator) deliberately differs from the old sort.
    return lang == "ru_RU.KOI8-R" and keys == "-srh"


def sample_variants(lang: str):
    """The extra arguments run.sh tries for one locale and flag set, in order."""
    numeric = lang != "ru_RU.KOI8-R"
    yield []
    yield ["-t", " "]
    yield ["-t", "|"]
    yield ["-t", "\\0"]
    for f1 in F1:
        for c1 in C1:
            yield [f"+{f1}.{c1}"]
            yield [f"-k{f1}.{c1}"]
            yield [f"-k{f1}.{c1}b"]
            yield ["-t", " ", f"-k{f1}.{c1}"]
            if numeric:
                yield ["-t", " ", f"-k{f1}.{c1}n"]
            yield ["-t", "|", f"-k{f1}.{c1}"]
            for f2 in F2:
                for c2 in C2:
                    yield [f"+{f1}.{c1}", f"-{f2}.{c2}"]
                    yield [f"-k{f1}.{c1},{f2}.{c2}"]
                    yield [f"-k{f1}.{c1}b,{f2}.{c2}"]
                    yield ["-t", " ", f"-k{f1}.{c1},{f2}.{c2}"]
                    if numeric:
                        yield ["-t", " ", f"-k{f1}.{c1}n,{f2}.{c2}"]
                        yield ["-t", "\\0", f"-k{f1}.{c1}n,{f2}.{c2}"]
                    yield ["-t", "|", f"-k{f1}.{c1},{f2}.{c2}"]
                    yield ["-t", "|", f"-k{f1}.{c1},{f2}.{c2}", f"-k{f2}.{c1},{f1}.{c2}"]
                    yield [f"-k{f1}.{c1}b,{f2}.{c2}", f"-k{f2}.{c1},{f1}.{c2}"]


@dataclass
class Case:
    lang: str
    keys: str
    extra: list[str]
    input: Path
    parallel: int | None = None  # --parallel for rsort (big runs)


@dataclass
class Failure:
    case: Case
    what: str  # "output", "status", "check"
    detail: str

    def command(self, rsort: str) -> str:
        c = self.case
        args = [c.keys, *c.extra, str(c.input)]
        tool = "sort" if self.what != "check" else rsort
        if self.what == "check":
            args = ["-c", c.keys, *c.extra, "<rsort output>"]
        cmd = " ".join(shlex.quote(a) for a in args)
        return f"LC_ALL= LANG={c.lang} {tool} {cmd}"


def environment(lang: str) -> dict[str, str]:
    env = {k: v for k, v in os.environ.items() if not k.startswith("LC_")}
    env["LANG"] = lang
    return env


_worker: dict = {}


def init_worker(root: str, sort: str, rsort: str):
    """Each worker process gets its own scratch directory."""
    _worker.update(dir=Path(tempfile.mkdtemp(prefix="w", dir=root)), sort=sort, rsort=rsort)


def run_in_worker(case: Case) -> list[Failure]:
    return run_case(case, _worker["sort"], _worker["rsort"], _worker["dir"])


def run_case(case: Case, sort: str, rsort: str, d: Path) -> list[Failure]:
    env = environment(case.lang)
    want, got = d / "orig", d / "new"
    args = [case.keys, *case.extra, str(case.input)]
    a = subprocess.run([sort, *args, "-o", want], env=env, capture_output=True)
    extra = ["--parallel", str(case.parallel)] if case.parallel else []
    b = subprocess.run([rsort, *extra, *args, "-o", got], env=env, capture_output=True)
    failures = []
    if (a.returncode == 0) != (b.returncode == 0):
        failures.append(
            Failure(
                case,
                "status",
                f"sort exit {a.returncode}, rsort exit {b.returncode}: "
                + (b.stderr or a.stderr).decode(errors="replace").strip(),
            )
        )
        return failures
    if a.returncode != 0:
        return failures  # both reject the arguments
    if want.read_bytes() != got.read_bytes():
        failures.append(Failure(case, "output", first_difference(want, got)))
    c = subprocess.run(
        [rsort, "-c", case.keys, *case.extra, str(got)], env=env, capture_output=True
    )
    if c.returncode != 0:
        failures.append(
            Failure(case, "check", c.stderr.decode(errors="replace").strip())
        )
    return failures


def first_difference(want: Path, got: Path) -> str:
    a = want.read_bytes().split(b"\n")
    b = got.read_bytes().split(b"\n")
    for i, (x, y) in enumerate(zip(a, b)):
        if x != y:
            return f"line {i + 1}: sort {x[:60]!r}, rsort {y[:60]!r}"
    return f"sort has {len(a) - 1} lines, rsort {len(b) - 1}"


def big_input() -> Path:
    path = OUT_DIR / "bigsample.txt"
    if not path.exists():
        print(f"unpacking {path.relative_to(ROOT)} ...", file=sys.stderr)
        OUT_DIR.mkdir(parents=True, exist_ok=True)
        with lzma.open(FIXTURES / "bigsample.txt.xz") as src, open(path, "wb") as dst:
            while chunk := src.read(1 << 20):
                dst.write(chunk)
    return path


def main() -> int:
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    p.add_argument("--quick", action="store_true", help="run every 50th sample case")
    p.add_argument("--big", action="store_true", help="also sort bigsample.txt")
    p.add_argument("--lang", action="append", help="only this locale (repeatable)")
    p.add_argument("--keys", action="append", help="only this flag set (repeatable)")
    p.add_argument("--jobs", type=int, default=os.cpu_count(), help="parallel runs")
    p.add_argument("--sort", default="/usr/bin/sort", help="reference sort")
    p.add_argument(
        "--rsort", default=str(ROOT / "target" / "release" / "rsort"), help="rsort binary"
    )
    p.add_argument("--show", type=int, default=20, help="mismatches to print")
    opts = p.parse_args()

    langs = opts.lang or LANGS
    keysets = opts.keys or KEYS
    sample = FIXTURES / "sample.txt"
    cases: list[Case] = []
    for lang in langs:
        for keys in keysets:
            if skipped(lang, keys):
                continue
            for i, extra in enumerate(sample_variants(lang)):
                if not opts.quick or i % 50 == 0:
                    cases.append(Case(lang, keys, extra, sample))
    big: list[Case] = []
    if opts.big:
        path = big_input()
        n = 0
        for lang in langs:
            for keys in keysets:
                if not skipped(lang, keys):
                    big.append(Case(lang, keys, [], path, parallel=n % 17 + 1))
                    n += 1

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    root = tempfile.mkdtemp(prefix="run-", dir=OUT_DIR)
    init = (root, opts.sort, opts.rsort)

    failures: list[Failure] = []
    totals: dict[tuple[str, str], list[int]] = {}
    start = time.time()

    def record(case: Case, fs: list[Failure]):
        t = totals.setdefault((case.lang, case.keys), [0, 0])
        t[0] += 1
        t[1] += bool(fs)
        failures.extend(fs)

    with concurrent.futures.ProcessPoolExecutor(
        opts.jobs, initializer=init_worker, initargs=init
    ) as pool:
        done = 0
        for case, fs in zip(cases, pool.map(run_in_worker, cases, chunksize=256)):
            record(case, fs)
            done += 1
            if done % 20000 == 0 or done == len(cases):
                rate = done / max(time.time() - start, 1e-9)
                left = (len(cases) - done) / rate
                print(
                    f"  {done:,}/{len(cases):,} cases, {len(failures)} mismatches, "
                    f"~{left / 60:.0f} min left",
                    file=sys.stderr,
                )
    # Big runs one or two at a time: each holds the whole file in memory.
    with concurrent.futures.ProcessPoolExecutor(
        2, initializer=init_worker, initargs=init
    ) as pool:
        for case, fs in zip(big, pool.map(run_in_worker, big)):
            print(f"  big {case.lang} {case.keys}: {'ok' if not fs else 'MISMATCH'}", file=sys.stderr)
            record(Case(case.lang, case.keys + " (big)", [], case.input), fs)

    report(opts, totals, failures, time.time() - start)
    return 1 if failures else 0


def report(opts, totals, failures: list[Failure], elapsed: float):
    sort_version = subprocess.run(
        [opts.sort, "--version"], capture_output=True, text=True
    ).stdout.strip()
    print(f"rsort compatibility suite (port of sort/testsuite/run.sh)")
    print(f"  sort   {opts.sort} ({sort_version})")
    print(f"  rsort  {opts.rsort}")
    print(f"  mode   {'quick (every 50th case)' if opts.quick else 'full'}"
          f"{' + bigsample' if opts.big else ''}, {elapsed / 60:.1f} min\n")
    rows = [("Locale", "Flags", "Cases", "Failed")]
    for (lang, keys), (n, bad) in totals.items():
        rows.append((lang, keys, f"{n:,}", str(bad)))
    widths = [max(len(r[i]) for r in rows) for i in range(4)]
    line = "+" + "+".join("-" * (w + 2) for w in widths) + "+"
    print(line)
    for i, r in enumerate(rows):
        cells = [
            f" {c:<{w}} " if j < 2 else f" {c:>{w}} " for j, (c, w) in enumerate(zip(r, widths))
        ]
        print("|" + "|".join(cells) + "|")
        if i == 0:
            print(line.replace("-", "="))
    print(line)
    n = sum(t[0] for t in totals.values())
    bad = sum(t[1] for t in totals.values())
    print(f"\n  {n - bad:,} of {n:,} cases match sort")
    if failures:
        out = OUT_DIR / "mismatches.txt"
        with open(out, "w") as f:
            for x in failures:
                f.write(f"{x.what}\t{x.command(opts.rsort)}\t{x.detail}\n")
        print(f"  {len(failures):,} mismatches, all listed in {out.relative_to(ROOT)}:\n")
        for x in failures[: opts.show]:
            print(f"  [{x.what}] {x.command(opts.rsort)}\n      {x.detail}")


if __name__ == "__main__":
    sys.exit(main())
