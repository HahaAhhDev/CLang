use crate::error::ClangError;
use crate::parser::{Expr, Parser, Stmt};
use std::collections::{HashMap, HashSet};

pub const NATIVE_UNSUPPORTED: &[&str] = &[
    "file_read",
    "file_write",
    "json_parse",
    "json_dump",
    "http_get",
    "http_post",
    "db_put",
    "db_get",
    "db_list",
    "pmap",
    "exec",
    "env",
    "cli_args",
    "time_now",
    "sleep_ms",
    "rand_seed",
    "rand_int",
    "mk_user",
    "sort",
    "push",
];

const RUNTIME: &str = r#"
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug)]
enum V {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<V>),
    Map(HashMap<String, V>),
}

fn globals() -> &'static Mutex<HashMap<String, V>> {
    static G: OnceLock<Mutex<HashMap<String, V>>> = OnceLock::new();
    G.get_or_init(|| Mutex::new(HashMap::new()))
}

fn g_get(k: &str) -> V {
    globals().lock().unwrap().get(k).cloned().unwrap_or(V::Null)
}

fn g_set(k: &str, v: V) {
    globals().lock().unwrap().insert(k.to_string(), v);
}

fn disp(v: &V) -> String {
    match v {
        V::Null => "null".to_string(),
        V::Bool(b) => b.to_string(),
        V::Num(n) => {
            if n.fract() == 0.0 && n.abs() < 9e15 {
                format!("{}", *n as i64)
            } else {
                format!("{}", n)
            }
        }
        V::Str(s) => s.clone(),
        V::List(items) => {
            let parts: Vec<String> = items
                .iter()
                .map(|x| match x {
                    V::Str(s) => format!("\"{}\"", s),
                    _ => disp(x),
                })
                .collect();
            format!("[{}]", parts.join(", "))
        }
        V::Map(m) => {
            let mut ks: Vec<&String> = m.keys().collect();
            ks.sort();
            let parts: Vec<String> =
                ks.iter().map(|k| format!("{}: {}", k, disp(&m[*k]))).collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn truthy(v: &V) -> bool {
    match v {
        V::Null => false,
        V::Bool(b) => *b,
        V::Num(n) => *n != 0.0,
        V::Str(s) => !s.is_empty(),
        V::List(l) => !l.is_empty(),
        V::Map(m) => !m.is_empty(),
    }
}

fn v_num(v: &V) -> f64 {
    match v {
        V::Num(n) => *n,
        _ => 0.0,
    }
}

fn v_add(a: V, b: V) -> V {
    match (a, b) {
        (V::Num(x), V::Num(y)) => V::Num(x + y),
        (V::Str(x), V::Str(y)) => V::Str(x + &y),
        (V::Str(x), V::Num(y)) => V::Str(format!("{}{}", x, disp(&V::Num(y)))),
        (V::Num(x), V::Str(y)) => V::Str(format!("{}{}", disp(&V::Num(x)), y)),
        (V::List(mut x), V::List(mut y)) => {
            x.append(&mut y);
            V::List(x)
        }
        _ => V::Null,
    }
}

fn v_sub(a: V, b: V) -> V {
    V::Num(v_num(&a) - v_num(&b))
}

fn v_mul(a: V, b: V) -> V {
    match (&a, &b) {
        (V::Num(x), V::Num(y)) => V::Num(x * y),
        (V::Str(s), V::Num(n)) | (V::Num(n), V::Str(s)) => {
            let k = (*n as i64).max(0).min(10000) as usize;
            V::Str(s.repeat(k))
        }
        _ => V::Null,
    }
}

fn v_div(a: V, b: V) -> V {
    let d = v_num(&b);
    if d == 0.0 {
        eprintln!("clang error at line 1: division by zero\nhelp: guard with if(b == 0) before dividing");
        std::process::exit(1);
    }
    V::Num(v_num(&a) / d)
}

fn v_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Num(x), V::Num(y)) => x == y,
        (V::Str(x), V::Str(y)) => x == y,
        _ => false,
    }
}

