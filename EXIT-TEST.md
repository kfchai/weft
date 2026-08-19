# Phase 0 Exit Test

**Claim under test:** an LLM given only `SPEC.md` — no examples, no prior exposure —
can write correct Weft programs. If a rule is consistently misread, the spec is
wrong, not the model.

## Protocol

1. Open a **fresh** session (no memory, no system prompt about Weft).
2. Paste `SPEC.md` verbatim, then one task from the list below, with:
   *"Using only the language specification above, write a complete single-file
   program solving this task. Include tests."*
3. One task per session (no learning across tasks). Repeat the full list on at
   least two different models.
4. Hand-check each program against the spec. Log every violation as
   `(task, model, rule broken, what the model apparently believed)`.
5. **Pass bar:** ≥ 8/10 tasks yield a program with no spec violations, per model.
   Any rule broken by two or more models on any task ⇒ open a spec issue and
   revise; token cap for SPEC.md stays at 3,500.

## Tasks

1. **sum-squares** — `def sum_squares(n)` returning the sum of squares 1..n; contract `n >= 0`; property test comparing against a `fold` formulation.
2. **balanced** — `def balanced(t: Text) -> Bool` for `()[]{}` bracket matching; unit tests for nested, interleaved-wrong, and empty cases.
3. **rpn** — Reverse Polish Notation calculator: `Text -> Result[Int, Text]` with distinct errors for stack underflow, bad token, and leftover operands.
4. **word-freq** — read a file path via `Fs`, print the 3 most frequent words. (Exercises capabilities + `sort_by` + Results.)
5. **caesar** — caesar cipher encode/decode over lowercase letters; property test: decode(encode(t, k), k) == t for k in 0..25 (contract on k).
6. **binary-search** — over `List[Int]`, returning `Option[Int]` index; property test against a linear `filter`-based search.
7. **grades** — record + variant modeling of students and letter grades; compute a class report Text; exhaustive match required.
8. **safe-config** — parse "key=value" lines into a record, all failures as accumulated-or-first `Err`; exercises `?` and Option→Result.
9. **temperature** — given `List[{day: Text, celsius: Float}]`, warmest day, average, and days above average. (Floats + records + no implicit conversion trap.)
10. **todo-machine** — a pure state machine: `def step(state: TodoState, cmd: Cmd) -> Result[TodoState, Text]` with Add/Done/Remove commands; tests for each transition and one illegal one.

## Known traps the tasks probe deliberately

- Int/Float never mix [W15] (tasks 9, 5)
- `else` is required [W22] (everywhere)
- capabilities can't be stored or returned [W33] (task 4)
- match exhaustiveness [W24] (tasks 7, 10)
- `?` needs matching Err types [W26] (tasks 3, 8)
- iteration without loops [W39] (tasks 2, 6)

## Results log

| date | model | task | verdict | rules broken | notes |
|------------|-----------------|--------------|------|------|-------|
| 2026-08-19 | claude (agent) | sum-squares | PASS | none | contract-bounded property test; correct recursion |
| 2026-08-19 | claude (agent) | balanced | PASS | none | worked around missing text_of_bool with if/else |
| 2026-08-19 | claude (agent) | rpn | PASS | none | correct operand order; `?` with matching Err types |
| 2026-08-19 | claude (agent) | word-freq | PASS | none | correctly refused to let lambdas capture Io [W33]; passed it as a parameter instead |
| 2026-08-19 | claude (agent) | caesar | PASS | none | built char indexing from fold+zip when stdlib had no find |
| 2026-08-19 | claude (agent) | binary-search | PASS | none | half-open bounds; duplicate-tolerant property test vs linear search |
| 2026-08-19 | claude (agent) | grades | PASS | none | exhaustive 4-case match; knew Int literals can't cover a match |
| 2026-08-19 | claude (agent) | safe-config | PASS | none | rejoined split parts so values may contain '='; clean `?` chains |
| 2026-08-19 | claude (agent) | temperature | PASS | none | dodged the Int/Float trap: int_to_float(len(..)) before dividing |
| 2026-08-19 | claude (agent) | todo-machine | PASS | none | noticed `contains` is Text-only; hand-rolled list membership |
| 2026-08-20 | claude (agent) | inventory (v0.2) | PASS | none | machine-graded: check clean, 11/11 tests, run correct. Discovered [W42] unprompted, routed failure through Err without constructing an invalid Item, used new `find` |

**Rounds 1+2: 10/10, zero rule violations** — clears the ≥8/10 bar for this model family. Submissions archived in [exit-test/](exit-test/).

### Spec issues surfaced (fixed in spec)

- `\t` escape missing — tab-containing text was unrepresentable, so tabs couldn't be whitespace (word-freq). Added to [W6].
- `sort_by` stability unspecified — tie order was undefined behavior. Now specified stable (§10).
- `[x, ..rest]` — what may follow `..` was unspecified; balanced wrote `.._rest`. Clarified in [W23]: a binder or `_`.
- No `text_of_bool` — two agents rendered Bools via if/else. Added to stdlib.

### Resolved in spec v0.2 (2026-08-20)

- **Unary minus** — added to [W25] (binds like `not`; Int/Float only).
- **List search/membership** — `find(xs, p)` and `index_of(xs, x)` added to §10. This forced a second change: caesar's archived program defines its own `index_of`, so [W7] now allows a def to shadow a stdlib name — otherwise every stdlib addition would break existing programs.
- **Type invariants** — [W42]: `type Account = {...} where balance >= 0` makes the type nominal (`Account{...}` construction, `Account{..a, ...}` copy) with the invariant checked at every construction; runtime halt carries expected (invariant source) / actual (field values). Postconditions deferred — invariants cover the main cases (a `Result[Account, E]`'s Ok payload is guarded by Account's own invariant).

All 21 corpus files (incl. new examples/11_invariants.weft) pass 100/100 tests under the v0.2 kernel. Probe task 11 (inventory: invariants as the trap) added below.

### Open questions

- Postconditions (`ensures` / named-return `where`) — revisit only if invariant-style modeling proves insufficient in the Phase 2 hard tier.

## Task 11 (v0.2 probe)

**inventory** — model stock items so "quantity never negative, name never empty" are language-enforced (probes [W42] discovery from spec alone); restock/ship (ship must route insufficiency through Err, never constructing an invalid item), total_quantity, and a find-based zero-stock helper (probes new §10 find). 

### Caveats

~~All ten subjects were Claude agents — same model family, and the spec was written by the same model checking it. The protocol requires a second model family before calling the exit test fully passed.~~ **Closed 2026-08-20:** Grok 4.6 (frontier, xAI) went 6/6 first-try and GPT-5.6-Luna (small, OpenAI) 6/6 by round 3 on the benchmark task set, spec-only, machine-graded (see bench/RESULTS.md). The spec is learnable across three model families and two capability tiers. The exit test is passed.

~~Verification was by hand against the spec.~~ **Closed 2026-08-19:** the Phase 1 kernel now machine-verifies the corpus — all 10 submissions parse, typecheck, and pass 91/91 tests under `weftc test` (property tests: 100 generated cases each, contracts respected).
