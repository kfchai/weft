# Escape rate: where do an agent's mistakes die?

The question this study exists to answer: **when an AI agent editing a codebase
makes a mistake, what fraction of those mistakes is stopped by the compiler,
what fraction is stopped by the test suite, and what fraction reaches
production?** The first number is the one that bounds autonomy. A mistake that
dies in the checker costs a repair round. A mistake that dies in tests costs a
slower repair round. A mistake that escapes costs a human.

Weft's design bets heavily on widening the first bucket. This measures whether
that bet pays, against a control.

## Setup

**Module under test.** `bench/escape/weft/core.weft` — 664 lines, 47
definitions, 58 tests: catalog, stock, cart, coupons and pricing, an order
state machine, reporting, receipts. It is the first section of `store/orderflow.weft`,
originally written by a spec-only agent, not authored for this study.

**Control arm.** `bench/escape/py/core.py` — a faithful Python port, deliberately
built as strong as Python reasonably gets: `@dataclass(frozen=True)` records,
validating `__post_init__`, every Weft `where` contract translated into an
explicit precondition, complete annotations passing `mypy --strict`, exhaustive
`match` with `typing.assert_never` at every enum site, and the 58 tests ported
one-for-one to pytest with hypothesis for the property tests. This is a
stronger Python than most teams actually run. If Weft only beats sloppy Python,
the result is worthless.

**Faithfulness oracle.** `bench/escape/{weft/probe_main.weft, py/probe.py}` — a
502-line transcript exercising all 47 definitions at their boundaries: both
sides of every threshold, stock exactly equal to demand, empty and singleton
collections, ties, unknown skus at every lookup site, the full 4x4 state
matrix, discounts driven past the subtotal, depleted inventories. The two arms
emit **byte-identical transcripts**, and the harness re-checks this before every
run. The probe was itself validated by mutation, not by reading: one
behaviour-changing edit per definition, 46/46 detected.

**Classification.** Per mutation, per arm: rejected by the static checker
(`weftc check` / `mypy --strict`) = **CHECKER**; passed the checker, failed the
suite (`weftc test` / `pytest`) = **TESTS**; passed both = **ESCAPED**.

**Two guards against measuring nothing.** A mutation that leaves the probe
transcript unchanged is a semantically-equivalent refactor, not a bug; it is
reported **EQUIVALENT** and excluded. And where both arms still compile, their
mutated transcripts must match each other exactly — positive proof the two arms
received *the same bug* rather than two similar-sounding ones. Zero pairs
diverged in the final run.

**Blinding.** Every authoring and translating agent was forbidden from running
`check`, `test`, `mypy`, or `pytest` on a mutation, and from reasoning about
detectability at all. They could verify syntax only. Detection is the dependent
variable; an author who can see it will tune toward it.

## Corpus 1 — domain-logic bugs (33 live)

Two agents, blind to each other, each authored 20 mistakes under the brief
"a logic error a competent agent would plausibly make, expressible in any
language". Deduplicated to 33.

|      | CHECKER | TESTS | ESCAPED |
|------|--------:|------:|--------:|
| Weft |       0 |    23 |      10 |
| Python |     0 |    23 |      10 |

**Not merely the same totals — the same verdict on every single bug, and the
same 10 escapes.** A perfect null.

This result is real but it answers a narrower question than it appears to,
and the reason is a flaw in my brief: *"expressible in any language"* means
*type-correct*, and a type-correct mutation is precisely what no static checker
in any language can see. The corpus could only produce a tie.

What it does establish:

- For business logic, **Weft's type system offers no advantage over
  `mypy --strict`.** Both are blind. Claims to the contrary should stop.
- A 58-test suite over 47 definitions let **30% of realistic bugs through**, in
  both languages.
- The escapes share a signature: thresholds, boundaries, tie-breaks. `>=` where
  `>` was meant at a discount tier; "insufficient stock" firing when stock
  exactly equals demand; shipping computed pre-discount instead of
  post-discount; the last matching coupon winning instead of the first;
  cancelling an already-cancelled order silently succeeding. These are the
  cases test authors do not think to write — in any language.

An incidental finding: the two blind authors produced the **byte-identical
mutation 7 times out of 20**. The space of plausible mistakes in a given
function is small and heavily peaked, which is mild evidence that mutation
corpora like this one generalise better than their arbitrariness suggests.

## Corpus 2 — structural bugs (22 live, 24 authored)

Same protocol, different brief: mistakes made during *maintenance and feature
work*, aimed at the structure a type system can actually see. Six categories,
four bugs each. Both arms receive the same mistake. Where a mistake cannot be
written in one arm at all, that is recorded rather than faked.

|      | CHECKER | TESTS | ESCAPED | unwritable |
|------|--------:|------:|--------:|-----------:|
| Weft |      11 |     8 |       1 |          2 |
| Python |     7 |     9 |       6 |          0 |

By category — and this is the whole result:

| category | Weft | Python |
|---|---|---|
| missing-case | 4 checker | 4 checker |
| **effects** | **4 checker** | **4 escaped** |
| error-paths | 3 tests, 1 escaped | 3 tests, 1 escaped |
| invalid-values | 1 checker, 3 tests | 1 checker, 3 tests |
| type-confusion | 2 checker, 2 tests | 2 checker, 2 tests |
| **aliasing** | **4 unwritable** | 1 test, 1 escaped, 2 latent |

