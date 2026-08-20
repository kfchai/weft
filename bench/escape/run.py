"""Escape-rate harness.

For every mutation in bugs/*.json, apply it to both arms (Weft and Python),
run each arm's static checker and then its test suite, and classify where the
bug died:

    CHECKER  - the static checker rejected it (cheapest possible catch)
    TESTS    - the checker passed it; the test suite caught it
    ESCAPED  - both passed; the bug is live

A separate probe program is run against each mutated arm to confirm the
mutation actually changes observable behaviour. A mutation that leaves the
probe transcript unchanged is reported as EQUIVALENT and excluded from the
rate, because "nothing caught it" is not meaningful when there is nothing to
catch.

Usage:  python run.py [--only id1,id2] [--jobs N]
"""

from __future__ import annotations

import argparse
import collections
import concurrent.futures
import json
import shutil
import subprocess
import sys
from dataclasses import asdict, dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent
WEFT_SRC = ROOT / "weft" / "core.weft"
PY_DIR = ROOT / "py"
BUGS_DIR = ROOT / "bugs"
WORK = ROOT / "work"
WEFTC = ROOT.parents[1] / "weftc" / "target" / "release" / "weftc.exe"

CHECKER, TESTS, ESCAPED, EQUIVALENT, BROKEN = (
    "CHECKER",
    "TESTS",
    "ESCAPED",
    "EQUIVALENT",
    "BROKEN",
)


def run(cmd: list[str], cwd: Path, timeout: int = 300) -> tuple[int, str]:
    try:
        p = subprocess.run(
            cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout
        )
    except subprocess.TimeoutExpired:
        return 124, "TIMEOUT"
    return p.returncode, (p.stdout or "") + (p.stderr or "")


def run_probe(cmd: list[str], cwd: Path) -> str:
    """Probe transcripts are compared across two different languages, so only
    the part both can agree on counts: the lines successfully printed, plus
    whether the run halted. A Weft contract halt and a Python ValueError are
    the same event; their error text is not comparable, and comparing it would
    flag every invariant violation as a mismatched pair."""
    try:
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=300)
    except subprocess.TimeoutExpired:
        return "<TIMEOUT>"
    out = p.stdout or ""
    return out + ("\n<HALTED>" if p.returncode != 0 else "")


@dataclass
class ArmResult:
    verdict: str
    checker_out: str
    tests_out: str
    probe_changed: bool
    rule: str = ""


@dataclass
class BugResult:
    id: str
    target: str
    area: str
    description: str
    weft: ArmResult
    py: ArmResult
    # Whether both arms provably received the SAME bug. When both still
    # compile, the two probe transcripts must match exactly; if they do not,
    # the mutation pair is invalid and the measurement is meaningless.
    paired: str = ""


def norm(s: str) -> str:
    """Line endings are not part of the experiment. core.weft is CRLF on disk
    and the two bug sets quoted it under different conventions, so everything
    is normalised to LF before matching."""
    return s.replace("\r\n", "\n").replace("\r", "\n")


def apply_once(text: str, old: str, new: str, where: str) -> str:
    text, old, new = norm(text), norm(old), norm(new)
    n = text.count(old)
    if n != 1:
        raise ValueError(f"{where}: snippet occurs {n} times, expected exactly 1")
    return text.replace(old, new)


DEMO_BANNER = "# ------------------------------------------------------------\n# Demo entry point"


def build_weft_probe(core_text: str) -> str:
    """A Weft program is one file, so the probe inlines the module.

    Take the module up to its demo `main`, drop that `main`, and append the
    probe's own `main`. Doing this from the *mutated* text means the probe
    always exercises the mutated code.
    """
    frag = ROOT / "weft" / "probe_main.weft"
    if not frag.exists():
        return ""
    cut = core_text.find(DEMO_BANNER)
    if cut < 0:
        raise ValueError("could not find the demo entry-point banner in core.weft")
    return core_text[:cut] + frag.read_text(encoding="utf-8")


def cited_rule(out: str) -> str:
    """Pull the [W#] rule id out of a weftc JSON diagnostic, if present."""
    for line in out.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(obj, dict) and obj.get("rule"):
            return str(obj["rule"])
    return ""


def eval_weft(work: Path, baseline_probe: str) -> tuple[ArmResult, str | None]:
    src = work / "core.weft"
    rc, check_out = run([str(WEFTC), "check", "--json", str(src)], work)
    if rc != 0:
        return ArmResult(CHECKER, check_out[:4000], "", True, cited_rule(check_out)), None

    probe_changed = True
    pout: str | None = None
    probe = work / "probe.weft"
    if probe.exists():
        pout = run_probe([str(WEFTC), "run", str(probe)], work)
        probe_changed = pout != baseline_probe

    rc, test_out = run([str(WEFTC), "test", "--json", str(src)], work)
    if rc != 0:
        return ArmResult(TESTS, check_out[:500], test_out[:4000], probe_changed), pout
    if not probe_changed:
        return ArmResult(EQUIVALENT, "", test_out[:500], False), pout
    return ArmResult(ESCAPED, "", test_out[:500], True), pout


