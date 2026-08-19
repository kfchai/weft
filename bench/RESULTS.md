# Phase 2 pilot results

## Cross-model summary — 6 easy-tier tasks, spec-only

| model | family | first-try | complete | median iters | notes |
|-------|--------|-----------|----------|--------------|-------|
| Claude (agent) | Anthropic | 6/6 | 6/6 | 1.0 | baseline |
| Grok 4.6 | xAI | 6/6 | 6/6 | 1.0 | heaviest self-testing (14–32 tests/file) |
| GPT-5.6-Luna | OpenAI (small) | 4/6 | 6/6 | 1.0 (mean 1.5) | repair payloads fixed 3/3 in one round |
| GPT-5.6-Luna (Python control) | OpenAI (small) | 6/6 | 6/6 | 1.0 | fresh session, same tasks |

### Luna control verdict

Luna's Python baseline on these tasks is 1.0 — so the Weft mean of 1.5 is a
**real language-unfamiliarity cost** for a small model, not Luna's error rate.
Precisely characterized: +0.5 mean iterations (2 extra rounds over 6 tasks),
with three qualifiers that shape what it means:

1. Both Weft failures were **surface-level** (trailing `;`, missing match arm)
   — Luna's algorithms were correct in both languages on all six tasks. The
   cost is familiarity, not capability, and is the most fixable kind (v0.3
   targeted hint; possibly one more block example in the spec).
2. Both failures were caught **at the parse/check layer with a rule citation**
   — never as wrong runtime behavior. A logic error in Python surfaces only
   if a test happens to cover it.
3. The repair loop priced the entire cost at **one payload round per failure**
   (3/3). The unfamiliarity tax exists; the loop is the rebate.

Net: for small models, Weft currently costs ~0.5 iterations of syntax
friction and buys checker-caught failure in exchange. Whether that trade wins
depends on tasks where wrong-but-plausible logic survives Python's thinner
net — which the easy tier cannot measure.

Three model families, one language none of them ever trained on, learned from
~2,600 in-context tokens: 16/18 first-try, 18/18 complete within 3 rounds.
The spec-learnability claim is now cross-family at both frontier and small
tiers. Grok submissions in `subs/grok/` (round 1 archived in
`subs/grok/iter1/`), graded 2026-08-20: all six passed every hidden and
self-written test on the first attempt.

## Cross-model: GPT-5.6-Luna via Codex — 2026-08-20 (COMPLETE)

Same 6 easy-tier tasks, same protocol (SPEC.md only, no self-testing),
submissions in `subs/codex/` (round 1 archived in `subs/codex/iter1/`).

| task | iterations | trajectory |
|------|------------|------------|
| collatz | 1 | pass (8/8) |
| dedupe | 1 | pass (8/8) |
| luhn | 2 | [W21] trailing `;` → fixed from payload (9/9) |
| roman | 1 | pass (10/10) |
| mode_min | 3 | [W21] trailing `;` → [W24] missing `[]` arm → fixed (8/8) |
| window_sum | 1 | pass (9/9) |

**6/6 complete; median 1, mean 1.5.** Claude baseline: 6/6 at 1.0.

**Repair-payload efficiency: 3/3** — every `repair-context` payload produced a
correct fix for its cited failure in exactly one round, on a deliberately
small model. This is the first direct evidence for the W41 loop design: rule
citation + rule text + marked excerpt was sufficient every time; no payload
had to be re-sent, no fix regressed a previously passing test.

Cross-family learnability: a second model family (GPT) wrote a zero-training-
data language from its in-context spec with a 67% first-try rate and 100%
completion by round 3 — partially discharging the "same model family only"
caveat (frontier-tier cross-check still pending: Grok).

**First first-try failures of the whole experiment** — and both are the same
prior-mismatch: the imperative trailing-semicolon habit against Weft's
expression blocks. The checker cited [W21] on both; `repair-context` payloads
went back as round 2.

**Round 2 results:** luhn **FIXED** (9/9) — one repair round from the payload
alone, the loop closing exactly as designed. mode_min: semicolon fixed, which
*unmasked* a second error — non-exhaustive match [W24], only `[x, ..rest]`
handled ("unhandled: lists of length 0" per the hint). Round 3 payload sent
(`codex-round3-mode_min.md`).

