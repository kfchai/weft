# Weft

A programming language designed for LLMs — not "syntax models like," but a
language optimized for the **generate → check → repair loop**:

- The entire spec fits in one context window (hard cap: 3,500 tokens). No
  training data needed; the spec *is* the training.
- Signatures are honest: effects are capabilities passed as arguments, so a
  function's type says everything it can do.
- Verification-dense: contracts, exhaustive matches, property tests, and typed
  holes — most hallucinations die in the checker, not in production.
- Every diagnostic cites a numbered spec rule `[W#]`, so error + spec + code
  all sit in the same context.
- Deterministic and pure by default; one file is one program.

**North-star metric:** median iterations for an LLM (given only the in-context
spec) to go from task description to a program passing hidden tests, vs a
Python baseline.

## Quick start

```
cd weftc && cargo build --release      # no dependencies; Rust 1.75+
cd ..
weftc/target/release/weftc test examples/11_invariants.weft
weftc/target/release/weftc run  examples/02_fizzbuzz.weft
weftc/target/release/weftc skeleton store/orderflow.weft
```

To teach the language to a model: paste `SPEC.md` into its context and ask
for a program. That is the entire onboarding — the spec *is* the training.
When a program fails, `weftc repair-context <file>` emits a paste-ready
repair payload (failure JSON + the violated spec rule's text + a marked
source excerpt).

Validation so far (all machine-graded; see `EXIT-TEST.md` and
`bench/RESULTS.md`): 3 model families (Claude, Grok 4.6, GPT-5.6-Luna) wrote
correct Weft from the spec alone — 16/18 tasks first-try, 18/18 within 3
repair rounds; zero fluency penalty vs Python for frontier models up to
recursive-descent-parser difficulty; and a 630-line codebase was correctly
modified by an agent that saw only a 222-line `weftc ctx` slice.

**Where the checker actually helps, measured** (`bench/ESCAPE-RATE.md`): 57
realistic agent mistakes injected into the same module in both Weft and a
`mypy --strict` Python port, classified by where each one died. For
domain-logic bugs the two languages are *identical* — same verdict on all 33,
0 caught by either checker, the same 10 escaping the test suite. The
difference is confined to two categories: effects (4 compile errors in Weft,
4 silent escapes in Python) and shared mutable state (4 mistakes that cannot
be written in Weft at all). The honest summary is that Weft's edge is not its
type system but its lack of ambient authority — which is precisely the failure
mode of an agent editing code it has only partly read.

## Status: Phase 1 complete

Phase 0 passed: 10/10 exit-test tasks written by spec-only models with zero
rule violations (see EXIT-TEST.md). The Phase 1 kernel in `weftc/` (Rust, no
dependencies) is feature-complete:

- **Lexer + parser** — full grammar, spans everywhere.
- **Typechecker** — generics via unification, structural records, capability
  escape analysis [W33] incl. lambda-capture detection, match exhaustiveness
  [W24] with missing-case reporting, `?` Err-type checking [W26], contract
  purity [W29], typed-hole reporting [W27].
- **Evaluator** — strict, deterministic; contracts enforced on every call
  [W28] with argument values in the error; runtime halts cite rules [W38].
- **Test runner** — unit tests plus property tests (100 deterministic cases,
  contract-aware generation, counterexamples reported).
- **Diagnostics** — every error, checker or runtime, is JSON citing a `[W#]`.

**Corpus status: all 20 files parse, check, and pass 91/91 tests** — the ten
Phase 0 model-written programs are now machine-verified, closing the
"hand-checked only" caveat. Next: Phase 2, the agent-loop harness and the
iterations-to-correct benchmark vs Python (the go/no-go gate).

```
weftc parse [--json] <file.weft>...   # syntax only
weftc check [--json] <file.weft>...   # + types, capabilities, exhaustiveness
weftc run   <file.weft>               # execute main
weftc test  [--json] <file.weft>...   # run unit + property tests
weftc repair-context <file.weft>      # first failure -> paste-ready repair payload
weftc skeleton <file.weft>            # the map: signatures + docs, ~10x compressed
weftc graph <file.weft>               # dependency edges (name -> references)
weftc ctx <file.weft> <def>...        # context slice for modifying those defs
weftc splice <base> <patch>... [--write]   # merge patches by definition name
```

The design's first principle is spec rule **[W41]: every failure is
machine-actionable** — any failure carries a rule id, span, and
expected/actual/hint; `repair-context` assembles failure + cited rule text +
source excerpt into one payload for the generate→check→repair loop.

## Play something written in it

```
weftc/target/release/weftc run games/invaders.weft
```

Space Invaders in a pure functional language: `step(state, command)` is one
total function, so the entire game — collisions, formation marching, bomb
drops, win/lose endings — is deterministic and unit-tested (30 tests, no
mocks). The only effectful code is the loop that prints a frame and reads a
key. Controls: `a`/`d` move, `f` fire, enter waits, `q` quits.

```
+-----------+
|...........|
|..W.W.W.W.W|
|..W.W.*.W.W|
|...........|
|.....|.....|
|.....A.....|
+-----------+
score 10   lives <3 <3 <3   left 14
```

| file | what |
|------|------|
| [SPEC.md](SPEC.md) | The whole language, rules `[W1]`–`[W40]` |
| [examples/](examples/) | 10 annotated programs, one feature focus each |
| [EXIT-TEST.md](EXIT-TEST.md) | Protocol + results: fresh models writing Weft from the spec alone |
| [exit-test/](exit-test/) | The 10 model-written programs (double as the kernel's regression corpus) |
| [weftc/](weftc/) | Phase 1 kernel: lexer, parser, diagnostics (typechecker next) |
| [bench/ESCAPE-RATE.md](bench/ESCAPE-RATE.md) | Where agent mistakes die: Weft vs `mypy --strict` Python, 57 injected bugs |

Examples: 01 hello · 02 fizzbuzz · 03 variants+records · 04 generics ·
05 contracts · 06 Result/`?` · 07 property tests · 08 typed holes ·
09 capabilities · 10 a mini interpreter in Weft.

## Roadmap

- **Phase 0** — spec + examples + exit test — **done** (passed across 3 model families)
- **Phase 1** — kernel in Rust: parser, typechecker, interpreter, JSON diagnostics — **done**
- **Phase 2** — agent-loop harness + iteration benchmark vs Python — **pilot done** (zero fluency penalty at all tested difficulty; small-model repair loop closes in 1 round/failure)
- **Phase 3** — contracts, invariants [W42], property-test runner — **done** (folded into 1 & spec v0.2)
- **Phase 4** — compiler-derived index: `skeleton` / `graph` / `ctx` — **done** (exit test passed: unseen 630-line codebase modified from a 35% slice); content-addressed store deferred
- **Phase 5** — `infer` functions (LLM calls as typed primitives) — **done** ([W43]: prompts are typed defs behind a `Model` capability; replies parse as Weft literals and [W42] invariants reject rule-violating model output at the boundary; verified against the real Claude CLI)

Deliberately out of scope: native codegen, package manager, human-ergonomics
work, IDE plugins. The "IDE" is the agent harness.
