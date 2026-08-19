# Weft — Language Specification v0.2

Weft is a small, statically typed, purely functional language designed to be learned entirely from this document inside one LLM context window. Everything the language does is defined here; there is nothing else to know. Rules are numbered `[W#]` so tools and error messages can cite them.

Core guarantees:

- **[W1] Deterministic.** No global state, no mutation, no hidden IO. Same inputs → same outputs.
- **[W2] Honest signatures.** All side effects require a *capability* value passed as an argument. A function's signature tells you everything it can do.
- **[W3] Self-contained.** One file is one complete program. All names used are defined in the file or in the standard library (§10).

## 1. Lexical

- **[W4]** Comments: `#` to end of line.
- **[W5]** Value names are `snake_case`; type and variant names are `CamelCase`.
- **[W6]** Literals: Int `42`, `-7` · Float `3.14` (always with a dot) · Bool `true`, `false` · Text `"hi"` with escapes `\n`, `\t`, `\"`, `\\` · List `[1, 2, 3]` · Unit `unit`.

## 2. Program structure

- **[W7]** A program is a sequence of top-level forms: `type`, `def`, `test`. Order does not matter; every top-level name is visible everywhere in the file. Top-level names must be unique. A def may reuse a standard-library name; the program's definition wins in that file.
- **[W8]** The entry point is `def main(io: Io) -> Int`; the returned Int is the exit code.

## 3. Types

- **[W9]** Builtin types: `Int`, `Float`, `Bool`, `Text`, `Unit`, `List[T]`, `Option[T]` (variants `Some(T)` and `None`), `Result[T, E]` (variants `Ok(T)` and `Err(E)`).
- **[W10]** Function types are written `(A, B) -> C`.
- **[W11]** Records are structural: the type `{name: Text, age: Int}` may be written anywhere a type may. Construct: `{name: "Ada", age: 36}`. Access: `u.name`. Copy-with-changes: `{..u, age: 37}`. `type User = {name: Text, age: Int}` declares an alias for readability; aliases and the record type they name are interchangeable.
- **[W12]** Variants are nominal: `type Shape = Circle(Float) | Rect(Float, Float)`. A variant may carry no payload: `type Grade = Pass | Fail`. Construct: `Circle(2.0)`, `Pass`. A variant value can only be inspected with `match`.
- **[W13]** Generics: type parameters in square brackets — `def map[A, B](xs: List[A], f: (A) -> B) -> List[B]`. Call sites infer type arguments; they are never written explicitly.
- **[W14]** Equality `==` / `!=` is structural and defined on all types except function types and capability types.
- **[W15]** There are no implicit conversions of any kind. Convert explicitly (`int_to_float`, `text_of_int`, …).

## 4. Definitions

- **[W16]** One definition form, two shapes:
  - Function: `def name(p1: T1, p2: T2) -> R = expr`
  - Constant: `def name: T = expr`
- **[W17]** A parameter may carry a contract: `p: T where <Bool expr>`. The contract expression may refer to this and earlier parameters. See §6.
- **[W18]** Every `def` states its full type. Only lambda parameter types may be omitted when inferable.

## 5. Expressions

Everything below the top level is an expression.

- **[W19]** Call: `f(x, y)`.
- **[W20]** Lambda: `(x: Int) => x + 1`. Lambdas capture immutable values from the enclosing scope (but never capabilities — see [W33]).
- **[W21]** Block: `{ let x = f(a); let y = g(x); x + y }` — zero or more `let name = expr;` statements followed by one final expression, which is the block's value. `_` may be used as a let name to discard a value. A `let` may shadow an earlier name.
- **[W22]** Conditional: `if cond then e1 else e2`. The `else` branch is required; both branches must have the same type.
- **[W23]** Match:

  ```weft
  match shape {
    Circle(r) => 3.14 * r * r,
    Rect(w, h) => w * h,
  }
  ```

  Patterns are: a variant with sub-patterns `Circle(r)`, a literal `0` / `"x"` / `true`, a binder `x`, the wildcard `_`, the empty list `[]`, or head/rest `[x, ..rest]` — head positions take any pattern; after `..` only a binder or `_`. Patterns nest: `Some(Ok(n))`, `[Ok(n), .._]`.
- **[W24]** Matches must be exhaustive. The checker reports any missing pattern.
- **[W25]** Operators, precedence high → low: `not` and unary `-` · `* / %` · `+ - ++` · `== != < <= > >=` · `and` · `or`. Arithmetic and unary `-` are defined on Int with Int and Float with Float, never mixed. Int `/` and `%` truncate toward zero. `++` concatenates Text with Text and List with List. `and`/`or` short-circuit. Parentheses group.
- **[W26]** Result propagation: `expr?` where `expr : Result[T, E]` evaluates to the `Ok` payload, or immediately returns the `Err` from the enclosing function — whose return type must be `Result[U, E]` with the same `E`.
- **[W27]** Hole: `?name` is a placeholder that typechecks in any position. The checker reports each hole with its expected type. Evaluating a hole halts the program with a hole error. Holes let a partial program typecheck while it is being written.

## 6. Contracts

- **[W28]** A `where` contract is checked at runtime on every call. A false contract halts with a contract error citing the def and the argument values. Example:

  ```weft
  def divide(a: Int, b: Int where b != 0) -> Int = a / b
  ```