fn v_lt(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Num(x), V::Num(y)) => x < y,
        (V::Str(x), V::Str(y)) => x < y,
        _ => false,
    }
}

fn v_index(o: &V, i: &V) -> V {
    match (o, i) {
        (V::List(v), V::Num(n)) => {
            let j = *n as i64;
            let pos = if j < 0 { v.len() as i64 + j } else { j } as usize;
            v.get(pos).cloned().unwrap_or(V::Null)
        }
        (V::Str(s), V::Num(n)) => {
            let ch: Vec<char> = s.chars().collect();
            let j = *n as i64;
            let pos = if j < 0 { ch.len() as i64 + j } else { j } as usize;
            ch.get(pos).map(|c| V::Str(c.to_string())).unwrap_or(V::Null)
        }
        _ => V::Null,
    }
}

fn v_slice(o: &V, s: Option<V>, e: Option<V>) -> V {
    let len = match o {
        V::List(v) => v.len() as i64,
        V::Str(x) => x.chars().count() as i64,
        _ => return V::Null,
    };
    let norm = |v: i64| {
        if v < 0 {
            (len + v).max(0).min(len)
        } else {
            v.max(0).min(len)
        }
    };
    let a = s.map(|v| norm(v_num(&v) as i64)).unwrap_or(0) as usize;
    let b = e.map(|v| norm(v_num(&v) as i64)).unwrap_or(len) as usize;
    let (from, to) = if a <= b { (a, b) } else { (b, a) };
    match o {
        V::List(v) => V::List(v[from.min(v.len())..to.min(v.len())].to_vec()),
        V::Str(x) => {
            let ch: Vec<char> = x.chars().collect();
            V::Str(ch[from.min(ch.len())..to.min(ch.len())].iter().collect())
        }
        _ => V::Null,
    }
}

fn v_iter(o: &V) -> Vec<V> {
    match o {
        V::List(v) => v.clone(),
        V::Str(s) => s.chars().map(|c| V::Str(c.to_string())).collect(),
        _ => vec![],
    }
}

fn v_field(o: &V, f: &str) -> V {
    match o {
        V::Map(m) => m.get(f).cloned().unwrap_or(V::Null),
        V::Str(s) if f == "len" => V::Num(s.chars().count() as f64),
        V::List(v) if f == "len" => V::Num(v.len() as f64),
        _ => V::Null,
    }
}

fn b_emit(args: &[V]) -> V {
    let parts: Vec<String> = args.iter().map(disp).collect();
    println!("{}", parts.join(" "));
    V::Null
}

fn b_cap_first(a: &V) -> V {
    match a {
        V::Str(s) => {
            let mut ch: Vec<char> = s.chars().collect();
            if let Some(f) = ch.first_mut() {
                *f = f.to_uppercase().next().unwrap_or(*f);
            }
            V::Str(ch.iter().collect())
        }
        _ => V::Null,
    }
}

fn b_lower(a: &V) -> V {
    match a {
        V::Str(s) => V::Str(s.to_lowercase()),
        _ => V::Null,
    }
}

fn b_upper(a: &V) -> V {
    match a {
        V::Str(s) => V::Str(s.to_uppercase()),
        _ => V::Null,
    }
}

fn b_len(a: &V) -> V {
    match a {
        V::Str(s) => V::Num(s.chars().count() as f64),
        V::List(v) => V::Num(v.len() as f64),
        V::Map(m) => V::Num(m.len() as f64),
        _ => V::Null,
    }
}

fn b_split(a: &V, d: &V) -> V {
    match (a, d) {
        (V::Str(s), V::Str(p)) => {
            if p.is_empty() {
                V::List(s.chars().map(|c| V::Str(c.to_string())).collect())
            } else {
                V::List(s.split(p.as_str()).map(|x| V::Str(x.to_string())).collect())
            }
        }
        _ => V::Null,
    }
}

