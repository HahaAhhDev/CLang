# Implementation Plan: Stack 3 system core

## Overview
Make CLang do real systems work with no imports: real http client for http, parallel map for speed, file backed db, Windows launcher.

## Architecture Decisions
- std only, no TLS in v1, https gives a clear error pointing to stub path
- pmap clones funcs per thread, isolated scopes, ordered results, pure functions only
- db is a JSON file holding a map, read modify write per call
- compile writes sh plus bat launchers

## Task List
### Phase 1: net and db
- [x] Task 1: http_get http_post real for http, clear https error
- [x] Task 2: db_put db_get db_list over JSON file
### Checkpoint: net plus db
- [x] cargo test passes with local TCP fake, no outside net

### Phase 2: parallel and package
- [x] Task 3: pmap list plus fn name, ordered, errors carry line
- [x] Task 4: compile writes bat launcher, docs update
### Checkpoint: system core
- [x] cargo test plus examples plus secular green

## Risks
| Risk | Impact | Mitigation |
| outside net in tests | High | local 127.0.0.1 TcpListener only |
| thread nondeterminism | Med | pmap keeps input order, pure fns in tests |
| https confusion | Med | clear error, contacts demo switches to http local |

## Open Questions
- None, TLS server lands in Stack 4