Two design findings from this run:
1. **Trailing `;` before `}`** is the dominant small-model syntax trap —
   v0.3 candidate: keep it an error but add a targeted hint ("remove the `;`;
   the block's last expression is its value").
2. **Contract-aware exhaustiveness**: Luna's `[x, ..rest]`-only match is
   semantically justified by the `len(xs) > 0` contract; the checker is
   conservatively right to demand the `[]` arm, but noting the interplay —
   contracts don't inform exhaustiveness — is a real future design question.
3. Layered unmasking (parse error hides check error hides test failure) means
   minimum iterations = number of layers a bug sits behind; that is inherent
   to any compiler loop, but worth remembering when reading iteration counts.

Claude baseline on identical tasks: 6/6 round 1 (below).

Model: claude (agent), one fresh session per task per arm. Cap: 4 iterations.

## Hard tier — 2026-08-20

Edge-case-dense tasks: recursive-descent calculator (precedence, unary minus,
truncating division, 5 error classes), full text justification, CSV parser
with quote escaping, interval booking with chain-merging.

| task | Weft iterations | Python iterations |
|------|-----------------|-------------------|
| calc | 1 | 1 |
| justify | 1 | 1 |
| csv | 1 | 1 |
| booking | 1 | 1 |

All 8 first-try passes against hidden edge suites (11–14 hidden tests each).
Notable: the Weft calc submission is a mutually recursive descent parser
threading parser state through records (no mutation available) — 40/40 tests
including its own 26; csv is a 3-state character machine with accumulator
threading. Parser-writing difficulty produced zero rule violations and zero
logic errors in a spec-only language.

### What the ceiling at 1.0 now means

Two tiers deep, this model family simply does not iterate on ≤150-line tasks
in either language. Consequences:

1. **The weak claim is now strong**: no fluency penalty for spec-only Weft at
   any difficulty tested, up to and including writing parsers. The risk that
   motivated the pilot (Weft needing *more* iterations) is thoroughly rejected.
2. **The strong claim (fewer iterations than Python) is untestable at this
   model strength** — not failed, unmeasurable: median 1.0 cannot go lower.
   The differential lives in the repair loop, and this model doesn't enter
   the loop. The deciding experiment needs either (a) weaker/other model
   families that fail first attempts routinely, or (b) project-scale tasks
   (multi-hundred-line, cross-def refactors) — which is Phase 4 territory
   (the definition store) rather than bigger one-shot puzzles.
3. Until then, Weft's measurable costs/benefits stand as: ~6k tokens/session
   spec overhead (cost) vs machine-checked capabilities/contracts/invariants
   and W41 diagnostics (benefit priced at zero iterations so far).

## Easy tier — 2026-08-19

| task | Weft iterations | Python iterations |
|------|-----------------|-------------------|
| collatz | 1 | 1 |
| dedupe | 1 | 1 |
| luhn | 1 | 1 |
| roman | 1 | 1 |
| mode_min | 1 | 1 |
| window_sum | 1 | 1 |
| **median** | **1.0** | **1.0** |

All hidden tests passed first try in both arms. Weft submissions also passed
their own self-written tests, including property tests (e.g. dedupe's
`dedupe(dedupe(xs)) == dedupe(xs)`, window_sum's None-iff-short law).

## Interpretation

- **The existential risk is retired at this difficulty**: a language with zero
  training data, learned entirely from a ~2.3k-token in-context spec, showed
  **no fluency penalty whatsoever** against the model's best-known language.
  Spec-in-context ≈ training-data fluency was the bet; at this task size it holds.
- **Ceiling effect**: 1.0 vs 1.0 cannot show Weft *beating* Python — these
  tasks are too easy to produce repair iterations at all. The thesis (fewer,
  cheaper repair loops) is only testable where iteration counts exceed 1:
  larger multi-function tasks, stateful logic, tasks with deliberate spec-trap
  interactions. That is what the full Phase 2 suite must target.
- **Token cost of the spec**: Weft agents averaged ~24.3k tokens vs ~18.5k for
  Python — ≈ 6k/session overhead to read and hold the spec. This is the real,
  measurable price of the spec-in-context design; it amortizes across a
  session but recurs for every fresh context.

## Caveats

- Single model family (claude agents); self-graded ecosystem (spec, kernel,
  tasks, and subjects all same family). Cross-model runs required.
- n = 6 tasks, one attempt each; no variance estimate.
- Weft grading runs the agents' own tests alongside hidden ones (can't filter
  by name yet — `weftc test --only <prefix>` would fix); Python grading runs
  hidden asserts only. Asymmetry favors Python; it did not matter here.

## Next for the full benchmark

Harder task tier (multi-function, stateful, parsing-heavy), `weftc test
--only` for symmetric grading, 2+ model families, and per-iteration token
accounting from the driver rather than by hand.
