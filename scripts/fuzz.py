#!/usr/bin/env python3
"""Differential fuzzing against the system sort.

Usage: scripts/fuzz.py [options]       (or `just fuzz [options]`, which builds first)

Each target generates random inputs and arguments, runs the system sort and
rsort on them, and compares exit status, stdout, and whether a check
reported disorder:

  keys     random lines of text, numbers, months and versions, with random
           -k/-t keys and ordering flags (-bdfirsu, -n -g -h -M -V, -c)
  numbers  -n, -h and -g on numbers built from the locale's own decimal
           point, thousands separator and minus sign
  blanks   fields and -b with every kind of white space, with and without
           GNUSORT_COMPATIBLE_BLANKS
  merge    -m on files the system sort sorted with the same flags
  options  random, often invalid, option and -k syntax (C locale only)

Every target except options runs in each locale below, chosen to cover
UTF-8 and single-byte locales with unusual collation, number symbols and
month names. Runs are deterministic for a given --seed. Cases that hit a
documented difference (conflicting modes are a usage error in rsort; -R
orders differently) are skipped. Mismatches are printed with a command to
reproduce them and all go to target/fuzz/mismatches.txt. Exits 1 on any
mismatch. Needs only python3 (3.9+), on macOS.
"""

from __future__ import annotations

import argparse
import codecs
import concurrent.futures
import locale as pylocale
import os
import random
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = ROOT / "target" / "fuzz"

LOCALES = [
    "C",
    "en_US.UTF-8",
    "de_DE.UTF-8",
    "fr_FR.UTF-8",
    "ru_RU.UTF-8",
    "sv_FI.UTF-8",
    "fa_IR.UTF-8",
    "ja_JP.UTF-8",
    "en_US.ISO8859-15",
    "ru_RU.KOI8-R",
    "ru_RU.ISO8859-5",
    "zh_HK.Big5HKSCS",
]
CHUNK = 50  # cases per job


MODES = ["", "n", "g", "h", "M", "V"]
TEXT_ATOMS = [
    "a", "b", "B", "Z", "z", "_", "-", ".", "0", "1", "9", "10", "-3", "1.5",
    " ", "  ", "\t", ":", ",", "K", "M", "G", "k", "jan", "Feb", "DEC", "MÄR",
    "mär", "янв.", "ФЕВР.", "мая", "déc.", "DÉC.", "févr.", "e", "1e3", "inf",
    "nan", "x~1", "v1.10", "v1.9", "0x1A", "é", "É", "ü", "å", "Å", "ø", "~", "+",
]
NUMBER_ATOMS = [
    "0", "1", "5", "9", "00", "10", "123", ".", ",", "-", "+", " ", "\t", "K",
    "k", "M", "G", "Y", "e", "E", "e5", "x", "inf", "nan", "0x1", "−", " ",
    " ", "٫", "٬", "\f", "\r", "\v", "a",
]
BLANK_ATOMS = [
    "a", "b", "B", "1", "10", "-2", "x", " ", "\t", "\v", "\f", "\r", " ",
    " ", "　", "jan", "FEB", "1.5", "K",
]
# Bytes from 0x80 that single-byte locales classify and collate.
HIGH_BYTES = [bytes([b]) for b in (0xA0, 0xAE, 0xC0, 0xE9, 0xF3, 0xFE)]
OPTION_FLAGS = [
    "-b", "-d", "-f", "-i", "-r", "-s", "-u", "-n", "-g", "-h", "-M", "-V",
    "-c", "-C", "-z", "-m", "--stable", "--reverse", "--sort=numeric",
    "--sort=month", "--sort=bogus", "-S", "10", "-T", "/tmp", "--parallel=2",
    "--batch-size=4", "--check=quiet", "--check=silent",
    "--check=diagnose-first", "--check=bad", "--", "-", "+1", "-2", "+0.1b",
]


@dataclass
class Ctx:
    """What a generator needs: its random source, locale and a scratch dir."""

    rng: random.Random
    locale: str
    sort: str
    dir: Path
    env: dict[str, str] = field(default_factory=dict)

    @property
    def charset(self) -> str:
        name = self.locale.split(".", 1)[1].split("@")[0] if "." in self.locale else "UTF-8"
        try:
            return codecs.lookup(name).name
        except LookupError:
            return "utf-8"

    def atoms(self, words: list[str]) -> list[bytes]:
        """`words` in the locale's encoding (UTF-8 where it has no such
        character), plus raw high bytes in single-byte locales."""
        out = []
        for w in words:
            try:
                out.append(w.encode(self.charset))
            except UnicodeEncodeError:
                out.append(w.encode())
        return out if self.charset == "utf-8" else out + HIGH_BYTES

    def lines(self, atoms: list[bytes], most: int, longest: int) -> bytes:
        rng = self.rng
        lines = [
            b"".join(rng.choice(atoms) for _ in range(rng.randint(0, longest)))
            for _ in range(rng.randint(0, most))
        ]
        return b"\n".join(lines) + (b"\n" if lines else b"")