**Four of six categories are exact ties.** Missing cases: Weft's `[W24]`
exhaustiveness and Python's `assert_never` catch all four each. Invalid values:
Weft's `[W42]` invariants and Python's `__post_init__` do the same work.
Type confusion: both checkers. Error paths: both leak one.

The difference is entirely in two rows.

### Effects: 4 caught at compile time vs 4 escaped to production

The mutations: a debug log added inside `take_stock`; wall-clock time read
inside the receipt formatter; a diagnostic `print` left in `tier_discount`;
an audit log emitted from inside `restock`'s mapping closure.

In Python each is a bare `print()` or `time.time()`. Nothing sees them —
`mypy --strict` cannot, and no mainstream Python linter does either. All four
escape into a shipped build.

In Weft all four are compile errors, and instructively they are *four different*
compile errors, because there is no way to sneak an effect in:

- `[W3] unknown name 'io'` — there is no ambient IO to reach for. The name
  simply does not exist in a pure function's scope. (×2)
- `[W19] call expects 2 arguments, got 1` — thread the capability in as a
  parameter instead, and every call site breaks visibly.
- `[W33] lambda captures capability 'io'` — the rule written for exactly this,
  firing on exactly this.

This is the [W2] "honest signatures" claim being cashed. It is worth being
precise about the mechanism rather than overselling it: two of the four catches
are ordinary scope errors. That *is* the design — purity is enforced by there
being nothing in scope to call, not by a clever analysis — but it is a cheap
mechanism, not a deep one.

### Aliasing: unwritable

Four mutations depend on shared mutable state: a `list` default argument
accumulating across calls, a module-level cache keyed on the wrong thing, a
mutable out-parameter, a global registry handed out live. In Python two are
live bugs (one caught by tests, one escaped), and two are *latent* — they
change no behaviour today and become bugs the moment a caller reads the shared
value. The harness reports those as EQUIVALENT, which is correct but flattering
to Python; a landmine is not a clean bill of health.

All four are **unwritable in Weft**, for a reason with no workaround: no
mutation, no mutable containers, no default arguments, no global state. That is
the cheapest possible form of "caught" — the mistake has no expression.

## What this actually says

**Weft's advantage is not the type system. It is the absence of ambient
authority.** Where both languages can express a mistake, a well-typed Python
catches it just as often, and for domain logic neither catches anything. Weft
pulls ahead in exactly the places where its restrictions mean the mistake
cannot be *written*: effects must be threaded visibly, state cannot be shared.

That is a narrower claim than "Weft catches more bugs", and it is the one the
data supports. It is also, for agentic engineering specifically, the claim that
matters most: an agent editing code it has only partially read is exactly the
actor most likely to reach for an ambient capability or a shared mutable, and
those are the two failure modes that survive review because they look like
nothing.

The corollary is equally important: **30% of domain-logic bugs escape a
59-test suite regardless of language, and no type system addresses this.**
If Weft wants that number down, the lever is not the checker. It is making
`where` contracts and `[W42]` invariants cheap and habitual enough that the
threshold and tie-break cases get *stated* rather than tested — the only
mechanism here that turns an untested boundary into a checked one. Every one
of the 10 corpus-1 escapes is expressible as an invariant.

## Limitations

Stated plainly, because several are load-bearing.

1. **The bugs are LLM-authored simulations of LLM mistakes**, not harvested
   from real agent sessions. This is the main external-validity threat. The
   7/20 independent-convergence rate is weak evidence the distribution is real;
   it is not strong evidence.
2. **Corpus 1's brief guaranteed its own null result.** Documented above.
3. **One module, one domain**, ~660 lines. Nothing here speaks to
   concurrency, resource lifetimes, or anything Weft cannot express at all.
4. **Corpus composition is arbitrary.** 33 logic bugs and 24 structural bugs
   is a ratio I chose, so the pooled 55-bug figure is not meaningful and is
   deliberately not reported. Only the per-corpus and per-category numbers are.
5. **The TESTS/ESCAPED split is a property of this suite**, not of either
   language. A different 58 tests moves that boundary.
6. **The Python arm is stronger than typical** and in three places *stricter
   than the Weft source*: `core.weft` uses `_ =>` wildcards where the port
   enumerates variants and reaches `assert_never`. That bias runs against
   Weft, which is the right direction for a control, but it means the
   missing-case tie slightly understates Python's edge at those sites.
7. **One category label is wrong.** `d07` is filed under invalid-values but
   both arms caught it on the resulting arity change, not on the invalid value
   itself. It is a fair tie, mislabelled.

## Reproducing

```
cd bench/escape
python run.py --bugs bugs     # corpus 1: domain logic
python run.py --bugs bugs2    # corpus 2: structural
```

Requires `mypy`, `pytest`, `hypothesis`, and a release build of `weftc`. The
harness verifies both arms agree on the baseline transcript before measuring
anything, and refuses to report numbers if any mutation pair turns out not to
be the same bug in both arms. Per-mutation verdicts, diagnostics, and cited
rules land in `bugs/results.json` and `bugs2/results.json`.