fn b_join(a: &V, d: &V) -> V {
    match (a, d) {
        (V::List(v), V::Str(p)) => {
            V::Str(v.iter().map(disp).collect::<Vec<_>>().join(p))
        }
        _ => V::Null,
    }
}

fn b_trim(a: &V) -> V {
    match a {
        V::Str(s) => V::Str(s.trim().to_string()),
        _ => V::Null,
    }
}

fn b_replace(a: &V, b: &V, c: &V) -> V {
    match (a, b, c) {
        (V::Str(s), V::Str(x), V::Str(y)) => V::Str(s.replace(x.as_str(), y.as_str())),
        _ => V::Null,
    }
}

fn b_starts(a: &V, b: &V) -> V {
    match (a, b) {
        (V::Str(s), V::Str(p)) => V::Bool(s.starts_with(p.as_str())),
        _ => V::Bool(false),
    }
}

fn b_ends(a: &V, b: &V) -> V {
    match (a, b) {
        (V::Str(s), V::Str(p)) => V::Bool(s.ends_with(p.as_str())),
        _ => V::Bool(false),
    }
}

fn b_contains(a: &V, b: &V) -> V {
    match (a, b) {
        (V::Str(h), V::Str(n)) => V::Bool(h.contains(n.as_str())),
        _ => V::Bool(false),
    }
}

fn b_str(a: &V) -> V {
    V::Str(disp(a))
}

fn b_num(a: &V) -> V {
    match a {
        V::Num(n) => V::Num(*n),
        V::Str(s) => V::Num(s.trim().parse().unwrap_or(0.0)),
        V::Bool(b) => V::Num(if *b { 1.0 } else { 0.0 }),
        _ => V::Null,
    }
}
"#;

pub fn transpile(src: &str) -> Result<String, ClangError> {
    let mut p = Parser::new(src);
    let prog = p.parse_program()?;
    let mut bad: Vec<String> = Vec::new();
    for s in &prog {
        collect_calls(s, &mut bad);
    }
    bad.sort();
    bad.dedup();
    let hit: Vec<String> = bad
        .into_iter()
        .filter(|n| NATIVE_UNSUPPORTED.contains(&n.as_str()))
        .collect();
    if !hit.is_empty() {
        return Err(ClangError::new(
            1,
            format!("native build cannot use {}", hit.join(", ")),
            "use plain compile for files, http, db, threads".to_string(),
        ));
    }
    let mut globals: HashSet<String> = HashSet::new();
    let mut funcs: Vec<(&String, &Vec<String>, &Vec<Stmt>)> = Vec::new();
    let mut mains: Vec<&Stmt> = Vec::new();
    for s in &prog {
        match s {
            Stmt::FuncDef(n, ps, b, _) => funcs.push((n, ps, b)),
            Stmt::VarDecl(n, _, _) => {
                globals.insert(n.clone());
                mains.push(s);
            }
            _ => mains.push(s),
        }
    }
    let mut out = String::new();
    out.push_str(RUNTIME);
    out.push('\n');
    for (n, ps, body) in funcs {
        out.push_str(&gen_func(n, ps, body, &globals)?);
        out.push('\n');
    }
    out.push_str("fn main() {\n");
    let mut locals: HashSet<String> = HashSet::new();
    for s in mains {
        out.push_str(&gen_stmt_main(s, &mut locals, &globals)?);
    }
    out.push_str("}\n");
    Ok(out)
}

