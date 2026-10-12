# Spec: CLang v1

## Objective
General scripting language that is simpler to write than Python, looks sharp, runs instant, and compiles to a runnable bundle.

## Scope
- In: var store, fn define, return, emit, if, for, f and r strings, core builtins, run plus compile, clear errors, secular check, CI
- Out: server framework, TLS net, db drivers, package registry, IDE, full stdlib clone

## Users and stories
- As a scripter, I want zero import text plus file plus json so small tools stay one file
- As a builder, I want compile plus chmod plus x plus run so I can ship without source

## Requirements
- [x] Single store form var:name
- [x] Single define for one input, brackets for multi define, parens for all calls
- [x] return carries data, emit only prints
- [x] Clear per line error with reason plus hint
- [x] No imports for core
- [x] Compile writes obfuscated bundle plus launcher
- [x] secular passes, cargo test passes

## Interface sketch
- `clng app.clng`
- `clng compile app.clng -o app.clngb`
- `secular`

## Verification
- [x] cargo test shows 5 passed
- [x] hello calculator contacts vault run
- [x] compiled hello runs and launcher runs
- [x] bad file shows line plus hint

## Open questions
- Native backend choice: transpile to Rust then rustc
- Real net in v2
