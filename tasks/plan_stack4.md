# Implementation Plan: Stack 4 native plus tooling

## Overview
Real native binaries for core subset via rustc, plus fmt, REPL, error snippets, CI matrix.

## Architecture Decisions
- Native supports core pure subset only, advanced IO builtins give a clear fallback error
- Generated Rust embeds a small Value runtime, no outside crates
- fmt reprints from AST in canonical style
- REPL keeps one persistent Interp across lines
- Errors show file line plus caret snippet in main, tests check message content

## Task List
### Phase 1: native
- [x] Task 1: codegen Rust for vars fns emit return if for exprs plus core builtins
- [x] Task 2: clng compile --native invokes rustc, chmod plus x, runs standalone
### Checkpoint: native
- [x] cargo test passes, hello plus calculator native binaries run

### Phase 2: tooling
- [x] Task 3: clng fmt canonical reprint
- [x] Task 4: REPL persistent eval plus snippet errors plus CI matrix
### Checkpoint: tooling
- [x] cargo test plus examples plus secular green

## Risks
| Risk | Impact | Mitigation |
| rustc missing | Med | clear error telling how to install |
| subset drift | Med | native rejects IO builtins with names |
| REPL multiline | Low | brace counting before eval |