fn collect_calls(s: &Stmt, acc: &mut Vec<String>) {
    match s {
        Stmt::VarDecl(_, e, _) | Stmt::Assign(_, e, _) => collect_expr(e, acc),
        Stmt::ExprStmt(e, _) | Stmt::Return(e, _) => collect_expr(e, acc),
        Stmt::Run(_, _) => {}
        Stmt::If(c, a, b, _) => {
            collect_expr(c, acc);
            for x in a {
                collect_calls(x, acc);
            }
            for x in b {
                collect_calls(x, acc);
            }
        }
        Stmt::For(_, it, body, _) => {
            collect_expr(it, acc);
            for x in body {
                collect_calls(x, acc);
            }
        }
        Stmt::FuncDef(_, _, body, _) => {
            for x in body {
                collect_calls(x, acc);
            }
        }
    }
}

fn collect_expr(e: &Expr, acc: &mut Vec<String>) {
    match e {
        Expr::Call(n, args, _) => {
            acc.push(n.clone());
            for a in args {
                collect_expr(a, acc);
            }
        }
        Expr::BinOp(l, _, r, _) => {
            collect_expr(l, acc);
            collect_expr(r, acc);
        }
        Expr::Neg(x, _) => collect_expr(x, acc),
        Expr::Attr(o, _, _) => collect_expr(o, acc),
        Expr::Index(o, i, _) => {
            collect_expr(o, acc);
            collect_expr(i, acc);
        }
        Expr::Slice { obj, start, end, .. } => {
            collect_expr(obj, acc);
            if let Some(s) = start {
                collect_expr(s, acc);
            }
            if let Some(x) = end {
                collect_expr(x, acc);
            }
        }
        Expr::List(items, _) => {
            for x in items {
                collect_expr(x, acc);
            }
        }
        _ => {}
    }
}