@dataclass
class Case:
    args: list[str]
    stdin: bytes = b""


def keyspec(rng: random.Random) -> str:
    f1 = rng.randint(1, 3)
    s = str(f1)
    if rng.random() < 0.3:
        s += f".{rng.randint(1, 3)}"
    s += "".join(rng.sample("bdfir", rng.randint(0, 2)))
    if rng.random() < 0.3:
        s += rng.choice(MODES)
    if rng.random() < 0.6:
        s += f",{rng.randint(f1, 4)}"
        if rng.random() < 0.3:
            s += f".{rng.randint(0, 3)}"
        if rng.random() < 0.2:
            s += "b"
    return s


def gen_keys(cx: Ctx) -> Case:
    rng = cx.rng
    args = [f"-{f}" for f in "bdfirsu" if rng.random() < 0.2]
    mode = rng.choice(MODES)
    if mode and rng.random() < 0.5:
        args.append(f"-{mode}")
    if rng.random() < 0.3:
        args += ["-t", rng.choice([":", ",", " ", "a"])]
    for _ in range(rng.choice([0, 0, 1, 2])):
        args += ["-k", keyspec(rng)]
    if rng.random() < 0.15:
        args.append("-c")
    longest = rng.choice([5, 16])
    return Case(args, cx.lines(cx.atoms(TEXT_ATOMS), 12, longest))


def gen_numbers(cx: Ctx) -> Case:
    rng = cx.rng
    args = [rng.choice(["-n", "-h", "-g", "-rn", "-rh", "-sn", "-sh", "-un", "-uh", "-ug"])]
    r = rng.random()
    if r < 0.15:
        args += ["-t", rng.choice([",", ".", "-", " ", "x"]), "-k2"]
    elif r < 0.25:
        args.append("-k1.2")
    return Case(args, cx.lines(cx.atoms(NUMBER_ATOMS + locale_symbols(cx.locale)), 10, 6))


def gen_blanks(cx: Ctx) -> Case:
    rng = cx.rng
    key = rng.choice(["-k2", "-k2b", "-k1.2b", "-k2,2", "-k3", "-k2n", "-k2bn", "-k1b,1"])
    args = [key, *rng.sample(["-b", "-d", "-s", "-n", "-M", "-g", "-r"], rng.randint(0, 2))]
    if rng.random() < 0.5:
        cx.env["GNUSORT_COMPATIBLE_BLANKS"] = "1"
    return Case(args, cx.lines(cx.atoms(BLANK_ATOMS), 8, 6))


MERGE_FLAGS = [
    [], ["-r"], ["-f"], ["-n"], ["-u"], ["-fu"], ["-k2,2", "-k1,1n"],
    ["-t", ":", "-k2"], ["-V"], ["-h", "-r"], ["-s", "-k1,1"],
]


def gen_merge(cx: Ctx) -> Case:
    rng = cx.rng
    flags = rng.choice(MERGE_FLAGS)
    atoms = cx.atoms(TEXT_ATOMS)
    paths = []
    for i in range(rng.randint(1, 4)):
        raw = cx.lines(atoms, 10, 14)
        sorted_ = subprocess.run([cx.sort, *flags], input=raw, capture_output=True, env=cx.env)
        path = cx.dir / f"in{i}"
        path.write_bytes(sorted_.stdout)
        paths.append(str(path))
    return Case(["-m", *flags, *paths])


def option_keyspec(rng: random.Random) -> str:
    if rng.random() < 0.5:
        return "".join(rng.choice("0123456789..,,bdfgiMhnrVz -+x") for _ in range(rng.randint(0, 7)))
    s = str(rng.randint(0, 3))
    if rng.random() < 0.5:
        s += f".{rng.randint(0, 3)}"
    s += "".join(rng.sample("bdfginrMVh", rng.randint(0, 2)))
    if rng.random() < 0.5:
        s += f",{rng.randint(0, 3)}"
        if rng.random() < 0.5:
            s += f".{rng.randint(0, 3)}"
        s += "".join(rng.sample("bdfginr", rng.randint(0, 1)))
    return s


