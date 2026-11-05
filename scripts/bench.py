#!/usr/bin/env python3
"""Benchmark rsort against the system sort and print an ASCII report.

Usage: scripts/bench.py [options]      (or `just bench [options]`, which builds first)

Each scenario runs both binaries on the same generated input, alternating
between them, and reports the median wall time, the rsort/sort ratio, peak
memory (max RSS) and whether the two outputs are byte-identical. Inputs are
deterministic and cached in target/bench/data. The report goes to stdout and
progress to stderr, so `just bench > report.txt` keeps just the report.

Exits 1 if any compared output differs or a command fails, else 0.
Needs only python3 (3.9+).
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import math
import os
import platform
import random
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

import harness
from harness import table, version

ROOT = Path(__file__).resolve().parent.parent
DATA_DIR = ROOT / "target" / "bench" / "data"
DATA_VERSION = 2  # bump when a generator changes, to regenerate cached inputs

# ---------------------------------------------------------------- inputs

LETTERS = "abcdefghijklmnopqrstuvwxyz"
ACCENTED = "éèüöøåçñ"
MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
PACKAGES = ["libfoo", "bar", "baz-utils", "qux", "openthing", "zlib", "py-requests"]


def word(r: random.Random) -> str:
    """A word with a realistic mix of case, accents and leading punctuation."""
    w = "".join(r.choices(LETTERS, k=r.randint(2, 10)))
    x = r.random()
    if x < 0.20:
        w = w.capitalize()
    elif x < 0.25:
        w = w.upper()
    elif x < 0.30:
        w = r.choice(ACCENTED) + w[1:]
    elif x < 0.33:
        w = r.choice("_-.'(") + w
    return w


def gen_text(r, n):
    return [" ".join(word(r) for _ in range(r.randint(1, 4))) for _ in range(n)]


def gen_dups(r, n):
    vocab = [word(r) for _ in range(2000)]
    return r.choices(vocab, k=n)


def gen_numbers(r, n):
    return [f"{r.uniform(-1e6, 1e6) / 10 ** r.randint(0, 5):.{r.randint(0, 4)}f}" for _ in range(n)]


def gen_floats(r, n):
    out = []
    for _ in range(n):
        if r.random() < 0.6:
            out.append(f"{r.uniform(-1, 1) * 10 ** r.randint(-20, 20):.{r.randint(1, 6)}e}")
        else:
            out.append(f"{r.uniform(-1e4, 1e4):.3f}")
    return out


def gen_human(r, n):
    out = []
    for _ in range(n):
        if r.random() < 0.8:
            out.append(f"{r.randint(1, 1023)}{r.choice(['', 'K', 'M', 'G', 'T'])}")
        else:
            out.append(f"{r.uniform(0.1, 999):.1f}{r.choice('KMGT')}")
    return out


def gen_versions(r, n):
    suffixes = ["", "", "", "-rc1", "-beta2", "a", "~pre"]
    return [
        f"{r.choice(PACKAGES)}-{r.randint(0, 20)}.{r.randint(0, 30)}.{r.randint(0, 100)}{r.choice(suffixes)}"
        for _ in range(n)
    ]


def gen_months(r, n):
    names = MONTHS + ["jan", "DEC", "xyz"]
    return [f"{r.choice(names)} {r.randint(1, 28):2d} {word(r)}" for _ in range(n)]


def gen_records(r, n):
    names = [word(r) for _ in range(500)]  # repeats exercise the second key
    return [
        f"{r.randint(1, 10**6)}\t{r.choice(names)}\t{r.randint(0, 99999)}\t{word(r)}" for _ in range(n)
    ]


def sorted_text(r, n):
    return sorted(gen_text(r, n))  # code point order = UTF-8 byte order = C locale


DATASETS = {
    "text": (gen_text, 1),
    "dups": (gen_dups, 1),
    "numbers": (gen_numbers, 1),
    "floats": (gen_floats, 1),
    "human": (gen_human, 1),
    "versions": (gen_versions, 1),
    "months": (gen_months, 1),
    "records": (gen_records, 1),
    "sorted": (sorted_text, 1),
    "merge-a": (sorted_text, 2),  # each merge input is half the lines
    "merge-b": (sorted_text, 2),
}


def dataset(name: str, lines: int) -> Path:
    path = DATA_DIR / f"{name}-{lines}-v{DATA_VERSION}.txt"
    if not path.exists():
        gen, divisor = DATASETS[name]
        progress(f"generating {path.name}")
        r = random.Random(f"{name}-{DATA_VERSION}")
        body = "\n".join(gen(r, lines // divisor)) + "\n"
        DATA_DIR.mkdir(parents=True, exist_ok=True)
        tmp = path.with_suffix(".tmp")
        tmp.write_bytes(body.encode())
        tmp.replace(path)
    return path


def random_source() -> Path:
    path = DATA_DIR / f"random-source-v{DATA_VERSION}.bin"
    if not path.exists():
        DATA_DIR.mkdir(parents=True, exist_ok=True)
        # macOS sort rejects seeds over 32 bytes ("random seed is too large").
        path.write_bytes(random.Random("random-source").randbytes(32))
    return path


# ---------------------------------------------------------------- scenarios


@dataclass
class Scenario:
    name: str
    args: list
    inputs: list
    utf8: bool = False
    compare: bool = True  # False when outputs legitimately differ (-R)


KEYS = ["-t", "\t", "-k2,2", "-k3,3n"]
SCENARIOS = [
    Scenario("text", [], ["text"]),
    Scenario("text", [], ["text"], utf8=True),
    Scenario("reverse", ["-r"], ["text"]),
    Scenario("fold case", ["-f"], ["text"]),
    Scenario("fold case", ["-f"], ["text"], utf8=True),
    Scenario("dictionary", ["-d"], ["text"]),
    Scenario("unique", ["-u"], ["dups"]),
    Scenario("numeric", ["-n"], ["numbers"]),
    Scenario("general numeric", ["-g"], ["floats"]),
    Scenario("human numeric", ["-h"], ["human"]),
    Scenario("version", ["-V"], ["versions"]),
    Scenario("month", ["-M"], ["months"]),
    Scenario("keys", KEYS, ["records"]),
    Scenario("keys", KEYS, ["records"], utf8=True),
    Scenario("check sorted", ["-c"], ["sorted"]),
    Scenario("merge", ["-m"], ["merge-a", "merge-b"]),
    Scenario("random", ["-R", "--random-source", "{random_source}"], ["text"], compare=False),
]


def shown_args(args: list) -> str:
    def show(a):
        if a == "\t":
            return "'\\t'"
        if a == "{random_source}":
            return "FILE"
        return a

    return " ".join(show(a) for a in args) or "(none)"


# ---------------------------------------------------------------- running


@dataclass
class Sample:
    seconds: float
    rss: int  # bytes
    code: int
    digest: str
    stderr: str


@dataclass
class Result:
    scenario: Scenario
    locale: str
    samples: dict = field(default_factory=dict)  # tool -> [Sample]


def run_once(cmd: list, env: dict, scratch: Path, digest: bool) -> Sample:
    """Run `cmd` once; `digest` hashes its output (only the first timed run's is used)."""
    out_path, err_path = scratch / "stdout", scratch / "stderr"
    with open(out_path, "wb") as out, open(err_path, "wb") as err:
        start = time.perf_counter()
        proc = subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=out, stderr=err, env=env)
        _, status, usage = os.wait4(proc.pid, 0)
        seconds = time.perf_counter() - start
    proc.returncode = os.waitstatus_to_exitcode(status)
    rss = usage.ru_maxrss * (1 if sys.platform == "darwin" else 1024)
    return Sample(
        seconds=seconds,
        rss=rss,
        code=proc.returncode,
        digest=hashlib.sha256(out_path.read_bytes()).hexdigest() if digest else "",
        stderr=err_path.read_text(errors="replace").strip(),
    )


