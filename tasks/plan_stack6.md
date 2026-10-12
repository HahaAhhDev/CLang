# Implementation Plan: Stack 6 polish plus speed

## Overview
Faster runs plus repo hygiene: bytecode VM for core subset with auto fallback, clng test runner, clng doc stubs, git package fetch.

## Architecture Decisions
- VM covers the pure core subset only, anything else falls back to tree walk silently
- Parity tests prove identical output on every pure example
- Test runner calls test_ functions with no args in fresh scopes
- Doc scans hash comments above fn defines, no AST change
- Packages clone git URLs shallow into ~/.clng/pkgs, file URLs work offline

## Task List
### Phase 1: speed
- [x] Task 1: bytecode compile plus VM with parity on hello calculator
- [x] Task 2: auto select VM with fallback, walk escape hatch
### Checkpoint: speed
- [x] cargo test passes, native still fine

### Phase 2: repo tools
- [x] Task 3: clng test runner over test_ functions
- [x] Task 4: clng doc stubs plus pkg_fetch git urls
### Checkpoint: stack done
- [x] cargo test plus examples plus secular green

## Risks
| Risk | Impact | Mitigation |
| VM drift | High | parity tests on all pure examples, fallback keeps IO exact |
| test runner hangs | Med | each test fresh Interp, no server in test files |
| git missing | Low | clear error naming git |

## Open Questions
- None
