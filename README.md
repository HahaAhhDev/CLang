# CLang

Small language that looks dense on screen but reads line by line. One way to store, one way to define, one verb to print.

## Syntax (locked v1)

```
var:name = value
fn::one(r) { return(r) }
fn::add[a, b] { var:o = a + b return(o) }
emit("hi")
emit(f"hi {name}")
emit(r"C:\tmp")
run(boot)
```

Rules:
- Every store starts `var:name = ...`
- One input defines with `fn::name(r)`, several with `fn::name[a, b]`
- Every call uses parens, even multi: `add(2, 3)`
- `return(x)` carries data, `emit(...)` only prints
- `if(b == 0) { ... } else { ... }`, `for(u in items) { ... }`
- Strings support `f"hi {name}"` fill and `r"raw"` backslashes

## Run and compile

```
clng app.clng
clng compile app.clng -o app.clngb
clng compile --native app.clng -o app
clng fmt app.clng
clng repl
chmod +x app
./app
```

`clng file` runs direct. `clng compile` writes obfuscated `app.clngb` plus sh plus bat launchers you can `chmod +x` and run. `clng compile --native` builds a real standalone binary through rustc for the core pure subset, and names blocked IO calls with a fallback hint when the file needs plain compile. The bundle is xor plus base64 with stripped symbols so source is much harder to recover. No bundle is ever fully unrecoverable, this one only raises the bar.

`clng fmt` prints canonical form. `clng repl` keeps one session across lines. Errors show file plus line plus source plus hint.

Run mode follows the approved shortcut: fast path without keeping a build around.

## Builtins (no imports)

Text `cap_first lower upper contains len split join trim replace starts_with ends_with`, lists `push sort empty_list pmap`, maps `mk_user keys type_of is_str is_num is_list is_map`, files `file_read file_write`, data `json_parse json_dump db_put db_get db_list`, system `time_now sleep_ms env cli_args exec rand_seed rand_int`, math `abs floor ceil sqrt pow min max`, net `http_get` real for http plus `http_post` real for http and stub for https, convert `str num`, output `emit`.

Missing pieces are remade in CLang from bytes plus files plus sockets instead of pulling packages.

## Speed

Interpreter is written in Rust. v1 is a tree walk core, fast for scripts. Native codegen lands next by transpiling the same AST to Rust and calling rustc, which keeps the surface unchanged.

## Layout

- `src/` lexer parser runtime compiler
- `examples/` hello calculator contacts vault
- `tools/secular` license check
- `.github/workflows/ci.yml` test plus build plus secular

## Verify

```
cargo test
cargo build
secular
./target/debug/clng examples/hello.clng
./target/debug/clng examples/calculator.clng
```

## Limits v1

Server framework, real TLS http, db drivers, and full stdlib clone are out. v1 owns scripting plus cli plus files plus http shape at good speed.