def bench(sc: Scenario, locale: str, tools: dict, lines: int, runs: int, warmup: int, scratch: Path) -> Result:
    env = harness.clean_env(LC_ALL=locale)
    args = [a.replace("{random_source}", str(random_source())) for a in sc.args]
    inputs = [str(dataset(name, lines)) for name in sc.inputs]
    result = Result(sc, locale, {tool: [] for tool in tools})
    order = list(tools)
    for i in range(warmup + runs):
        for tool in order if i % 2 == 0 else reversed(order):
            sample = run_once([tools[tool], *args, *inputs], env, scratch, digest=i == warmup)
            if i >= warmup:
                result.samples[tool].append(sample)
    return result


def median(samples: list) -> float:
    return statistics.median(s.seconds for s in samples)


# ---------------------------------------------------------------- report


def fmt_time(seconds: float) -> str:
    return f"{seconds * 1000:.0f} ms" if seconds < 1 else f"{seconds:.2f} s"


def fmt_mem(rss: int) -> str:
    return f"{rss / 2**20:.0f} MiB"


def outcome(r: Result) -> str:
    sort, rsort = r.samples["sort"][0], r.samples["rsort"][0]
    if sort.code != 0 or rsort.code != 0:
        return f"EXIT {sort.code}/{rsort.code}"
    if not r.scenario.compare:
        return "not compared"
    return "identical" if sort.digest == rsort.digest else "DIFFERENT"