fn rust_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn expr_rust(e: &Expr, locals: &HashSet<String>, globals: &HashSet<String>) -> Result<String, ClangError> {
    match e {
        Expr::Str { val, is_f, .. } => {
            if *is_f {
                fstr_rust(val, locals, globals)
            } else {
                Ok(format!("V::Str({}.to_string())", rust_str(val)))
            }
        }
        Expr::Num(n, _) => Ok(format!("V::Num({:?})", n)),
        Expr::Bool(b, _) => Ok(format!("V::Bool({})", b)),
        Expr::Null(_) => Ok("V::Null".to_string()),
        Expr::List(items, _) => {
            let mut parts = Vec::new();
            for x in items {
                parts.push(expr_rust(x, locals, globals)?);
            }
            Ok(format!("V::List(vec![{}])", parts.join(", ")))
        }
        Expr::Var(n, _) => {
            if locals.contains(n) {
                Ok(format!("{}.clone()", n))
            } else if globals.contains(n) {
                Ok(format!("g_get({})", rust_str(n)))
            } else {
                Ok(format!("{}.clone()", n))
            }
        }
        Expr::Call(n, args, line) => {
            let mut parts = Vec::new();
            for a in args {
                parts.push(expr_rust(a, locals, globals)?);
            }
            match n.as_str() {
                "emit" => Ok(format!("b_emit(&[{}])", parts.join(", "))),
                "cap_first" => Ok(format!("b_cap_first(&{})", parts[0])),
                "lower" => Ok(format!("b_lower(&{})", parts[0])),
                "upper" => Ok(format!("b_upper(&{})", parts[0])),
                "contains" => Ok(format!("b_contains(&{}, &{})", parts[0], parts[1])),
                "len" => Ok(format!("b_len(&{})", parts[0])),
                "split" => Ok(format!("b_split(&{}, &{})", parts[0], parts[1])),
                "join" => Ok(format!("b_join(&{}, &{})", parts[0], parts[1])),
                "trim" => Ok(format!("b_trim(&{})", parts[0])),
                "replace" => Ok(format!(
                    "b_replace(&{}, &{}, &{})",
                    parts[0], parts[1], parts[2]
                )),
                "starts_with" => Ok(format!("b_starts(&{}, &{})", parts[0], parts[1])),
                "ends_with" => Ok(format!("b_ends(&{}, &{})", parts[0], parts[1])),
                "abs" => Ok(format!("V::Num(v_num(&{}).abs())", parts[0])),
                "floor" => Ok(format!("V::Num(v_num(&{}).floor())", parts[0])),
                "ceil" => Ok(format!("V::Num(v_num(&{}).ceil())", parts[0])),
                "sqrt" => Ok(format!("V::Num(v_num(&{}).sqrt())", parts[0])),
                "pow" => Ok(format!(
                    "V::Num(v_num(&{}).powf(v_num(&{})))",
                    parts[0], parts[1]
                )),
                "min" => Ok(format!(
                    "V::Num(v_num(&{}).min(v_num(&{})))",
                    parts[0], parts[1]
                )),
                "max" => Ok(format!(
                    "V::Num(v_num(&{}).max(v_num(&{})))",
                    parts[0], parts[1]
                )),
                "str" => Ok(format!("b_str(&{})", parts[0])),
                "num" => Ok(format!("b_num(&{})", parts[0])),
                "empty_list" => Ok("V::List(vec![])".to_string()),
                _ => {
                    let _ = line;
                    Ok(format!("{}({})", n, parts.join(", ")))
                }
            }
        }
        Expr::Attr(o, f, _) => {
            let base = expr_rust(o, locals, globals)?;
            Ok(format!("v_field(&({}), {})", base, rust_str(f)))
        }
        Expr::Index(o, i, _) => {
            let a = expr_rust(o, locals, globals)?;
            let b = expr_rust(i, locals, globals)?;
            Ok(format!("v_index(&({}), &({}))", a, b))
        }
        Expr::Slice { obj, start, end, .. } => {
            let a = expr_rust(obj, locals, globals)?;
            let s = match start {
                Some(e) => format!("Some({})", expr_rust(e, locals, globals)?),
                None => "None".to_string(),
            };
            let x = match end {
                Some(e) => format!("Some({})", expr_rust(e, locals, globals)?),
                None => "None".to_string(),
            };
            Ok(format!("v_slice(&({}), {}, {})", a, s, x))
        }
        Expr::BinOp(l, op, r, _) => {
            use crate::parser::Op;
            let a = expr_rust(l, locals, globals)?;
            let b = expr_rust(r, locals, globals)?;
            Ok(match op {
                Op::Add => format!("v_add(({a}), ({b}))", a = a, b = b),
                Op::Sub => format!("v_sub(({a}), ({b}))", a = a, b = b),
                Op::Mul => format!("v_mul(({a}), ({b}))", a = a, b = b),
                Op::Div => format!("v_div(({a}), ({b}))", a = a, b = b),
                Op::Eq => format!("V::Bool(v_eq(&({a}), &({b})))", a = a, b = b),
                Op::Ne => format!("V::Bool(!v_eq(&({a}), &({b})))", a = a, b = b),
                Op::Lt => format!("V::Bool(v_lt(&({a}), &({b})))", a = a, b = b),
                Op::Gt => format!("V::Bool(v_lt(&({b}), &({a})))", a = a, b = b),
                Op::Le => format!(
                    "V::Bool(v_lt(&({a}), &({b})) || v_eq(&({a}), &({b})))",
                    a = a,
                    b = b
                ),
                Op::Ge => format!(
                    "V::Bool(v_lt(&({b}), &({a})) || v_eq(&({a}), &({b})))",
                    a = a,
                    b = b
                ),
            })
        }
        Expr::Neg(x, _) => {
            let a = expr_rust(x, locals, globals)?;
            Ok(format!("V::Num(-v_num(&({})))", a))
        }
    }
}

