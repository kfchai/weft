# Phase 2 pilot benchmark — iterations to correct

**Metric:** iterations for a fresh model to produce a program passing hidden
tests. Iteration 1 = initial generation; each repair round (error output sent
back, full corrected program returned) adds one. Cap: 4 iterations.

**Arms:** Weft (model sees only SPEC.md, in context) vs Python 3 (native
fluency, stdlib only). Same task wording, same hidden tests, same model.

**Protocol per task/arm:** fresh agent writes the program → driver appends
hidden tests → `weftc test` / `python` → on failure the tool output goes back
to the same agent (context preserved) → repeat.

## Tasks (6 — none overlap the Phase 0 exit-test suite)

| id | signature (Weft) | difficulty |
|----|------------------|------------|
| collatz | `def collatz_steps(n: Int where n >= 1) -> Int` | easy-medium |
| dedupe | `def dedupe(xs: List[Int]) -> List[Int]` | easy |
| luhn | `def luhn(t: Text) -> Bool` | medium |
| roman | `def roman(n: Int where n >= 1 and n <= 3999) -> Text` | medium |
| mode_min | `def mode_min(xs: List[Int] where len(xs) > 0) -> Int` | medium |
| window_sum | `def max_window_sum(xs: List[Int], k: Int where k >= 1) -> Option[Int]` | medium |

Python signatures mirror these; `max_window_sum` returns `int | None`.

Hidden tests live in `hidden/<task>.weft.txt` and `hidden/<task>.py.txt`;
submissions in `subs/`, merged run files in `work/`. Results in RESULTS.md.

Deferred from the full Phase 2 plan: `weftc repair-context` (driver inlines
the error instead), 50-task suite, multiple models per arm.