def sysctl(name: str) -> str:
    try:
        return subprocess.run(["sysctl", "-n", name], capture_output=True, text=True).stdout.strip()
    except OSError:
        return ""


def machine() -> str:
    cpu, mem = platform.processor() or platform.machine(), 0
    if sys.platform == "darwin":
        cpu = sysctl("machdep.cpu.brand_string") or cpu
        mem = int(sysctl("hw.memsize") or 0)
    else:
        try:
            for line in Path("/proc/cpuinfo").read_text().splitlines():
                if line.startswith("model name"):
                    cpu = line.split(":", 1)[1].strip()
                    break
            for line in Path("/proc/meminfo").read_text().splitlines():
                if line.startswith("MemTotal:"):
                    mem = int(line.split()[1]) * 1024
        except OSError:
            pass
    ram = f", {mem / 2**30:.0f} GiB RAM" if mem else ""
    return f"{cpu}, {os.cpu_count()} cores{ram}, {platform.system()} {platform.release()} {platform.machine()}"


def report(results: list, tools: dict, lines: int, runs: int, warmup: int, skipped: list) -> str:
    rows, ratios = [], []
    for r in results:
        s, t = median(r.samples["sort"]), median(r.samples["rsort"])
        ratio = t / s if s > 0 else float("nan")
        ratios.append(ratio)
        rows.append([
            r.scenario.name,
            shown_args(r.scenario.args),
            r.locale,
            fmt_time(s),
            fmt_time(t),
            f"{ratio:.2f}x",
            fmt_mem(max(x.rss for x in r.samples["sort"])),
            fmt_mem(max(x.rss for x in r.samples["rsort"])),
            outcome(r),
        ])
    headers = ["Scenario", "Flags", "Locale", "sort", "rsort", "rsort/sort", "sort mem", "rsort mem", "Output"]
    now = datetime.datetime.now().strftime("%Y-%m-%d %H:%M")
    out = [
        f"rsort benchmark  {now}",
        "",
        f"  machine  {machine()}",
        f"  sort     {tools['sort']}  ({version(tools['sort'])})",
        f"  rsort    {tools['rsort']}  ({version(tools['rsort'])})",
        f"  input    {lines:,} lines per scenario; {runs} timed runs after {warmup} warmup, median wall time",
        "",
        table(headers, rows, right={3, 4, 5, 6, 7}),
        "",
    ]
    valid = [x for x in ratios if x > 0 and math.isfinite(x)]
    if valid:
        geo = math.exp(sum(math.log(x) for x in valid) / len(valid))
        faster = sum(x < 1 for x in valid)
        out.append(f"  geometric mean rsort/sort: {geo:.2f}x  (rsort faster in {faster} of {len(valid)} scenarios)")
    compared = [r for r in results if r.scenario.compare]
    same = sum(outcome(r) == "identical" for r in compared)
    out.append(f"  outputs identical in {same} of {len(compared)} compared scenarios")
    out.append("  A ratio below 1.00x means rsort is faster. mem is the peak resident set size.")
    for name in skipped:
        out.append(f"  skipped: {name}")
    problems = [r for r in results if outcome(r).startswith(("EXIT", "DIFFERENT"))]
    if problems:
        out.append("")
        out.append("Problems:")
        for r in problems:
            label = f"{r.scenario.name} [{r.locale}]: {outcome(r)}"
            out.append(f"  {label}")
            for tool in ("sort", "rsort"):
                if r.samples[tool][0].stderr:
                    out.append(f"    {tool}: {r.samples[tool][0].stderr.splitlines()[0]}")
    return "\n".join(out)


