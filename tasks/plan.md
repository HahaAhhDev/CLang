# Implementation Plan: Stack 2 extended stdlib

## Overview
Add everyday stdlib so scripts need no imports: time, math extras, strings, env, cli args, process exec, seeded random, type checks.

## Architecture Decisions
- No new deps, std only, keeps secular clean
- Seeded LCG for deterministic random in tests
- exec via sh -c with output capture, errors are clear ClangError

## Task List
### Phase 1: strings and math
- [x] Task 1: split join trim replace starts_with ends_with
- [x] Task 2: abs floor ceil sqrt pow min max
### Checkpoint: strings plus math
- [x] cargo test passes

### Phase 2: system
- [x] Task 3: time_now sleep_ms env cli_args exec
- [x] Task 4: rand_seed rand_int is_* keys type_of
### Checkpoint: system
- [x] cargo test plus examples plus secular green

## Risks
| Risk | Impact | Mitigation |
| exec abuse | Med | capture output, no shell injection helper, docs warn |
| flaky time | Low | tests use ranges, not exact wall clock |

## Open Questions
- None for Stack 2
