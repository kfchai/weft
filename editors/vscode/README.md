# Weft for VS Code

Editor support for the [Weft](https://github.com/kfchai/weft) language.

Weft's compiler already emits machine-actionable JSON diagnostics that cite
numbered spec rules `[W#]` ([W41]). This extension is a thin adapter over that:
it runs the real `weftc` checker against your buffer and renders the result as
native editor diagnostics. The compiler stays the single source of truth — the
extension never re-implements analysis.

## Features

- **Syntax highlighting** — keywords, capability types (`Io`/`Fs`/`Rand`/`Clock`/`Model`),
  builtin types, the full standard library, holes `?name`, and `[W#]` rule
  citations inside comments.
- **Live diagnostics** — every error is checked as you type (debounced, before
  save) via `weftc check --json`. Each diagnostic carries its `[W#]` code,
  linked to the relevant spec rule, plus `expected`/`actual`/`hint`.
- **Typed holes** — `?name` placeholders surface inline with their inferred
  type ([W27]).
- **Commands** (Command Palette or the editor title ▷ menu):
  - **Weft: Run File** — `weftc run` in a terminal (interactive `read_line` works).
  - **Weft: Run Tests** — `weftc test`.
  - **Weft: Check File** — force a re-check.
  - **Weft: Repair Context** — first failure → paste-ready repair payload ([W41]).
  - **Weft: Skeleton** — signatures + docs, ~10× compressed.

## Requirements

A `weftc` binary. If you leave `weft.weftcPath` at its default, the extension
looks for a release build at `weftc/target/release/weftc(.exe)` in your
workspace before falling back to `weftc` on your `PATH`. To build one:

```
cd weftc && cargo build --release
```

## Settings

| Setting | Default | Meaning |
|---|---|---|
| `weft.weftcPath` | `weftc` | Path to the `weftc` executable. |
| `weft.checkOnType` | `true` | Re-check on edit (debounced) rather than only on save. |
| `weft.debounceMs` | `300` | Idle time before an on-type check runs. |
| `weft.showHoles` | `true` | Show typed holes as inline hints. |

## Developing

```
cd editors/vscode
npm install
npm run compile        # or: npm run watch
```

Then press **F5** in VS Code to launch an Extension Development Host, and open
any `.weft` file (try the repo's `examples/`).

To package a `.vsix`: `npx @vscode/vsce package`.
