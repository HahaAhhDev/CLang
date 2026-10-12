# Implementation Plan: Stack 7 release hardening

## Overview
Ship binaries plus numbers plus debuggability: release uploads, bench table, trace logs.

## Architecture Decisions
- Release workflow uses gh upload on tags, no outside actions beyond checkout plus toolchain
- Bench warms up once with output, then times quiet runs
- Trace records stmt lines into a log vector, test runner prints tail on failure

## Task List
### Phase 1: bench and trace
- [x] Task 1: quiet mode plus trace log in both engines
- [x] Task 2: clng bench table VM versus walk
### Checkpoint: bench plus trace
- [x] cargo test passes

### Phase 2: release
- [x] Task 3: release workflow on tags plus Linux binary upload to v0.5.0
- [x] Task 4: clng test trace tail on failure plus docs
### Checkpoint: stack done
- [x] cargo test plus examples plus secular green

## Risks
| Risk | Impact | Mitigation |
| noisy bench | Low | quiet after warmup |
| trace bloat | Low | cap log at 5000 lines |
| upload overwrite | Low | clobber flag on re-run |

## Open Questions
- None