fn fstr_rust(
    raw: &str,
    locals: &HashSet<String>,
    globals: &HashSet<String>,
) -> Result<String, ClangError> {
    let chars: Vec<char> = raw.chars().collect();
    let mut fmt = String::new();
    let mut args: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                fmt.push_str("{{");
                i += 2;
                continue;
            }
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '}' {
                j += 1;
            }
            if j >= chars.len() {
                return Err(ClangError::new(
                    1,
                    "f-string has '{' without '}'".to_string(),
                    "close it like f\"hi {name}\"".to_string(),
                ));
            }
            let inner: String = chars[i + 1..j].iter().collect();
            let mut p = Parser::new(inner.trim());
            let e = p.parse_expr()?;
            args.push(format!("disp(&({}))", expr_rust(&e, locals, globals)?));
            fmt.push_str("{}");
            i = j + 1;
        } else if chars[i] == '}' {
            fmt.push_str("}}");
            i += 1;
        } else {
            fmt.push(chars[i]);
            i += 1;
        }
    }
    if args.is_empty() {
        return Ok(format!("V::Str(format!({}))", rust_str(&fmt)));
    }
    let mut call = format!("V::Str(format!({}, ", rust_str(&fmt));
    call.push_str(&args.join(", "));
    call.push_str("))");
    Ok(call)
}

fn gen_func(
    name: &str,
    params: &[String],
    body: &[Stmt],
    globals: &HashSet<String>,
) -> Result<String, ClangError> {
    let mut out = String::new();
    let plist: Vec<String> = params.iter().map(|p| format!("{}: V", p)).collect();
    out.push_str(&format!("fn {}({}) -> V {{\n", name, plist.join(", ")));
    let mut locals: HashSet<String> = params.iter().cloned().collect();
    for s in body {
        out.push_str(&gen_stmt_fn(s, &mut locals, globals, params)?);
    }
    out.push_str("V::Null\n}\n");
    Ok(out)
}

fn gen_stmt_fn(
    s: &Stmt,
    locals: &mut HashSet<String>,
    globals: &HashSet<String>,
    params: &[String],
) -> Result<String, ClangError> {
    match s {
        Stmt::VarDecl(n, e, _) => {
            let v = expr_rust(e, locals, globals)?;
            if locals.contains(n) || params.contains(n) {
                Ok(format!("{} = {}; \n", n, v))
            } else if globals.contains(n) {
                Ok(format!("g_set({}, {}); \n", rust_str(n), v))
            } else {
                locals.insert(n.clone());
                Ok(format!("let mut {} = {}; \n", n, v))
            }
        }
        Stmt::Assign(n, e, _) => {
            let v = expr_rust(e, locals, globals)?;
            if locals.contains(n) || params.contains(n) {
                Ok(format!("{} = {}; \n", n, v))
            } else if globals.contains(n) {
                Ok(format!("g_set({}, {}); \n", rust_str(n), v))
            } else {
                locals.insert(n.clone());
                Ok(format!("let mut {} = {}; \n", n, v))
            }
        }
        Stmt::ExprStmt(e, _) => Ok(format!("let _ = {}; \n", expr_rust(e, locals, globals)?)),
        Stmt::Return(e, _) => Ok(format!("return {}; \n", expr_rust(e, locals, globals)?)),
        Stmt::Run(n, _) => Ok(format!("{}(); \n", n)),
        Stmt::If(c, a, b, _) => {
            let cond = expr_rust(c, locals, globals)?;
            let mut s = format!("if truthy(&({})) {{ \n", cond);
            let mut la = locals.clone();
            for x in a {
                s.push_str(&gen_stmt_fn(x, &mut la, globals, params)?);
            }
            s.push_str("} ");
            if b.is_empty() {
                s.push_str("\n");
            } else {
                s.push_str("else { \n");
                let mut lb = locals.clone();
                for x in b {
                    s.push_str(&gen_stmt_fn(x, &mut lb, globals, params)?);
                }
                s.push_str("} \n");
            }
            Ok(s)
        }
        Stmt::For(v, it, body, _) => {
            let iv = expr_rust(it, locals, globals)?;
            let mut s = format!("for {} in v_iter(&({})) {{ \n", v, iv);
            let mut lb = locals.clone();
            lb.insert(v.clone());
            for x in body {
                s.push_str(&gen_stmt_fn(x, &mut lb, globals, params)?);
            }
            s.push_str("} \n");
            Ok(s)
        }
        Stmt::FuncDef(..) => Ok(String::new()),
    }
}

