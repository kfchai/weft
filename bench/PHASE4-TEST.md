# Phase 4 exit test — modify unseen code from a ctx slice

**Claim under test:** an agent can correctly modify a codebase it has never
seen, given only SPEC.md and the output of `weftc ctx <file> <targets>` —
with the context staying well under a full-file read. (On the roadmap this was
"touching 3 definitions without loading more than ~15% of the code"; the pilot
codebase is ~450 lines, so the honest target here is "meaningfully less than
the full file" — the percentage claim needs the multi-kiloline store.)

## Protocol

1. The subject codebase (`store/orderflow.weft`) is written by a separate
   builder agent and verified green (`check` + `test` + `run`) before the test.
2. The change task below was fixed BEFORE the codebase existed (this file is
   the record), so the test is not tailored to the implementation.
3. Driver (me) picks the target defs by reading only the `skeleton` output,
   generates `weftc ctx` for them, and hands a fresh agent: SPEC.md + the ctx
   output + the change task. The agent never sees the file, the file path, or
   any other project artifact.
4. The agent returns complete rewritten versions of each def/test it changes,
   plus any new defs/tests — identified by name. Driver splices them into the
   file by name (replace or append), then runs `weftc check` + `weftc test`
   + hidden acceptance tests.
5. Pass: full file green + acceptance tests green, in <= 2 repair rounds
   (repair-context payloads on failure).

## The change task (fixed 2026-08-20, before the codebase existed)

> Add a flat shipping fee of 500 cents to every order total. The fee is
> waived when the post-discount total is 30000 cents or more. The receipt
> must show a "shipping" line with the fee charged (0 when waived). Existing
> behavior otherwise unchanged; update any existing tests your change
> legitimately affects, and add tests for: fee applied below the threshold,
> fee waived at/above it, and the receipt line in both cases.

## Acceptance tests (hidden from the agent; appended at grading time)

Written after the codebase lands (they need real def names) but before the
subject agent runs. Recorded in `hidden/phase4.weft.txt`.

## Metrics

- context lines vs full-file lines (the compression claim)
- iterations to green (the loop claim)
- whether the agent respected "callers — do not break" signatures

## RESULT — 2026-08-20: PASSED (2 rounds, within the cap)

Codebase: store/orderflow.weft, 630 lines, 46 defs, 51 tests, built by an
independent agent and verified green before the test.

- **Context**: 222 lines vs 630 (35% of a full-file read; 94 lines of bodies).
  The ≤15% aspiration needs multi-kiloline codebases for the map to amortize.
- **Round 1**: the agent — which never saw the file — returned a patch that
  spliced cleanly: 57/58 file tests, 6/6 hidden acceptance tests, all caller
  signatures intact, demo run correct. It derived catalog prices from tests
  in the slice, identified the one existing in-slice test its change
  invalidated, and introduced a `shipping_fee` helper making the exact 30000
  boundary unit-testable.
- **The one failure was a tooling bug, not an agent error**: a test asserting
  the target's behavior *through a caller* (`place_order`) was omitted from
  the slice — ctx selected only tests referencing targets directly. The agent
  honored its actual contract perfectly.
- **Round 2**: given the missing test, the agent returned the correct
  one-number fix (3600 → 4100). 58/58 + acceptance 64/64.
- **Tool fixed**: ctx now includes tests referencing targets OR their callers;
  the missing test verifiably appears in the regenerated slice. Corpus
  regression green.

Verdict: modify-unseen-code-from-a-slice works end to end; the exit test's
one failure improved the tool, which is the failure mode you want.

---

# Scaled re-run (store grown to ~2.5k lines by three independent builders)

## The change task (fixed 2026-08-20, before reading any round-B/C bodies)

> Loyal customers return for free: when computing a refund for an Unwanted
> return, the 10% restocking fee is waived entirely if the returning
> customer's loyalty tier is Gold (the refund computation must take whatever
> loyalty input the existing tier machinery needs). Defective returns are
> unchanged. The Refund record's fee field must be 0 in the waived case.
> Update any tests the change legitimately affects; add tests for: Gold
> waives the fee, non-Gold still pays it, Defective unaffected by tier.

Deliberately cross-domain: returns/refunds (built by round-B agent) must now
consume loyalty tiers (built by round-A agent) — two builders who never saw
each other's code. Targets to be picked from `weftc skeleton` output only.

## Results — PASSED in ONE round (2026-08-20)

Store scaled by three independent builder agents (append-only, each gated on
green): 630 → 1290 → 1892 → **2468 lines, 182 defs, 234 tests**.

- **Compression at scale**: ctx slice = **483 lines (19%) total; actual code
  bodies 247 lines (10.0%)**. The bodies stay proportional to the task while
  the signature map grows only linearly (~7:1 vs source). Measurement forced
  one honest refinement: ctx's map no longer lists test names (234 lines of
  pure overhead; relevant tests ship in full anyway).
- **The subject** (fresh agent, spec + slice only, never saw the file) chose
  the strongest available design: tier-aware `*_for` variants with the
  original four functions delegating at Bronze — making it *impossible by
  construction* for any unseen caller or test to break. It wired returns
  (round-B builder) to loyalty tiers (round-A builder) — two authors who
  never saw each other's code — including ledger integration via
  `customer_tier` and an end-to-end place→return→restock test.
- **Grade: 243/243** (all 234 pre-existing tests + 9 new incl. 3 property
  tests) — first splice, zero repair rounds. The small-scale run's one
  failure (tests-via-callers missing from ctx) did not recur: the tool fix
  from that round demonstrably paid off.

Roadmap claim status: "modify ~3 definitions of a multi-kiloline unseen
codebase loading ~15% of the code" — **met** on the bodies-loaded metric
(10.0%) and near-met on total context (19%, map included). The map fraction
shrinks further as codebases grow, since bodies scale with the task.