- **[W29]** Contract expressions must be pure Bool expressions (they cannot take capabilities or contain holes).
- **[W42]** A record type may carry an invariant: `type Account = {owner: Text, balance: Int} where balance >= 0`. Such a type is **nominal**, unlike plain aliases [W11]: its values are created only as `Account{owner: "a", balance: 1}` and copied as `Account{..acct, balance: 2}`. The invariant — a pure Bool over the field names — is checked at every construction and copy; a false invariant halts citing this rule. Because values are immutable, a value that exists satisfies its invariant, always.

## 7. Capabilities

- **[W30]** Capability types: `Io` (root), `Fs` (files), `Rand` (randomness), `Clock` (time), `Model` (language-model calls). Capability values cannot be created in Weft; the runtime passes the single `Io` to `main`.
- **[W31]** Child capabilities derive from Io via stdlib: `fs(io)`, `rand(io)`, `clock(io)`, `model(io)`.
- **[W32]** Every effectful stdlib function takes a capability as its first argument (`print(io, t)`, `fs_read(f, path)`). A function that performs an effect must therefore receive a capability through its parameters — making the signature a complete effect statement.
- **[W33]** A capability may be: received as a parameter, passed as an argument, or let-bound in a block. It may **not** be placed in records, variants, lists, Options, or Results, may not be returned from a `def` (the stdlib derivations `fs`, `rand`, `clock` are the sole capability-returning functions), and may not be captured by a lambda. Violations are compile errors.

## 7b. Model calls

- **[W43]** `infer name(m: Model, p: T, ...) -> Result[U, Text] = expr` declares a model-backed function. The body is a pure Text expression (parameters in scope) that evaluates to a prompt. Calling the def sends the prompt to the ambient model and reads the reply as a Weft literal of type `U`, checked structurally — including any type invariants [W42], so a reply violating an invariant is rejected. Every failure (no model configured, unparseable or ill-typed reply) is the `Err` case, never a crash. An infer def must take a `Model` parameter [W2] and cannot have type parameters; the reply is a literal only — it cannot name or call anything in the program.

## 8. Tests

- **[W34]** Unit test: `test "name" = expr` where `expr : Bool`. The runner executes all tests; false or halted means fail.
- **[W35]** Property test: `test "name" (x: Int, s: Text) = expr` — the runner invents many argument values and checks the Bool for each. Parameter types are limited to Int, Float, Bool, Text, and Lists of these. Contracts on the parameters constrain generated values.
- **[W36]** Tests are pure: they cannot take or reach capabilities.

## 9. Evaluation

- **[W37]** Evaluation is strict, left to right; arguments evaluate before the call.
- **[W38]** Runtime halts carry a structured error citing a rule: contract violation [W28], hole reached [W27], Int division/modulo by zero, `list_get` out of range never halts (returns Option).
- **[W39]** Recursion is the only loop. The stdlib covers most iteration (`map`, `filter`, `fold`, `range`).

## 10. Standard library (complete)

Pure — Text:
`text_len(t) -> Int` · `text_of_int(n) -> Text` · `text_of_float(x) -> Text` · `text_of_bool(b) -> Text` · `int_of_text(t) -> Option[Int]` · `split(t, sep) -> List[Text]` · `join(parts, sep) -> Text` · `contains(t, sub) -> Bool` · `chars(t) -> List[Text]` (single-char Texts) · `to_upper(t)` · `to_lower(t)` · `trim(t)`

Pure — List (generic over A, B):
`len(xs) -> Int` · `list_get(xs, i) -> Option[A]` · `append(xs, x) -> List[A]` · `map(xs, f)` · `filter(xs, p)` · `fold(xs, init: B, f: (B, A) -> B) -> B` · `range(lo, hi) -> List[Int]` (lo inclusive, hi exclusive) · `reverse(xs)` · `sort_by(xs, key: (A) -> Int) -> List[A]` (ascending, stable) · `zip(xs, ys) -> List[{fst: A, snd: B}]` · `find(xs, p: (A) -> Bool) -> Option[A]` (first match) · `index_of(xs, x) -> Option[Int]` (first structural match)

Pure — Option/Result/math:
`unwrap_or(o: Option[A], d: A) -> A` · `ok_or(o: Option[A], e: E) -> Result[A, E]` · `abs(n)` · `min(a, b)` · `max(a, b)` · `int_to_float(n)` · `float_to_int(x)` (truncates)

Effectful:
`print(io, t) -> Unit` (appends newline) · `read_line(io) -> Text` · `fs(io) -> Fs` · `fs_read(f, path) -> Result[Text, Text]` · `fs_write(f, path, content) -> Result[Unit, Text]` · `rand(io) -> Rand` · `rand_int(r, lo, hi) -> Int` (inclusive) · `clock(io) -> Clock` · `now_ms(c) -> Int`

## 11. Diagnostics

- **[W40]** All checker and runtime errors are JSON: `{rule, message, span, expected, actual, hint}` where `rule` is a `[W#]` from this document. When you see an error, the cited rule is the ground truth for what went wrong.
- **[W41]** Every failure is machine-actionable. Any failure — parse, check, runtime halt, or test — carries a rule id, a span, and where applicable `expected`/`actual`/`hint`, sufficient to act on without reading anything outside this document plus the program. A failure whose diagnostic does not meet this bar is a toolchain bug.