def eval_py(work: Path, baseline_probe: str) -> tuple[ArmResult, str | None]:
    rc, check_out = run(
        [
            sys.executable,
            "-m",
            "mypy",
            "--strict",
            "--no-incremental",
            "--no-color-output",
            "core.py",
            "test_core.py",
        ],
        work,
    )
    if rc != 0:
        return ArmResult(CHECKER, check_out[:4000], "", True), None

    probe_changed = True
    pout: str | None = None
    if (work / "probe.py").exists():
        pout = run_probe([sys.executable, "probe.py"], work)
        probe_changed = pout != baseline_probe

    rc, test_out = run(
        [sys.executable, "-m", "pytest", "-q", "--no-header", "-p", "no:cacheprovider"],
        work,
    )
    if rc != 0:
        return ArmResult(TESTS, check_out[:500], test_out[:4000], probe_changed), pout
    if not probe_changed:
        return ArmResult(EQUIVALENT, "", test_out[:500], False), pout
    return ArmResult(ESCAPED, "", test_out[:500], True), pout


def stage(bug: dict, baselines: tuple[str, str]) -> BugResult:
    bid = bug["id"]
    d = WORK / bid
    if d.exists():
        shutil.rmtree(d)
    (d / "weft").mkdir(parents=True)
    (d / "py").mkdir(parents=True)

    # A corpus entry may be inexpressible in one arm — that is itself a
    # finding (a mistake the other language makes impossible), not an error.
    weft_na = not bug.get("old")
    py_na = not bug.get("py_old")

    # --- Weft arm -------------------------------------------------------
    wdir = d / "weft"
    text = WEFT_SRC.read_text(encoding="utf-8")
    (wdir / "core.weft").write_text(
        text if weft_na else apply_once(text, bug["old"], bug["new"], f"{bid}/weft"),
        encoding="utf-8",
    )
    probe = build_weft_probe((wdir / "core.weft").read_text(encoding="utf-8"))
    if probe:
        (wdir / "probe.weft").write_text(probe, encoding="utf-8")

    # --- Python arm -----------------------------------------------------
    pdir = d / "py"
    for f in PY_DIR.glob("*.py"):
        shutil.copy(f, pdir / f.name)
    core = (pdir / "core.py").read_text(encoding="utf-8")
    if not py_na:
        (pdir / "core.py").write_text(
            apply_once(core, bug["py_old"], bug["py_new"], f"{bid}/py"),
            encoding="utf-8",
        )

    NA = ArmResult("N/A", bug.get("py_untranslatable", "") or
                   bug.get("weft_untranslatable", ""), "", False)
    wres, wprobe_out = (NA, None) if weft_na else eval_weft(wdir, baselines[0])
    pres, pprobe_out = (NA, None) if py_na else eval_py(pdir, baselines[1])

    if weft_na or py_na:
        paired = "one-arm-only"
    elif wprobe_out is None or pprobe_out is None:
        # One arm's checker rejected the mutation, so it never ran. The two
        # arms cannot be compared by transcript; flag for manual review.
        paired = "uncomparable(" + ("weft" if wprobe_out is None else "py") + "-rejected)"
    else:
        wl = [l for l in wprobe_out.splitlines() if l.strip()]
        pl = [l for l in pprobe_out.splitlines() if l.strip()]
        paired = "identical" if wl == pl else "DIFFER"
        if paired == "DIFFER":
            (d / "probe-diff.txt").write_text(
                "\n".join(
                    f"line {i}\n  weft: {a}\n  py  : {b}"
                    for i, (a, b) in enumerate(zip(wl, pl))
                    if a != b
                )
                or f"length differs: weft {len(wl)} lines, py {len(pl)}",
                encoding="utf-8",
            )

    return BugResult(
        id=bid,
        target=bug["target"],
        area=bug["area"],
        description=bug["description"],
        weft=wres,
        py=pres,
        paired=paired,
    )


