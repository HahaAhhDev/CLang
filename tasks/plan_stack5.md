# Implementation Plan: Stack 5 server plus data

## Overview
Local apps plus data: http_serve with routing handler, https via system TLS, file tables, editor grammar.

## Architecture Decisions
- std only, no TLS crate, https delegates to curl with system certs
- http_serve handles N requests then returns, isolated worker per request
- tables live inside the JSON db file as nested objects
- grammar is TextMate JSON plus language config, validated by parse

## Task List
### Phase 1: data and serve
- [x] Task 1: tbl_put tbl_get tbl_list tbl_del over db file
- [x] Task 2: http_serve port handler max_requests with request map
### Checkpoint: data plus serve
- [x] cargo test passes, local loopback only

### Phase 2: net and editors
- [x] Task 3: https via curl, clear error when curl missing
- [x] Task 4: vscode grammar plus language config plus vim syntax
### Checkpoint: stack done
- [x] cargo test plus examples plus secular green

## Risks
| Risk | Impact | Mitigation |
| port flake | Med | loopback ephemeral ports, one shot servers |
| curl missing | Low | clear error naming curl |
| handler panic | Med | worker errors become 500 bodies |

## Open Questions
- None