def gen_options(cx: Ctx) -> Case:
    rng = cx.rng
    args: list[str] = []
    for _ in range(rng.randint(0, 4)):
        r = rng.random()
        if r < 0.45:
            spec = option_keyspec(rng)
            args += ["-k", spec] if rng.random() < 0.6 else [f"-k{spec}"]
        elif r < 0.55:
            args += ["-t", rng.choice([":", " ", "", "ab", "\\t", "\\0"])]
        else:
            args.append(rng.choice(OPTION_FLAGS))
    return Case(args, b"b 2 x\na 10 y\nc 1 z\na 2 y\n\n  d 3\n")


TARGETS = {
    "keys": (gen_keys, LOCALES),
    "numbers": (gen_numbers, LOCALES),
    "blanks": (gen_blanks, LOCALES),
    "merge": (gen_merge, LOCALES),
    "options": (gen_options, ["C"]),
}

_symbols: dict[str, list[str]] = {}


def locale_symbols(name: str) -> list[str]:
    """The locale's decimal point, thousands separator and minus sign."""
    if name not in _symbols:
        try:
            pylocale.setlocale(pylocale.LC_ALL, name)
            conv = pylocale.localeconv()
            _symbols[name] = [
                s for s in (conv["decimal_point"], conv["thousands_sep"], conv["negative_sign"]) if s
            ]
        except (pylocale.Error, UnicodeDecodeError):
            _symbols[name] = []
        finally:
            pylocale.setlocale(pylocale.LC_ALL, "C")
    return _symbols[name]


@dataclass
class Job:
    target: str
    locale: str
    chunk: int
    count: int
    seed: int


@dataclass
class Failure:
    target: str
    locale: str
    env: dict[str, str]
    args: list[str]
    stdin: bytes
    sort: subprocess.CompletedProcess
    rsort: subprocess.CompletedProcess

    def command(self) -> str:
        """A shell command that reproduces the case (with `sort` or `rsort`)."""
        env = " ".join(f"{k}={shlex.quote(v)}" for k, v in self.env.items())
        args = " ".join(shlex.quote(a) for a in self.args)
        feed = f"{printf(self.stdin)} | " if self.stdin else ""
        return f"{feed}env {env} sort {args}"

    def detail(self) -> str:
        def show(r):
            return f"exit {r.returncode} {r.stdout[:120]!r} {r.stderr[:80]!r}"

        return f"sort  {show(self.sort)}\n      rsort {show(self.rsort)}"


def printf(data: bytes) -> str:
    """A printf command writing exactly `data`: other than plain ASCII, every
    byte is an octal escape."""
    plain = (chr(c) if 32 <= c < 127 and chr(c) not in "\\%'" else f"\\{c:03o}" for c in data)
    return f"printf '{''.join(plain)}'"


_worker: dict = {}


def init_worker(root: str, sort: str, rsort: str):
    """Each worker process gets its own scratch directory."""
    d = Path(tempfile.mkdtemp(prefix="w", dir=root))
    _worker.update(dir=d, sort=sort, rsort=rsort)


def environment(rng: random.Random, name: str) -> dict[str, str]:
    """A clean environment selecting `name` through LC_ALL or LANG."""
    env = {k: v for k, v in os.environ.items() if not k.startswith(("LC_", "LANG", "GNUSORT_"))}
    env["LC_ALL" if rng.random() < 0.5 else "LANG"] = name
    return env


def run_job(job: Job) -> tuple[int, list[Failure]]:
    """Run one chunk of cases; returns how many were skipped and the failures."""
    gen = TARGETS[job.target][0]
    sort, rsort, d = _worker["sort"], _worker["rsort"], _worker["dir"]
    rng = random.Random(f"{job.seed}/{job.target}/{job.locale}/{job.chunk}")
    skipped, failures = 0, []
    for _ in range(job.count):
        env = environment(rng, job.locale)
        case = gen(Ctx(rng, job.locale, sort, d, env))
        a = subprocess.run([sort, *case.args], input=case.stdin, capture_output=True, env=env, cwd=d)
        b = subprocess.run([rsort, *case.args], input=case.stdin, capture_output=True, env=env, cwd=d)
        if nondeterministic(case.args) or deliberate(a, b):
            skipped += 1
        elif outcome(a) != outcome(b):
            changed = {k: v for k, v in env.items() if os.environ.get(k) != v}
            failures.append(Failure(job.target, job.locale, changed, case.args, case.stdin, a, b))
    return skipped, failures