def baseline() -> tuple[str, str]:
    """Run both probes unmutated; their transcripts are the comparison point."""
    w = ""
    probe = build_weft_probe(WEFT_SRC.read_text(encoding="utf-8"))
    if probe:
        WORK.mkdir(parents=True, exist_ok=True)
        gen = WORK / "_baseline_probe.weft"
        gen.write_text(probe, encoding="utf-8")
        w = run_probe([str(WEFTC), "run", str(gen)], ROOT / "weft")
        if "<HALTED>" in w:
            sys.exit(f"baseline weft probe halted:\n{w[-2000:]}")
    p = ""
    if (PY_DIR / "probe.py").exists():
        p = run_probe([sys.executable, "probe.py"], PY_DIR)
        if "<HALTED>" in p:
            sys.exit(f"baseline python probe halted:\n{p[-2000:]}")
    if w and p:
        wl = [l for l in w.splitlines() if l.strip()]
        pl = [l for l in p.splitlines() if l.strip()]
        if wl != pl:
            n = sum(1 for a, b in zip(wl, pl) if a != b) + abs(len(wl) - len(pl))
            sys.exit(
                f"the two arms disagree on {n} baseline probe lines — "
                "the port is not faithful; fix before measuring.\n"
                + "\n".join(
                    f"  weft: {a}\n  py  : {b}"
                    for a, b in list(zip(wl, pl))
                    if a != b
                )[:3000]
            )
    return w, p


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--only", default="")
    ap.add_argument("--jobs", type=int, default=6)
    ap.add_argument("--bugs", default="bugs", help="corpus directory under bench/escape")
    args = ap.parse_args()

    global BUGS_DIR, WORK
    BUGS_DIR = ROOT / args.bugs
    WORK = ROOT / "work" / args.bugs

    bugs: list[dict] = []
    for f in sorted(BUGS_DIR.glob("*.json")):
        if f.name == "results.json":
            continue
        bugs.extend(json.loads(f.read_text(encoding="utf-8")))
    # The two bug sets were authored blind to each other and converged on some
    # identical mutations. Keep one of each; record the twin so the convergence
    # is visible in the results rather than silently double-counted.
    seen: dict[tuple[str, str], dict] = {}
    deduped: list[dict] = []
    for b in bugs:
        # Identity of a bug is the mutation itself. Key on the Weft arm when
        # there is one — two authors who wrote the same Weft edit wrote the
        # same bug, even if their Python translations of it differ in shape.
        # Fall back to the Python arm only for Weft-untranslatable entries,
        # which all share an empty Weft snippet and would otherwise collapse
        # into a single row.
        k = (
            (norm(b["old"]), norm(b["new"]))
            if b.get("old")
            else ("py-only", norm(b.get("py_old", "")), norm(b.get("py_new", "")))
        )
        if k in seen:
            seen[k].setdefault("also_found_as", []).append(b["id"])
            continue
        seen[k] = b
        deduped.append(b)
    dropped = len(bugs) - len(deduped)
    bugs = deduped

    if args.only:
        keep = set(args.only.split(","))
        bugs = [b for b in bugs if b["id"] in keep]

    missing = [
        b["id"]
        for b in bugs
        if not b.get("py_old") and not b.get("py_untranslatable")
    ]
    if missing:
        sys.exit(f"bugs missing python mutation: {', '.join(missing)}")

    print("baseline: verifying the two arms agree ...", flush=True)
    base = baseline()
    print(
        f"running {len(bugs)} mutations x 2 arms "
        f"({dropped} dropped as independently-duplicated)\n",
        flush=True,
    )

    WORK.mkdir(parents=True, exist_ok=True)
    results: list[BugResult] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as ex:
        futs = {ex.submit(stage, b, base): b for b in bugs}
        for fut in concurrent.futures.as_completed(futs):
            b = futs[fut]
            try:
                r = fut.result()
            except Exception as e:  # noqa: BLE001
                r = BugResult(
                    b["id"],
                    b.get("target", "?"),
                    b.get("area", "?"),
                    b.get("description", ""),
                    ArmResult(BROKEN, str(e), "", False),
                    ArmResult(BROKEN, str(e), "", False),
                )
            results.append(r)
            flag = "" if r.paired in ("identical", "") else f"   [{r.paired}]"
            print(
                f"  {r.id:<5} {r.target:<24} weft={r.weft.verdict:<10} "
                f"py={r.py.verdict:<10}{flag}",
                flush=True,
            )

    results.sort(key=lambda r: r.id)
    (BUGS_DIR / "results.json").write_text(
        json.dumps([asdict(r) for r in results], indent=2), encoding="utf-8"
    )

    live = [r for r in results if r.weft.verdict != EQUIVALENT and r.py.verdict != EQUIVALENT]
    print(f"\n{'':<12}{'CHECKER':>9}{'TESTS':>8}{'ESCAPED':>9}{'N/A':>6}")
    for arm in ("weft", "py"):
        row = [getattr(r, arm).verdict for r in live]
        print(
            f"  {arm:<10}{row.count(CHECKER):>9}{row.count(TESTS):>8}"
            f"{row.count(ESCAPED):>9}{row.count('N/A'):>6}"
        )
    rules = collections.Counter(
        r.weft.rule for r in live if r.weft.verdict == CHECKER and r.weft.rule
    )
    if rules:
        print("\n  weft rules that did the catching: "
              + ", ".join(f"{k} x{v}" for k, v in rules.most_common()))
    print(f"\n  n = {len(live)} live mutations "
          f"({len(results) - len(live)} equivalent, excluded)")

    differ = [r.id for r in results if r.paired == "DIFFER"]
    if differ:
        print(
            f"\n  !! {len(differ)} mutation pairs are NOT the same bug in both "
            f"arms and must be fixed before these numbers mean anything: "
            + ", ".join(differ)
        )
    unc = [r for r in results if r.paired.startswith("uncomparable")]
    if unc:
        print(f"  {len(unc)} pairs uncomparable by probe (one arm rejected it "
              f"at check time) — review by hand: "
              + ", ".join(r.id for r in unc))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