# ---------------------------------------------------------------- main


def progress(msg: str) -> None:
    print(msg, file=sys.stderr, flush=True)


def utf8_locale() -> str | None:
    try:
        names = subprocess.run(["locale", "-a"], capture_output=True, text=True).stdout.split()
    except OSError:
        return None
    available = {n.lower(): n for n in names}
    for want in ("en_US.UTF-8", "en_US.utf8", "C.UTF-8", "C.utf8"):
        if want.lower() in available:
            return available[want.lower()]
    return None


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Benchmark rsort against the system sort and print an ASCII report.",
        epilog="Exit status: 0 when every compared output is identical, 1 otherwise.",
    )
    parser.add_argument("--lines", type=int, default=500_000, help="lines per input (default 500000)")
    parser.add_argument("--runs", type=int, default=5, help="timed runs per binary (default 5)")
    parser.add_argument("--warmup", type=int, default=1, help="untimed runs first (default 1)")
    parser.add_argument("--quick", action="store_true", help="100000 lines, 3 runs: a fast smoke check")
    parser.add_argument("--only", nargs="+", metavar="WORD", help="only scenarios whose name contains a WORD")
    parser.add_argument("--list", action="store_true", help="list the scenarios and exit")
    parser.add_argument("--sort", help="sort binary (default /usr/bin/sort, else sort on PATH)")
    parser.add_argument("--rsort", default=str(ROOT / "target" / "release" / "rsort"), help="rsort binary")
    parser.add_argument("--locale", help="UTF-8 locale for the locale scenarios (default: en_US.UTF-8 if present)")
    opts = parser.parse_args()
    if opts.quick:
        opts.lines, opts.runs = 100_000, 3
    if opts.lines < 2 or opts.runs < 1 or opts.warmup < 0:
        parser.error("--lines must be at least 2, --runs at least 1, --warmup at least 0")

    scenarios = SCENARIOS
    if opts.only:
        scenarios = [s for s in scenarios if any(w.lower() in s.name for w in opts.only)]
    if opts.list:
        for s in scenarios:
            print(f"{s.name:16} {'utf-8' if s.utf8 else 'C':6} {shown_args(s.args)}")
        return 0
    if not scenarios:
        parser.error("no scenario matches --only")

    sort = opts.sort or ("/usr/bin/sort" if Path("/usr/bin/sort").exists() else shutil.which("sort"))
    if not sort:
        parser.error("no sort binary found; pass --sort")
    if not Path(opts.rsort).exists():
        parser.error(f"{opts.rsort} not found; build it with `cargo build --release` (`just bench` does this)")
    tools = {"sort": sort, "rsort": opts.rsort}

    locale = opts.locale or utf8_locale()
    skipped = []
    if locale is None:
        skipped = [f"{s.name} [utf-8]: no UTF-8 locale installed" for s in scenarios if s.utf8]
        scenarios = [s for s in scenarios if not s.utf8]

    results = []
    with tempfile.TemporaryDirectory(prefix="rsort-bench-") as scratch:
        for i, sc in enumerate(scenarios, 1):
            loc = locale if sc.utf8 else "C"
            label = f"[{i}/{len(scenarios)}] {sc.name} ({shown_args(sc.args)}, {loc})"
            progress(f"{label} ...")
            r = bench(sc, loc, tools, opts.lines, opts.runs, opts.warmup, Path(scratch))
            progress(f"    sort {fmt_time(median(r.samples['sort']))}  rsort {fmt_time(median(r.samples['rsort']))}")
            results.append(r)

    print(report(results, tools, opts.lines, opts.runs, opts.warmup, skipped))
    bad = any(outcome(r).startswith(("EXIT", "DIFFERENT")) for r in results)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