def nondeterministic(args: list[str]) -> bool:
    """-R orders differently, -o writes a file, --debug is free-form."""
    return any(
        a in ("-o", "--debug") or (a.startswith("-") and not a.startswith("--") and "R" in a)
        for a in args
    )


def deliberate(a: subprocess.CompletedProcess, b: subprocess.CompletedProcess) -> bool:
    """The documented differences (README: Differences): conflicting modes or
    -c with -m are usage errors (exit 2) where sort exits 1, and -c with
    several files is one where sort checks only the first."""
    if b.returncode != 2:
        return False
    return b"mutually exclusive" in a.stderr or b"can check only one" in b.stderr


def outcome(r: subprocess.CompletedProcess):
    return r.returncode, r.stdout, b"disorder" in r.stderr


def main() -> int:
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    p.add_argument("--target", action="append", choices=TARGETS, help="only this target (repeatable)")
    p.add_argument("--locale", action="append", help="only this locale (repeatable)")
    p.add_argument("-n", "--cases", type=int, default=300, help="cases per target and locale")
    p.add_argument("--seed", type=int, default=1, help="random seed")
    p.add_argument("--jobs", type=int, default=os.cpu_count(), help="parallel runs")
    p.add_argument("--sort", default="/usr/bin/sort", help="reference sort")
    p.add_argument("--rsort", default=str(ROOT / "target" / "release" / "rsort"), help="rsort binary")
    p.add_argument("--show", type=int, default=10, help="mismatches to print")
    p.add_argument(
        "--mismatches", type=Path, default=OUT_DIR / "mismatches.txt", help="where to list every mismatch"
    )
    opts = p.parse_args()
    rsort = str(Path(opts.rsort).resolve())

    jobs = []
    for target in opts.target or TARGETS:
        for name in TARGETS[target][1]:
            if opts.locale and name not in opts.locale:
                continue
            for chunk in range(0, opts.cases, CHUNK):
                jobs.append(Job(target, name, chunk, min(CHUNK, opts.cases - chunk), opts.seed))

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    root = tempfile.mkdtemp(prefix="run-", dir=OUT_DIR)
    totals: dict[tuple[str, str], list[int]] = {}
    failures: list[Failure] = []
    start = time.time()
    with concurrent.futures.ProcessPoolExecutor(
        opts.jobs, initializer=init_worker, initargs=(root, opts.sort, rsort)
    ) as pool:
        for job, (skipped, fs) in zip(jobs, pool.map(run_job, jobs)):
            t = totals.setdefault((job.target, job.locale), [0, 0, 0])
            t[0] += job.count
            t[1] += skipped
            t[2] += len(fs)
            failures.extend(fs)
    shutil.rmtree(root, ignore_errors=True)
    report(opts, totals, failures, time.time() - start)
    return 1 if failures else 0


def report(opts, totals, failures: list[Failure], elapsed: float):
    print("rsort differential fuzzing")
    print(f"  sort   {opts.sort}")
    print(f"  rsort  {opts.rsort}")
    print(f"  seed   {opts.seed}, {opts.cases} cases per row, {elapsed:.0f} s\n")
    rows = [("Target", "Locale", "Cases", "Skipped", "Failed")]
    for (target, name), (n, skipped, bad) in totals.items():
        rows.append((target, name, f"{n:,}", f"{skipped:,}", str(bad)))
    widths = [max(len(r[i]) for r in rows) for i in range(len(rows[0]))]
    line = "+" + "+".join("-" * (w + 2) for w in widths) + "+"
    print(line)
    for i, r in enumerate(rows):
        cells = [f" {c:<{w}} " if j < 2 else f" {c:>{w}} " for j, (c, w) in enumerate(zip(r, widths))]
        print("|" + "|".join(cells) + "|")
        if i == 0:
            print(line.replace("-", "="))
    print(line)
    n = sum(t[0] - t[1] for t in totals.values())
    print(f"\n  {n - len(failures):,} of {n:,} compared cases match sort")
    if failures:
        with open(opts.mismatches, "w") as f:
            for x in failures:
                f.write(f"{x.target}\t{x.command()}\n")
        print(f"  {len(failures):,} mismatches, all listed in {opts.mismatches}:\n")
        for x in failures[: opts.show]:
            print(f"  [{x.target} {x.locale}] {x.command()}\n      {x.detail()}")


if __name__ == "__main__":
    sys.exit(main())