fn gen_stmt_main(
    s: &Stmt,
    locals: &mut HashSet<String>,
    globals: &HashSet<String>,
) -> Result<String, ClangError> {
    match s {
        Stmt::VarDecl(n, e, _) => {
            let v = expr_rust(e, locals, globals)?;
            Ok(format!("g_set({}, {}); \n", rust_str(n), v))
        }
        Stmt::Assign(n, e, _) => {
            let v = expr_rust(e, locals, globals)?;
            if globals.contains(n) {
                Ok(format!("g_set({}, {}); \n", rust_str(n), v))
            } else if locals.contains(n) {
                Ok(format!("{} = {}; \n", n, v))
            } else {
                locals.insert(n.clone());
                Ok(format!("let mut {} = {}; \n", n, v))
            }
        }
        Stmt::ExprStmt(e, _) => Ok(format!("let _ = {}; \n", expr_rust(e, locals, globals)?)),
        Stmt::Return(_, line) => Err(ClangError::new(
            *line,
            "return outside a function".to_string(),
            "move return inside fn::name { ... }".to_string(),
        )),
        Stmt::Run(n, _) => Ok(format!("{}(); \n", n)),
        Stmt::If(c, a, b, _) => {
            let cond = expr_rust(c, locals, globals)?;
            let mut s = format!("if truthy(&({})) {{ \n", cond);
            let mut la = locals.clone();
            for x in a {
                s.push_str(&gen_stmt_main(x, &mut la, globals)?);
            }
            s.push_str("} ");
            if b.is_empty() {
                s.push_str("\n");
            } else {
                s.push_str("else { \n");
                let mut lb = locals.clone();
                for x in b {
                    s.push_str(&gen_stmt_main(x, &mut lb, globals)?);
                }
                s.push_str("} \n");
            }
            Ok(s)
        }
        Stmt::For(v, it, body, _) => {
            let iv = expr_rust(it, locals, globals)?;
            let mut s = format!("for {} in v_iter(&({})) {{ \n", v, iv);
            let mut lb = locals.clone();
            lb.insert(v.clone());
            for x in body {
                s.push_str(&gen_stmt_main(x, &mut lb, globals)?);
            }
            s.push_str("} \n");
            Ok(s)
        }
        Stmt::FuncDef(..) => Ok(String::new()),
    }
}

pub fn native_build(src_path: &str, out_path: &str) -> Result<(), ClangError> {
    let src = std::fs::read_to_string(src_path).map_err(|e| {
        ClangError::new(
            1,
            format!("cannot read '{}': {}", src_path, e),
            "check the file path".to_string(),
        )
    })?;
    let code = transpile(&src)?;
    let dir = std::env::temp_dir().join(format!("clng_native_{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| {
        ClangError::new(1, format!("temp failed: {}", e), "retry the build".to_string())
    })?;
    let rs_path = dir.join("app.rs");
    std::fs::write(&rs_path, code).map_err(|e| {
        ClangError::new(1, format!("temp write failed: {}", e), "retry the build".to_string())
    })?;
    let status = std::process::Command::new("rustc")
        .arg("--edition=2021")
        .arg("-O")
        .arg(&rs_path)
        .arg("-o")
        .arg(out_path)
        .status()
        .map_err(|e| {
            ClangError::new(
                1,
                format!("rustc not found: {}", e),
                "install Rust from rustup".to_string(),
            )
        })?;
    if !status.success() {
        return Err(ClangError::new(
            1,
            "rustc build failed".to_string(),
            "run with plain compile instead".to_string(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(out_path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(out_path, perms);
        }
    }
    println!("native binary at {}", out_path);
    Ok(())
}
