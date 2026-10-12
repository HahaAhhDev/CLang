## Task 1: strings and math builtins
**Description:** Add split join trim replace starts_with ends_with plus abs floor ceil sqrt pow min max.
**Acceptance criteria:**
- [x] split("a,b", ",") gives list, join(list, ",") round trips
- [x] trim replace starts_with ends_with behave on strings
- [x] sqrt pow min max numeric and clear on bad types
**Verification:**
- [ ] Tests pass: cargo test
- [ ] Build succeeds: cargo build
**Dependencies:** None
**Files likely touched:**
- `src/interp.rs`
**Estimated scope:** Small: 1-2 files

## Task 2: system builtins
**Description:** Add time_now sleep_ms env cli_args exec plus rand_seed rand_int is checks keys type_of.
**Acceptance criteria:**
- [x] time_now returns num, env reads var, cli_args returns list
- [x] exec("echo hi") contains hi, seeded rand is deterministic
- [x] is_str is_num is_list is_map keys type_of correct
**Verification:**
- [x] Tests pass: cargo test
- [x] End to end: examples still run, secular ok
**Dependencies:** Task 1
**Files likely touched:**
- `src/interp.rs`
- `src/main.rs`
**Estimated scope:** Small: 1-2 files

## Checkpoint: After Tasks 1-2
- [x] All tests pass
- [x] Application builds without errors
- [x] Core examples work end-to-end
- [x] Review with human before Stack 3
