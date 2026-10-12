use std::collections::HashMap;
use crate::error::ClangError;
use crate::parser::{Expr, Op, Parser, Stmt};
use crate::value::Value;

enum Signal {
    None,
    Return(Value),
}

pub struct Interp {
    scopes: Vec<HashMap<String, Value>>,
    funcs: HashMap<String, (Vec<String>, Vec<Stmt>, usize)>,
    pub printed: Vec<String>,
    pub quiet: bool,
    pub trace: bool,
    pub trace_log: Vec<String>,
    rng: u64,
    cli: Vec<String>,
}

impl Interp {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            funcs: HashMap::new(),
            printed: Vec::new(),
            quiet: false,
            trace: false,
            trace_log: Vec::new(),
            rng: 0x12345678,
            cli: Vec::new(),
        }
    }

    pub fn set_cli(&mut self, args: Vec<String>) {
        self.cli = args;
    }

    pub fn run_test_fns(
        &mut self,
        src: &str,
    ) -> Result<Vec<(String, Result<(), ClangError>)>, ClangError> {
        let mut p = Parser::new(src);
        let prog = p.parse_program()?;
        let mut setup: Vec<Stmt> = Vec::new();
        let mut tests: Vec<String> = Vec::new();
        for s in &prog {
            match s {
                Stmt::FuncDef(n, ps, b, line) if n.starts_with("test_") => {
                    if !ps.is_empty() {
                        return Err(ClangError::new(
                            *line,
                            format!("test '{}' must take no inputs", n),
                            "write fn::test_name() { ... }".to_string(),
                        ));
                    }
                    if self.funcs.contains_key(n) {
                        return Err(ClangError::new(
                            *line,
                            format!("function '{}' is defined twice", n),
                            "rename one of them".to_string(),
                        ));
                    }
                    self.funcs.insert(n.clone(), (ps.clone(), b.clone(), *line));
                    tests.push(n.clone());
                }
                Stmt::FuncDef(n, ps, b, line) => {
                    if self.funcs.contains_key(n) {
                        return Err(ClangError::new(
                            *line,
                            format!("function '{}' is defined twice", n),
                            "rename one of them".to_string(),
                        ));
                    }
                    self.funcs.insert(n.clone(), (ps.clone(), b.clone(), *line));
                }
                _ => setup.push(s.clone()),
            }
        }
        self.scopes.push(HashMap::new());
        let setup_res = self.exec_block(&setup);
        self.scopes.pop();
        if let Err(e) = setup_res {
            return Err(e);
        }
        tests.sort();
        let mut out = Vec::new();
        for name in tests {
            let (ps, body, _) = self.funcs.get(&name).cloned().unwrap_or_default();
            let _ = ps;
            self.scopes.push(HashMap::new());
            let r = match self.exec_block(&body) {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            };
            self.scopes.pop();
            out.push((name, r));
        }
        Ok(out)
    }

    fn next_rand(&mut self) -> u64 {
        self.rng = self
            .rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.rng >> 33) & 0x7fffffff
    }

    pub fn run_source(&mut self, src: &str) -> Result<(), ClangError> {
        let mut p = Parser::new(src);
        let prog = p.parse_program()?;
        let walk = std::env::var("CLNG_MODE").unwrap_or_default() == "walk";
        if !walk {
            match crate::bytecode::try_run_opts(
                &prog,
                self.cli.clone(),
                self.trace,
                self.quiet,
            ) {
                Ok((out, log)) => {
                    self.printed.extend(out);
                    self.trace_log.extend(log);
                    return Ok(());
                }
                Err(crate::bytecode::FastError::Unsupported(_)) => {}
                Err(crate::bytecode::FastError::Clang(e)) => return Err(e),
            }
        }
        self.run_program(&prog)
    }

    pub(crate) fn run_program(&mut self, prog: &[Stmt]) -> Result<(), ClangError> {
        for s in prog {
            if let Stmt::FuncDef(name, params, body, line) = s {
                if self.funcs.contains_key(name) {
                    return Err(ClangError::new(
                        *line,
                        format!("function '{}' is defined twice", name),
                        "rename one of them".to_string(),
                    ));
                }
                self.funcs
                    .insert(name.clone(), (params.clone(), body.clone(), *line));
            }
        }
        let mut mains: Vec<Stmt> = Vec::new();
        for s in prog {
            if !matches!(s, Stmt::FuncDef(..)) {
                mains.push(s.clone());
            }
        }
        match self.exec_block(&mains)? {
            Signal::None => Ok(()),
            Signal::Return(_) => Err(ClangError::new(
                1,
                "return outside a function".to_string(),
                "move return inside fn::name { ... }".to_string(),
            )),
        }
    }

    fn exec_block(&mut self, stmts: &[Stmt]) -> Result<Signal, ClangError> {
        for s in stmts {
            match self.exec_stmt(s)? {
                Signal::None => {}
                r @ Signal::Return(_) => return Ok(r),
            }
        }
        Ok(Signal::None)
    }

    fn exec_stmt(&mut self, s: &Stmt) -> Result<Signal, ClangError> {
        if self.trace && self.trace_log.len() < 5000 {
            self.trace_log.push(format!("{}:{}", stmt_kind(s), stmt_line(s)));
        }
        match s {
            Stmt::VarDecl(name, e, line) => {
                let v = self.eval(e)?;
                if !self.set_existing(name, v.clone()) {
                    self.set(name, v);
                }
                let _ = line;
                Ok(Signal::None)
            }
            Stmt::Assign(name, e, line) => {
                let v = self.eval(e)?;
                if !self.set_existing(name, v.clone()) {
                    if !self.has(name) {
                        return Err(ClangError::new(
                            *line,
                            format!("unknown name '{}'", name),
                            format!("declare it first with var:{} = ...", name),
                        ));
                    }
                    self.set(name, v);
                }
                Ok(Signal::None)
            }
            Stmt::ExprStmt(e, _) => {
                self.eval(e)?;
                Ok(Signal::None)
            }
            Stmt::Return(e, _) => {
                let v = self.eval(e)?;
                Ok(Signal::Return(v))
            }
            Stmt::Run(name, line) => {
                let (params, body, _) = self.funcs.get(name).cloned().ok_or_else(|| {
                    ClangError::new(
                        *line,
                        format!("unknown function '{}'", name),
                        "define it with fn::name() { ... } first".to_string(),
                    )
                })?;
                if !params.is_empty() {
                    return Err(ClangError::new(
                        *line,
                        format!("function '{}' needs inputs", name),
                        "call it with values like name(1, 2)".to_string(),
                    ));
                }
                self.scopes.push(HashMap::new());
                let sig = self.exec_block(&body)?;
                self.scopes.pop();
                match sig {
                    Signal::None => Ok(Signal::None),
                    Signal::Return(_) => Ok(Signal::None),
                }
            }
            Stmt::If(cond, then_b, else_b, _) => {
                let c = self.eval(cond)?;
                if c.is_truthy() {
                    self.scopes.push(HashMap::new());
                    let r = self.exec_block(then_b)?;
                    self.scopes.pop();
                    Ok(r)
                } else {
                    self.scopes.push(HashMap::new());
                    let r = self.exec_block(else_b)?;
                    self.scopes.pop();
                    Ok(r)
                }
            }
            Stmt::For(var, iter, body, line) => {
                let it = self.eval(iter)?;
                let items = match it {
                    Value::List(v) => v,
                    Value::Str(s) => s.chars().map(|c| Value::Str(c.to_string())).collect(),
                    _ => {
                        return Err(ClangError::new(
                            *line,
                            format!("for loop needs a list, got {}", it.type_name()),
                            "pass a list like [1, 2]".to_string(),
                        ))
                    }
                };
                for item in items {
                    self.scopes.push(HashMap::new());
                    self.set(var, item);
                    match self.exec_block(body)? {
                        Signal::None => {
                            self.scopes.pop();
                        }
                        r @ Signal::Return(_) => {
                            self.scopes.pop();
                            return Ok(r);
                        }
                    }
                }
                Ok(Signal::None)
            }
            Stmt::FuncDef(..) => Ok(Signal::None),
        }
    }

    fn get(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    fn set(&mut self, name: &str, v: Value) {
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_string(), v);
        }
    }

    fn set_existing(&mut self, name: &str, v: Value) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), v);
                return true;
            }
        }
        false
    }

    fn eval(&mut self, e: &Expr) -> Result<Value, ClangError> {
        match e {
            Expr::Str { val, is_f, line, .. } => {
                if *is_f {
                    Ok(Value::Str(self.render_fstr(val, *line)?))
                } else {
                    Ok(Value::Str(val.clone()))
                }
            }
            Expr::Num(n, _) => Ok(Value::Num(*n)),
            Expr::Bool(b, _) => Ok(Value::Bool(*b)),
            Expr::Null(_) => Ok(Value::Null),
            Expr::List(items, _) => {
                let mut out = Vec::new();
                for it in items {
                    out.push(self.eval(it)?);
                }
                Ok(Value::List(out))
            }
            Expr::Var(name, line) => self.get(name).ok_or_else(|| {
                ClangError::new(
                    *line,
                    format!("unknown name '{}'", name),
                    format!("declare it first with var:{} = ...", name),
                )
            }),
            Expr::Call(name, args, line) => self.eval_call(name, args, *line),
            Expr::Attr(obj, field, line) => {
                let base = self.eval(obj)?;
                match base {
                    Value::Map(m) => m.get(field).cloned().ok_or_else(|| {
                        ClangError::new(
                            *line,
                            format!("missing field '{}'", field),
                            "check the names inside the map".to_string(),
                        )
                    }),
                    Value::Str(s) => match field.as_str() {
                        "len" => Ok(Value::Num(s.chars().count() as f64)),
                        _ => Err(ClangError::new(
                            *line,
                            format!("str has no field '{}'", field),
                            "use len(x) or cap_first(x)".to_string(),
                        )),
                    },
                    Value::List(v) => match field.as_str() {
                        "len" => Ok(Value::Num(v.len() as f64)),
                        _ => Err(ClangError::new(
                            *line,
                            format!("list has no field '{}'", field),
                            "use len(x) or push(x, item)".to_string(),
                        )),
                    },
                    other => Err(ClangError::new(
                        *line,
                        format!("{} has no field '{}'", other.type_name(), field),
                        "only maps carry named fields".to_string(),
                    )),
                }
            }
            Expr::Index(obj, idx, line) => {
                let base = self.eval(obj)?;
                let i = self.eval(idx)?;
                match (base, i) {
                    (Value::List(v), Value::Num(n)) => {
                        let j = n as i64;
                        let pos = if j < 0 { v.len() as i64 + j } else { j } as usize;
                        v.get(pos).cloned().ok_or_else(|| {
                            ClangError::new(
                                *line,
                                format!("index {} is past the list", j),
                                format!("list holds {} items", v.len()),
                            )
                        })
                    }
                    (Value::Str(s), Value::Num(n)) => {
                        let chars: Vec<char> = s.chars().collect();
                        let j = n as i64;
                        let pos = if j < 0 {
                            chars.len() as i64 + j
                        } else {
                            j
                        } as usize;
                        match chars.get(pos) {
                            Some(c) => Ok(Value::Str(c.to_string())),
                            None => Err(ClangError::new(
                                *line,
                                format!("index {} is past the string", j),
                                "keep the index inside the text".to_string(),
                            )),
                        }
                    }
                    (Value::Map(m), Value::Str(k)) => m.get(&k).cloned().ok_or_else(|| {
                        ClangError::new(
                            *line,
                            format!("missing key '{}'", k),
                            "check the map keys".to_string(),
                        )
                    }),
                    (b, k) => Err(ClangError::new(
                        *line,
                        format!(
                            "cannot index {} with {}",
                            b.type_name(),
                            k.type_name()
                        ),
                        "use a number for lists and strings".to_string(),
                    )),
                }
            }
            Expr::Slice { obj, start, end, line } => {
                let base = self.eval(obj)?;
                let s_idx = match start {
                    Some(e) => match self.eval(e)? {
                        Value::Num(n) => n as i64,
                        other => {
                            return Err(ClangError::new(
                                *line,
                                format!("slice start must be a number, got {}", other.type_name()),
                                "write r[1:] or r[0]".to_string(),
                            ))
                        }
                    },
                    None => 0,
                };
                let items_len: i64 = match &base {
                    Value::List(v) => v.len() as i64,
                    Value::Str(s) => s.chars().count() as i64,
                    other => {
                        return Err(ClangError::new(
                            *line,
                            format!("cannot slice {}", other.type_name()),
                            "slice a list or a string".to_string(),
                        ))
                    }
                };
                let e_idx = match end {
                    Some(e) => match self.eval(e)? {
                        Value::Num(n) => n as i64,
                        other => {
                            return Err(ClangError::new(
                                *line,
                                format!("slice end must be a number, got {}", other.type_name()),
                                "write r[1:3]".to_string(),
                            ))
                        }
                    },
                    None => items_len,
                };
                let norm = |v: i64| {
                    if v < 0 {
                        (items_len + v).max(0).min(items_len)
                    } else {
                        v.max(0).min(items_len)
                    }
                };
                let a = norm(s_idx) as usize;
                let b = norm(e_idx) as usize;
                let (from, to) = if a <= b { (a, b) } else { (b, a) };
                match base {
                    Value::List(v) => Ok(Value::List(v[from.min(v.len())..to.min(v.len())].to_vec())),
                    Value::Str(s) => {
                        let chars: Vec<char> = s.chars().collect();
                        Ok(Value::Str(chars[from.min(chars.len())..to.min(chars.len())].iter().collect()))
                    }
                    _ => unreachable!(),
                }
            }
            Expr::BinOp(l, op, r, line) => self.eval_bin(l, op, r, *line),
            Expr::Neg(inner, _) => match self.eval(inner)? {
                Value::Num(n) => Ok(Value::Num(-n)),
                other => Err(ClangError::new(
                    Parser::expr_line(e),
                    format!("cannot negate {}", other.type_name()),
                    "use - only on numbers".to_string(),
                )),
            },
        }
    }

    fn eval_bin(
        &mut self,
        l: &Expr,
        op: &Op,
        r: &Expr,
        line: usize,
    ) -> Result<Value, ClangError> {
        let a = self.eval(l)?;
        let b = self.eval(r)?;
        match op {
            Op::Add => match (a, b) {
                (Value::Num(x), Value::Num(y)) => Ok(Value::Num(x + y)),
                (Value::Str(x), Value::Str(y)) => Ok(Value::Str(x + &y)),
                (Value::Str(x), Value::Num(y)) => Ok(Value::Str(format!("{}{}", x, Value::Num(y).display()))),
                (Value::Num(x), Value::Str(y)) => Ok(Value::Str(format!("{}{}", Value::Num(x).display(), y))),
                (Value::List(mut x), Value::List(mut y)) => {
                    x.append(&mut y);
                    Ok(Value::List(x))
                }
                (x, y) => Err(ClangError::new(
                    line,
                    format!("cannot add {} and {}", x.type_name(), y.type_name()),
                    "add numbers, join strings, or concat lists".to_string(),
                )),
            },
            Op::Sub => match (a, b) {
                (Value::Num(x), Value::Num(y)) => Ok(Value::Num(x - y)),
                (x, y) => Err(ClangError::new(
                    line,
                    format!("cannot subtract {} and {}", x.type_name(), y.type_name()),
                    "use - only on numbers".to_string(),
                )),
            },
            Op::Mul => match (a, b) {
                (Value::Num(x), Value::Num(y)) => Ok(Value::Num(x * y)),
                (Value::Str(s), Value::Num(n)) | (Value::Num(n), Value::Str(s)) => {
                    let k = n as i64;
                    if k < 0 || k > 10000 {
                        return Err(ClangError::new(
                            line,
                            "repeat count is out of range".to_string(),
                            "keep it between 0 and 10000".to_string(),
                        ));
                    }
                    Ok(Value::Str(s.repeat(k as usize)))
                }
                (x, y) => Err(ClangError::new(
                    line,
                    format!("cannot multiply {} and {}", x.type_name(), y.type_name()),
                    "use * on numbers".to_string(),
                )),
            },
            Op::Div => match (a, b) {
                (Value::Num(_), Value::Num(y)) if y == 0.0 => Err(ClangError::new(
                    line,
                    "division by zero".to_string(),
                    "guard with if(b == 0) before dividing".to_string(),
                )),
                (Value::Num(x), Value::Num(y)) => Ok(Value::Num(x / y)),
                (x, y) => Err(ClangError::new(
                    line,
                    format!("cannot divide {} and {}", x.type_name(), y.type_name()),
                    "use / only on numbers".to_string(),
                )),
            },
            Op::Eq => Ok(Value::Bool(values_equal(&a, &b))),
            Op::Ne => Ok(Value::Bool(!values_equal(&a, &b))),
            Op::Lt | Op::Gt | Op::Le | Op::Ge => {
                let ord = compare_values(&a, &b).ok_or_else(|| {
                    ClangError::new(
                        line,
                        format!("cannot compare {} and {}", a.type_name(), b.type_name()),
                        "compare numbers or strings of the same kind".to_string(),
                    )
                })?;
                let out = match op {
                    Op::Lt => ord < 0,
                    Op::Gt => ord > 0,
                    Op::Le => ord <= 0,
                    Op::Ge => ord >= 0,
                    _ => false,
                };
                Ok(Value::Bool(out))
            }
        }
    }

    fn render_fstr(&mut self, raw: &str, line: usize) -> Result<String, ClangError> {
        let mut out = String::new();
        let chars: Vec<char> = raw.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '{' {
                if i + 1 < chars.len() && chars[i + 1] == '{' {
                    out.push('{');
                    i += 2;
                    continue;
                }
                let mut j = i + 1;
                let mut depth = 1;
                while j < chars.len() {
                    if chars[j] == '{' {
                        depth += 1;
                    } else if chars[j] == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if j >= chars.len() {
                    return Err(ClangError::new(
                        line,
                        "f-string has '{' without '}'".to_string(),
                        "close it like f\"hi {name}\"".to_string(),
                    ));
                }
                let inner: String = chars[i + 1..j].iter().collect();
                let inner = inner.trim();
                if inner.is_empty() {
                    return Err(ClangError::new(
                        line,
                        "empty {} in f-string".to_string(),
                        "put a name inside like {name}".to_string(),
                    ));
                }
                let mut p = Parser::new(inner);
                let expr = p.parse_expr().map_err(|_| {
                    ClangError::new(
                        line,
                        format!("bad placeholder '{{{}}}'", inner),
                        "use a name or field like {user.name}".to_string(),
                    )
                })?;
                let v = self.eval(&expr)?;
                out.push_str(&v.display());
                i = j + 1;
            } else if chars[i] == '}' && i + 1 < chars.len() && chars[i + 1] == '}' {
                out.push('}');
                i += 2;
            } else if chars[i] == '}' {
                return Err(ClangError::new(
                    line,
                    "f-string has '}' without '{'".to_string(),
                    "open it like f\"hi {name}\"".to_string(),
                ));
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        Ok(out)
    }

    fn eval_call(
        &mut self,
        name: &str,
        args: &[Expr],
        line: usize,
    ) -> Result<Value, ClangError> {
        if let Some((params, body, _)) = self.funcs.get(name).cloned() {
            if params.len() != args.len() {
                return Err(ClangError::new(
                    line,
                    format!(
                        "function '{}' wants {} inputs but got {}",
                        name,
                        params.len(),
                        args.len()
                    ),
                    format!("call it like {}({})", name, params.join(", ")),
                ));
            }
            let mut vals = Vec::new();
            for a in args {
                vals.push(self.eval(a)?);
            }
            self.scopes.push(HashMap::new());
            for (k, v) in params.iter().zip(vals) {
                self.set(k, v);
            }
            let sig = self.exec_block(&body)?;
            self.scopes.pop();
            match sig {
                Signal::None => Ok(Value::Null),
                Signal::Return(v) => Ok(v),
            }
        } else {
            self.eval_builtin(name, args, line)
        }
    }

    fn eval_builtin(
        &mut self,
        name: &str,
        args: &[Expr],
        line: usize,
    ) -> Result<Value, ClangError> {
        match name {
            "emit" => {
                let mut parts = Vec::new();
                for a in args {
                    parts.push(self.eval(a)?.display());
                }
                let text = if parts.is_empty() {
                    String::new()
                } else if parts.len() == 1 {
                    parts[0].clone()
                } else {
                    parts.join(" ")
                };
                if !self.quiet {
                    println!("{}", text);
                }
                self.printed.push(text);
                Ok(Value::Null)
            }
            "cap_first" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Str(s) => {
                        let mut ch: Vec<char> = s.chars().collect();
                        if let Some(first) = ch.first_mut() {
                            *first = first.to_uppercase().next().unwrap_or(*first);
                        }
                        Ok(Value::Str(ch.iter().collect()))
                    }
                    other => Err(ClangError::new(
                        line,
                        format!("cap_first needs a string, got {}", other.type_name()),
                        "call cap_first(name)".to_string(),
                    )),
                }
            }
            "lower" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Str(s) => Ok(Value::Str(s.to_lowercase())),
                    other => Err(bad_type(line, "lower", "string", other.type_name())),
                }
            }
            "upper" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Str(s) => Ok(Value::Str(s.to_uppercase())),
                    other => Err(bad_type(line, "upper", "string", other.type_name())),
                }
            }
            "contains" => {
                need_args(name, args, 2, line)?;
                let hay = self.eval(&args[0])?;
                let needle = self.eval(&args[1])?;
                match (hay, needle) {
                    (Value::Str(h), Value::Str(n)) => Ok(Value::Bool(h.contains(&n))),
                    (Value::List(v), item) => Ok(Value::Bool(v.iter().any(|x| values_equal(x, &item)))),
                    (h, n) => Err(ClangError::new(
                        line,
                        format!("contains cannot search {} in {}", n.type_name(), h.type_name()),
                        "use contains(text, part) or contains(list, item)".to_string(),
                    )),
                }
            }
            "len" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Str(s) => Ok(Value::Num(s.chars().count() as f64)),
                    Value::List(v) => Ok(Value::Num(v.len() as f64)),
                    Value::Map(m) => Ok(Value::Num(m.len() as f64)),
                    other => Err(ClangError::new(
                        line,
                        format!("len cannot measure {}", other.type_name()),
                        "pass a string, list, or map".to_string(),
                    )),
                }
            }
            "push" => {
                need_args(name, args, 2, line)?;
                let item = self.eval(&args[1])?.clone();
                if let Expr::Var(var_name, _) = &args[0] {
                    let cur = self.get(var_name).ok_or_else(|| {
                        ClangError::new(
                            line,
                            format!("unknown name '{}'", var_name),
                            format!("declare it first with var:{} = []", var_name),
                        )
                    })?;
                    match cur {
                        Value::List(mut v) => {
                            v.push(item);
                            let n = v.len() as f64;
                            let val = Value::List(v);
                            if !self.set_existing(var_name, val.clone()) {
                                self.set(var_name, val);
                            }
                            Ok(Value::Num(n))
                        }
                        other => Err(ClangError::new(
                            line,
                            format!("push needs a list, '{}' is {}", var_name, other.type_name()),
                            "start it with var:name = []".to_string(),
                        )),
                    }
                } else {
                    let base = self.eval(&args[0])?;
                    match base {
                        Value::List(mut v) => {
                            v.push(item);
                            Ok(Value::List(v))
                        }
                        other => Err(ClangError::new(
                            line,
                            format!("push needs a list, got {}", other.type_name()),
                            "call push(items, value)".to_string(),
                        )),
                    }
                }
            }
            "empty_list" => {
                need_args(name, args, 0, line)?;
                Ok(Value::List(Vec::new()))
            }
            "mk_user" => {
                need_args(name, args, 3, line)?;
                let n = self.eval(&args[0])?;
                let m = self.eval(&args[1])?;
                let a = self.eval(&args[2])?;
                let (ns, ms) = match (n, m) {
                    (Value::Str(x), Value::Str(y)) => (x, y),
                    _ => {
                        return Err(ClangError::new(
                            line,
                            "mk_user needs (name, mail, age)".to_string(),
                            "call mk_user(\"Ana\", \"a@mail.com\", 31)".to_string(),
                        ))
                    }
                };
                let age = match a {
                    Value::Num(v) => v,
                    _ => {
                        return Err(ClangError::new(
                            line,
                            "mk_user age must be a number".to_string(),
                            "call mk_user(\"Ana\", \"a@mail.com\", 31)".to_string(),
                        ))
                    }
                };
                let mut map = HashMap::new();
                map.insert("name".to_string(), Value::Str(ns));
                map.insert("mail".to_string(), Value::Str(ms));
                map.insert("age".to_string(), Value::Num(age));
                Ok(Value::Map(map))
            }
            "sort" => {
                need_args(name, args, 3, line)?;
                let base = self.eval(&args[0])?;
                let key = self.eval(&args[1])?;
                let order = self.eval(&args[2])?;
                let (k, o) = match (key, order) {
                    (Value::Str(a), Value::Str(b)) => (a, b),
                    _ => {
                        return Err(ClangError::new(
                            line,
                            "sort needs (list, key, order)".to_string(),
                            "call sort(users, \"age\", \"asc\")".to_string(),
                        ))
                    }
                };
                match base {
                    Value::List(mut v) => {
                        v.sort_by(|a, b| {
                            let av = field_for_sort(a, &k);
                            let bv = field_for_sort(b, &k);
                            compare_values(&av, &bv).unwrap_or(0).cmp(&0)
                        });
                        if o == "desc" {
                            v.reverse();
                        } else if o != "asc" {
                            return Err(ClangError::new(
                                line,
                                format!("sort order '{}' is unknown", o),
                                "use \"asc\" or \"desc\"".to_string(),
                            ));
                        }
                        Ok(Value::List(v))
                    }
                    other => Err(ClangError::new(
                        line,
                        format!("sort needs a list, got {}", other.type_name()),
                        "call sort(users, \"age\", \"asc\")".to_string(),
                    )),
                }
            }
            "file_read" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Str(p) => match std::fs::read_to_string(&p) {
                        Ok(s) => Ok(Value::Str(s)),
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            Ok(Value::Str(String::new()))
                        }
                        Err(e) => Err(ClangError::new(
                            line,
                            format!("cannot read file '{}': {}", p, e),
                            "check the path and permissions".to_string(),
                        )),
                    },
                    other => Err(bad_type(line, "file_read", "string path", other.type_name())),
                }
            }
            "file_write" => {
                need_args(name, args, 2, line)?;
                let p = self.eval(&args[0])?;
                let t = self.eval(&args[1])?;
                match (p, t) {
                    (Value::Str(path), Value::Str(text)) => {
                        std::fs::write(&path, text).map_err(|e| {
                            ClangError::new(
                                line,
                                format!("cannot write file '{}': {}", path, e),
                                "check the folder exists".to_string(),
                            )
                        })?;
                        Ok(Value::Null)
                    }
                    _ => Err(ClangError::new(
                        line,
                        "file_write needs (path, text)".to_string(),
                        "call file_write(\"a.txt\", body)".to_string(),
                    )),
                }
            }
            "json_parse" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Str(s) => {
                        let t = s.trim();
                        if t.is_empty() {
                            return Ok(Value::List(Vec::new()));
                        }
                        parse_json_value(t, line)
                    }
                    other => Err(bad_type(line, "json_parse", "string", other.type_name())),
                }
            }
            "json_dump" => {
                need_args(name, args, 1, line)?;
                let v = self.eval(&args[0])?;
                Ok(Value::Str(json_dump(&v)))
            }
            "http_post" => {
                need_args(name, args, 2, line)?;
                let url = self.eval(&args[0])?;
                let body = self.eval(&args[1])?;
                match (url, body) {
                    (Value::Str(u), Value::Str(b)) => {
                        if u.starts_with("https://") {
                            do_https("POST", &u, Some(&b), line)
                        } else {
                            do_http("POST", &u, Some(&b), line)
                        }
                    }
                    _ => Err(ClangError::new(
                        line,
                        "http_post needs (url, body)".to_string(),
                        "call http_post(url, text)".to_string(),
                    )),
                }
            }
            "http_get" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Str(u) => {
                        if u.starts_with("https://") {
                            do_https("GET", &u, None, line)
                        } else {
                            do_http("GET", &u, None, line)
                        }
                    }
                    other => Err(bad_type(line, "http_get", "string url", other.type_name())),
                }
            }
            "db_put" => {
                need_args(name, args, 3, line)?;
                let path = self.eval(&args[0])?;
                let key = self.eval(&args[1])?;
                let val = self.eval(&args[2])?;
                match (path, key) {
                    (Value::Str(p), Value::Str(k)) => {
                        let mut m = load_db_map(&p, line)?;
                        m.insert(k, val.clone());
                        save_db_map(&p, &m, line)?;
                        Ok(val)
                    }
                    _ => Err(ClangError::new(
                        line,
                        "db_put needs (path, key, value)".to_string(),
                        "call db_put(\"a.json\", \"k\", \"v\")".to_string(),
                    )),
                }
            }
            "db_get" => {
                need_args(name, args, 2, line)?;
                match (self.eval(&args[0])?, self.eval(&args[1])?) {
                    (Value::Str(p), Value::Str(k)) => {
                        let m = load_db_map(&p, line)?;
                        Ok(m.get(&k).cloned().unwrap_or(Value::Null))
                    }
                    _ => Err(ClangError::new(
                        line,
                        "db_get needs (path, key)".to_string(),
                        "call db_get(\"a.json\", \"k\")".to_string(),
                    )),
                }
            }
            "db_list" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Str(p) => {
                        let m = load_db_map(&p, line)?;
                        let mut ks: Vec<Value> =
                            m.keys().map(|k| Value::Str(k.clone())).collect();
                        ks.sort_by(|a, b| a.display().cmp(&b.display()));
                        Ok(Value::List(ks))
                    }
                    other => Err(bad_type(line, "db_list", "string path", other.type_name())),
                }
            }
            "pmap" => {
                need_args(name, args, 2, line)?;
                let items = self.eval(&args[0])?;
                let fname = self.eval(&args[1])?;
                match (items, fname) {
                    (Value::List(v), Value::Str(f)) => {
                        let (params, body, _) = self.funcs.get(&f).cloned().ok_or_else(|| {
                            ClangError::new(
                                line,
                                format!("unknown function '{}'", f),
                                "define it with fn::name(r) { ... }".to_string(),
                            )
                        })?;
                        if params.len() != 1 {
                            return Err(ClangError::new(
                                line,
                                format!("pmap needs a one input function, '{}' takes {}", f, params.len()),
                                "write fn::name(r) { ... }".to_string(),
                            ));
                        }
                        let funcs = self.funcs.clone();
                        let mut handles = Vec::new();
                        for item in v {
                            let fc = funcs.clone();
                            let ps = params.clone();
                            let bd = body.clone();
                            handles.push(std::thread::spawn(move || {
                                let mut w = Interp::new();
                                w.funcs = fc;
                                w.scopes.push(HashMap::new());
                                w.set(&ps[0], item);
                                match w.exec_block(&bd) {
                                    Ok(Signal::None) => Value::Null,
                                    Ok(Signal::Return(r)) => r,
                                    Err(_) => Value::Null,
                                }
                            }));
                        }
                        let mut out = Vec::new();
                        for h in handles {
                            match h.join() {
                                Ok(val) => out.push(val),
                                Err(_) => {
                                    return Err(ClangError::new(
                                        line,
                                        "worker thread failed".to_string(),
                                        "keep mapped functions pure".to_string(),
                                    ))
                                }
                            }
                        }
                        Ok(Value::List(out))
                    }
                    _ => Err(ClangError::new(
                        line,
                        "pmap needs (list, fn name)".to_string(),
                        "call pmap([1, 2], \"dbl\")".to_string(),
                    )),
                }
            }
            "split" => {
                need_args(name, args, 2, line)?;
                match (self.eval(&args[0])?, self.eval(&args[1])?) {
                    (Value::Str(s), Value::Str(d)) => {
                        if d.is_empty() {
                            Ok(Value::List(s.chars().map(|c| Value::Str(c.to_string())).collect()))
                        } else {
                            Ok(Value::List(s.split(&d).map(|p| Value::Str(p.to_string())).collect()))
                        }
                    }
                    _ => Err(ClangError::new(
                        line,
                        "split needs (text, sep)".to_string(),
                        "call split(\"a,b\", \",\")".to_string(),
                    )),
                }
            }
            "join" => {
                need_args(name, args, 2, line)?;
                let base = self.eval(&args[0])?;
                let sep = self.eval(&args[1])?;
                match (base, sep) {
                    (Value::List(v), Value::Str(d)) => {
                        let mut parts = Vec::new();
                        for x in v {
                            parts.push(x.display());
                        }
                        Ok(Value::Str(parts.join(&d)))
                    }
                    _ => Err(ClangError::new(
                        line,
                        "join needs (list, sep)".to_string(),
                        "call join(parts, \",\")".to_string(),
                    )),
                }
            }
            "trim" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Str(s) => Ok(Value::Str(s.trim().to_string())),
                    other => Err(bad_type(line, "trim", "string", other.type_name())),
                }
            }
            "replace" => {
                need_args(name, args, 3, line)?;
                match (
                    self.eval(&args[0])?,
                    self.eval(&args[1])?,
                    self.eval(&args[2])?,
                ) {
                    (Value::Str(s), Value::Str(a), Value::Str(b)) => {
                        Ok(Value::Str(s.replace(&a, &b)))
                    }
                    _ => Err(ClangError::new(
                        line,
                        "replace needs (text, old, new)".to_string(),
                        "call replace(text, \"a\", \"b\")".to_string(),
                    )),
                }
            }
            "starts_with" => {
                need_args(name, args, 2, line)?;
                match (self.eval(&args[0])?, self.eval(&args[1])?) {
                    (Value::Str(s), Value::Str(p)) => Ok(Value::Bool(s.starts_with(&p))),
                    _ => Err(ClangError::new(
                        line,
                        "starts_with needs (text, prefix)".to_string(),
                        "call starts_with(text, \"hi\")".to_string(),
                    )),
                }
            }
            "ends_with" => {
                need_args(name, args, 2, line)?;
                match (self.eval(&args[0])?, self.eval(&args[1])?) {
                    (Value::Str(s), Value::Str(p)) => Ok(Value::Bool(s.ends_with(&p))),
                    _ => Err(ClangError::new(
                        line,
                        "ends_with needs (text, suffix)".to_string(),
                        "call ends_with(text, \"bye\")".to_string(),
                    )),
                }
            }
            "abs" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Num(n) => Ok(Value::Num(n.abs())),
                    other => Err(bad_type(line, "abs", "number", other.type_name())),
                }
            }
            "floor" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Num(n) => Ok(Value::Num(n.floor())),
                    other => Err(bad_type(line, "floor", "number", other.type_name())),
                }
            }
            "ceil" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Num(n) => Ok(Value::Num(n.ceil())),
                    other => Err(bad_type(line, "ceil", "number", other.type_name())),
                }
            }
            "sqrt" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Num(n) if n >= 0.0 => Ok(Value::Num(n.sqrt())),
                    Value::Num(_) => Err(ClangError::new(
                        line,
                        "sqrt needs zero or more".to_string(),
                        "guard negative inputs".to_string(),
                    )),
                    other => Err(bad_type(line, "sqrt", "number", other.type_name())),
                }
            }
            "pow" => {
                need_args(name, args, 2, line)?;
                match (self.eval(&args[0])?, self.eval(&args[1])?) {
                    (Value::Num(a), Value::Num(b)) => Ok(Value::Num(a.powf(b))),
                    _ => Err(ClangError::new(
                        line,
                        "pow needs (base, exp)".to_string(),
                        "call pow(2, 8)".to_string(),
                    )),
                }
            }
            "min" => {
                need_args(name, args, 2, line)?;
                match (self.eval(&args[0])?, self.eval(&args[1])?) {
                    (Value::Num(a), Value::Num(b)) => Ok(Value::Num(a.min(b))),
                    _ => Err(ClangError::new(
                        line,
                        "min needs two numbers".to_string(),
                        "call min(a, b)".to_string(),
                    )),
                }
            }
            "max" => {
                need_args(name, args, 2, line)?;
                match (self.eval(&args[0])?, self.eval(&args[1])?) {
                    (Value::Num(a), Value::Num(b)) => Ok(Value::Num(a.max(b))),
                    _ => Err(ClangError::new(
                        line,
                        "max needs two numbers".to_string(),
                        "call max(a, b)".to_string(),
                    )),
                }
            }
            "time_now" => {
                need_args(name, args, 0, line)?;
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as f64)
                    .unwrap_or(0.0);
                Ok(Value::Num(ms))
            }
            "sleep_ms" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Num(n) if n >= 0.0 && n <= 60000.0 => {
                        std::thread::sleep(std::time::Duration::from_millis(n as u64));
                        Ok(Value::Null)
                    }
                    _ => Err(ClangError::new(
                        line,
                        "sleep_ms needs 0 to 60000".to_string(),
                        "call sleep_ms(50)".to_string(),
                    )),
                }
            }
            "env" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Str(k) => Ok(Value::Str(
                        std::env::var(&k).unwrap_or_default(),
                    )),
                    other => Err(bad_type(line, "env", "string key", other.type_name())),
                }
            }
            "cli_args" => {
                need_args(name, args, 0, line)?;
                Ok(Value::List(
                    self.cli.iter().map(|s| Value::Str(s.clone())).collect(),
                ))
            }
            "exec" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Str(cmd) => {
                        let out = std::process::Command::new("sh")
                            .arg("-c")
                            .arg(&cmd)
                            .output()
                            .map_err(|e| {
                                ClangError::new(
                                    line,
                                    format!("exec failed: {}", e),
                                    "check the command text".to_string(),
                                )
                            })?;
                        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
                        if !out.status.success() {
                            let err = String::from_utf8_lossy(&out.stderr).to_string();
                            return Err(ClangError::new(
                                line,
                                format!("command exited badly: {}", err.trim()),
                                "run it in sh first".to_string(),
                            ));
                        }
                        while text.ends_with('\n') || text.ends_with('\r') {
                            text.pop();
                        }
                        Ok(Value::Str(text))
                    }
                    other => Err(bad_type(line, "exec", "string command", other.type_name())),
                }
            }
            "rand_seed" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Num(n) => {
                        self.rng = (n as u64) ^ 0x9e3779b97f4a7c15;
                        if self.rng == 0 {
                            self.rng = 0x12345678;
                        }
                        Ok(Value::Null)
                    }
                    other => Err(bad_type(line, "rand_seed", "number", other.type_name())),
                }
            }
            "rand_int" => {
                need_args(name, args, 2, line)?;
                match (self.eval(&args[0])?, self.eval(&args[1])?) {
                    (Value::Num(a), Value::Num(b)) => {
                        let lo = a as i64;
                        let hi = b as i64;
                        if lo > hi {
                            return Err(ClangError::new(
                                line,
                                "rand_int needs min <= max".to_string(),
                                "call rand_int(1, 10)".to_string(),
                            ));
                        }
                        let span = (hi - lo + 1) as u64;
                        let r = self.next_rand() % span;
                        Ok(Value::Num((lo as u64 + r) as f64))
                    }
                    _ => Err(ClangError::new(
                        line,
                        "rand_int needs (min, max)".to_string(),
                        "call rand_int(1, 10)".to_string(),
                    )),
                }
            }
            "is_str" => {
                need_args(name, args, 1, line)?;
                Ok(Value::Bool(matches!(self.eval(&args[0])?, Value::Str(_))))
            }
            "is_num" => {
                need_args(name, args, 1, line)?;
                Ok(Value::Bool(matches!(self.eval(&args[0])?, Value::Num(_))))
            }
            "is_list" => {
                need_args(name, args, 1, line)?;
                Ok(Value::Bool(matches!(self.eval(&args[0])?, Value::List(_))))
            }
            "is_map" => {
                need_args(name, args, 1, line)?;
                Ok(Value::Bool(matches!(self.eval(&args[0])?, Value::Map(_))))
            }
            "type_of" => {
                need_args(name, args, 1, line)?;
                Ok(Value::Str(self.eval(&args[0])?.type_name().to_string()))
            }
            "keys" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Map(m) => {
                        let mut ks: Vec<Value> =
                            m.keys().map(|k| Value::Str(k.clone())).collect();
                        ks.sort_by(|a, b| a.display().cmp(&b.display()));
                        Ok(Value::List(ks))
                    }
                    other => Err(bad_type(line, "keys", "map", other.type_name())),
                }
            }
            "tbl_put" => {
                need_args(name, args, 4, line)?;
                let path = self.eval(&args[0])?;
                let table = self.eval(&args[1])?;
                let id = self.eval(&args[2])?;
                let val = self.eval(&args[3])?;
                match (path, table, id) {
                    (Value::Str(p), Value::Str(tb), Value::Str(k)) => {
                        let mut m = load_db_map(&p, line)?;
                        let mut sub = match m.get(&tb) {
                            Some(Value::Map(s)) => s.clone(),
                            Some(_) => {
                                return Err(ClangError::new(
                                    line,
                                    format!("table '{}' is damaged", tb),
                                    "delete the db file to start fresh".to_string(),
                                ))
                            }
                            None => HashMap::new(),
                        };
                        sub.insert(k, val.clone());
                        m.insert(tb, Value::Map(sub));
                        save_db_map(&p, &m, line)?;
                        Ok(val)
                    }
                    _ => Err(ClangError::new(
                        line,
                        "tbl_put needs (db, table, id, value)".to_string(),
                        "call tbl_put(\"a.json\", \"users\", \"a\", x)".to_string(),
                    )),
                }
            }
            "tbl_get" => {
                need_args(name, args, 3, line)?;
                match (
                    self.eval(&args[0])?,
                    self.eval(&args[1])?,
                    self.eval(&args[2])?,
                ) {
                    (Value::Str(p), Value::Str(tb), Value::Str(k)) => {
                        let m = load_db_map(&p, line)?;
                        match m.get(&tb) {
                            Some(Value::Map(s)) => {
                                Ok(s.get(&k).cloned().unwrap_or(Value::Null))
                            }
                            _ => Ok(Value::Null),
                        }
                    }
                    _ => Err(ClangError::new(
                        line,
                        "tbl_get needs (db, table, id)".to_string(),
                        "call tbl_get(\"a.json\", \"users\", \"a\")".to_string(),
                    )),
                }
            }
            "tbl_list" => {
                need_args(name, args, 2, line)?;
                match (self.eval(&args[0])?, self.eval(&args[1])?) {
                    (Value::Str(p), Value::Str(tb)) => {
                        let m = load_db_map(&p, line)?;
                        match m.get(&tb) {
                            Some(Value::Map(s)) => {
                                let mut ks: Vec<Value> = s
                                    .keys()
                                    .map(|k| Value::Str(k.clone()))
                                    .collect();
                                ks.sort_by(|a, b| a.display().cmp(&b.display()));
                                Ok(Value::List(ks))
                            }
                            _ => Ok(Value::List(Vec::new())),
                        }
                    }
                    _ => Err(ClangError::new(
                        line,
                        "tbl_list needs (db, table)".to_string(),
                        "call tbl_list(\"a.json\", \"users\")".to_string(),
                    )),
                }
            }
            "tbl_del" => {
                need_args(name, args, 3, line)?;
                match (
                    self.eval(&args[0])?,
                    self.eval(&args[1])?,
                    self.eval(&args[2])?,
                ) {
                    (Value::Str(p), Value::Str(tb), Value::Str(k)) => {
                        let mut m = load_db_map(&p, line)?;
                        let removed = match m.get_mut(&tb) {
                            Some(Value::Map(s)) => s.remove(&k).is_some(),
                            _ => false,
                        };
                        save_db_map(&p, &m, line)?;
                        Ok(Value::Bool(removed))
                    }
                    _ => Err(ClangError::new(
                        line,
                        "tbl_del needs (db, table, id)".to_string(),
                        "call tbl_del(\"a.json\", \"users\", \"a\")".to_string(),
                    )),
                }
            }
            "http_serve" => {
                need_args(name, args, 3, line)?;
                let port = self.eval(&args[0])?;
                let handler = self.eval(&args[1])?;
                let max = self.eval(&args[2])?;
                match (port, handler, max) {
                    (Value::Num(p), Value::Str(h), Value::Num(n)) => {
                        let (params, body, _) = self.funcs.get(&h).cloned().ok_or_else(|| {
                            ClangError::new(
                                line,
                                format!("unknown function '{}'", h),
                                "define it with fn::name(r) { ... }".to_string(),
                            )
                        })?;
                        if params.len() != 1 {
                            return Err(ClangError::new(
                                line,
                                format!("handler '{}' must take one input", h),
                                "write fn::name(r) { ... }".to_string(),
                            ));
                        }
                        let count = n as i64;
                        if count < 1 || count > 100000 {
                            return Err(ClangError::new(
                                line,
                                "http_serve count must sit between 1 and 100000".to_string(),
                                "call http_serve(8080, \"h\", 10)".to_string(),
                            ));
                        }
                        let funcs = self.funcs.clone();
                        serve_blocking(p as u16, &h, params[0].clone(), body, funcs, count as usize, line)?;
                        Ok(Value::Null)
                    }
                    _ => Err(ClangError::new(
                        line,
                        "http_serve needs (port, handler, count)".to_string(),
                        "call http_serve(8080, \"h\", 10)".to_string(),
                    )),
                }
            }
            "pkg_fetch" => {
                if args.len() != 1 && args.len() != 2 {
                    return Err(ClangError::new(
                        line,
                        format!("pkg_fetch wants 1 or 2 inputs but got {}", args.len()),
                        "call pkg_fetch(url) or pkg_fetch(url, name)".to_string(),
                    ));
                }
                let url = self.eval(&args[0])?;
                let pname = if args.len() == 2 {
                    self.eval(&args[1])?
                } else {
                    Value::Null
                };
                match (url, pname) {
                    (Value::Str(u), Value::Null) => {
                        let base = u
                            .trim_end_matches('/')
                            .rsplit('/')
                            .next()
                            .unwrap_or("pkg")
                            .trim_end_matches(".git");
                        pkg_clone(&u, base, line)
                    }
                    (Value::Str(u), Value::Str(n)) => pkg_clone(&u, &n, line),
                    _ => Err(ClangError::new(
                        line,
                        "pkg_fetch needs (url) or (url, name)".to_string(),
                        "call pkg_fetch(\"https://...\", \"mylib\")".to_string(),
                    )),
                }
            }
            "assert" => {
                if args.len() != 1 && args.len() != 2 {
                    return Err(ClangError::new(
                        line,
                        format!("assert wants 1 or 2 inputs but got {}", args.len()),
                        "call assert(ok) or assert(ok, \"why\")".to_string(),
                    ));
                }
                let cond = self.eval(&args[0])?;
                if !cond.is_truthy() {
                    let msg = if args.len() == 2 {
                        self.eval(&args[1])?.display()
                    } else {
                        "check failed".to_string()
                    };
                    return Err(ClangError::new(line, format!("assert failed: {}", msg), "inspect the values above".to_string()));
                }
                Ok(Value::Null)
            }
            "str" => {
                need_args(name, args, 1, line)?;
                Ok(Value::Str(self.eval(&args[0])?.display()))
            }
            "num" => {
                need_args(name, args, 1, line)?;
                match self.eval(&args[0])? {
                    Value::Num(n) => Ok(Value::Num(n)),
                    Value::Str(s) => s.trim().parse::<f64>().map(Value::Num).map_err(|_| {
                        ClangError::new(
                            line,
                            format!("cannot turn '{}' into a number", s),
                            "pass digits like \"12\"".to_string(),
                        )
                    }),
                    Value::Bool(b) => Ok(Value::Num(if b { 1.0 } else { 0.0 })),
                    other => Err(ClangError::new(
                        line,
                        format!("num cannot convert {}", other.type_name()),
                        "pass a string or number".to_string(),
                    )),
                }
            }
            _ => Err(ClangError::new(
                line,
                format!("unknown function '{}'", name),
                "define it with fn::name() { ... } or check spelling".to_string(),
            )),
        }
    }
}

fn parse_http_url(url: &str, line: usize) -> Result<(String, String), ClangError> {
    let rest = url.strip_prefix("http://").ok_or_else(|| {
        ClangError::new(
            line,
            format!("bad url '{}'", url),
            "use http://host/path".to_string(),
        )
    })?;
    match rest.find('/') {
        Some(i) => Ok((rest[..i].to_string(), rest[i..].to_string())),
        None => Ok((rest.to_string(), "/".to_string())),
    }
}

fn do_https(method: &str, url: &str, body: Option<&str>, line: usize) -> Result<Value, ClangError> {
    use std::io::Write;
    let check = std::process::Command::new("curl")
        .arg("--version")
        .output()
        .map_err(|_| {
            ClangError::new(
                line,
                "https needs curl on PATH".to_string(),
                "install curl or use plain http".to_string(),
            )
        })?;
    if !check.status.success() {
        return Err(ClangError::new(
            line,
            "https needs curl on PATH".to_string(),
            "install curl or use plain http".to_string(),
        ));
    }
    let mut cmd = std::process::Command::new("curl");
    cmd.arg("-sS")
        .arg("--max-time")
        .arg("10")
        .arg("-w")
        .arg("\n%{http_code}");
    if method == "POST" {
        cmd.arg("-X").arg("POST").arg("--data-binary").arg("@-");
    }
    cmd.arg(url);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        ClangError::new(line, format!("curl failed to start: {}", e), "check curl".to_string())
    })?;
    if method == "POST" {
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(body.unwrap_or("").as_bytes());
        }
    }
    let out = child.wait_with_output().map_err(|e| {
        ClangError::new(line, format!("curl failed: {}", e), "check the url".to_string())
    })?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() && text.trim().is_empty() {
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        let mut m = HashMap::new();
        m.insert("code".to_string(), Value::Num(0.0));
        m.insert(
            "body".to_string(),
            Value::Str(format!("net down: {}", err.trim())),
        );
        return Ok(Value::Map(m));
    }
    let mut lines: Vec<&str> = text.lines().collect();
    let code_text = lines.pop().unwrap_or("0").trim().to_string();
    let code = code_text.parse::<f64>().unwrap_or(0.0);
    let mut resp_body = lines.join("\n");
    if resp_body.ends_with('\n') {
        resp_body.pop();
    }
    let mut m = HashMap::new();
    m.insert("code".to_string(), Value::Num(code));
    m.insert("body".to_string(), Value::Str(resp_body));
    Ok(Value::Map(m))
}

fn do_http(method: &str, url: &str, body: Option<&str>, line: usize) -> Result<Value, ClangError> {
    use std::io::{Read, Write};
    let (host_port, path) = parse_http_url(url, line)?;
    let host = host_port.split(':').next().unwrap_or(&host_port).to_string();
    let addrs: Vec<std::net::SocketAddr> = format!("{}", host_port)
        .parse()
        .map(|a| vec![a])
        .unwrap_or_else(|_| {
            use std::net::ToSocketAddrs;
            host_port
                .to_socket_addrs()
                .map(|it| it.collect())
                .unwrap_or_default()
        });
    if addrs.is_empty() {
        return Err(ClangError::new(
            line,
            format!("cannot resolve '{}'", host_port),
            "check the host and port".to_string(),
        ));
    }
    let mut stream = std::net::TcpStream::connect_timeout(
        &addrs[0],
        std::time::Duration::from_secs(5),
    )
    .map_err(|e| {
        ClangError::new(
            line,
            format!("cannot connect to '{}': {}", host_port, e),
            "is the server up".to_string(),
        )
    })?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    let req = if method == "POST" {
        let b = body.unwrap_or("");
        format!(
            "POST {} HTTP/1.0\r\nHost: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            path,
            host,
            b.len(),
            b
        )
    } else {
        format!(
            "GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n",
            path, host
        )
    };
    stream.write_all(req.as_bytes()).map_err(|e| {
        ClangError::new(line, format!("send failed: {}", e), "retry the request".to_string())
    })?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| {
        ClangError::new(line, format!("read failed: {}", e), "retry the request".to_string())
    })?;
    let text = String::from_utf8_lossy(&raw).to_string();
    let split = text.find("\r\n\r\n").map(|i| i + 4).or_else(|| text.find("\n\n").map(|i| i + 2));
    let (head, resp_body) = match split {
        Some(i) => (text[..i].to_string(), text[i..].to_string()),
        None => (text.clone(), String::new()),
    };
    let code = head
        .lines()
        .next()
        .unwrap_or("")
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<f64>().ok())
        .unwrap_or(0.0);
    let mut m = HashMap::new();
    m.insert("code".to_string(), Value::Num(code));
    m.insert("body".to_string(), Value::Str(resp_body));
    Ok(Value::Map(m))
}

fn load_db_map(path: &str, line: usize) -> Result<HashMap<String, Value>, ClangError> {
    let text = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => {
            return Err(ClangError::new(
                line,
                format!("cannot read db '{}': {}", path, e),
                "check the path".to_string(),
            ))
        }
    };
    let t = text.trim();
    if t.is_empty() {
        return Ok(HashMap::new());
    }
    match parse_json_value(t, line)? {
        Value::Map(m) => Ok(m),
        _ => Err(ClangError::new(
            line,
            "db file must hold an object".to_string(),
            "delete it to start fresh".to_string(),
        )),
    }
}

fn save_db_map(path: &str, m: &HashMap<String, Value>, line: usize) -> Result<(), ClangError> {
    let v = Value::Map(m.clone());
    std::fs::write(path, json_dump(&v)).map_err(|e| {
        ClangError::new(
            line,
            format!("cannot write db '{}': {}", path, e),
            "check the folder".to_string(),
        )
    })
}

fn serve_blocking(
    port: u16,
    _handler_name: &str,
    param: String,
    body_stmts: Vec<Stmt>,
    funcs: HashMap<String, (Vec<String>, Vec<Stmt>, usize)>,
    max_requests: usize,
    line: usize,
) -> Result<(), ClangError> {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(format!("127.0.0.1:{}", port)).map_err(|e| {
        ClangError::new(
            line,
            format!("cannot serve on port {}: {}", port, e),
            "pick a free port".to_string(),
        )
    })?;
    for _ in 0..max_requests {
        let (mut stream, _) = listener.accept().map_err(|e| {
            ClangError::new(line, format!("accept failed: {}", e), "retry the serve".to_string())
        })?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .ok();
        let mut raw = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    raw.extend_from_slice(&buf[..n]);
                    if raw.len() > 65536 {
                        break;
                    }
                    if let Some(pos) = find_header_end(&raw) {
                        let head = String::from_utf8_lossy(&raw[..pos]).to_string();
                        let want = content_want(&head);
                        if raw.len() >= pos + want {
                            break;
                        }
                    }
                    if raw.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let (method, path, req_body) = parse_http_request(&raw);
        let mut req = HashMap::new();
        req.insert("method".to_string(), Value::Str(method));
        req.insert("path".to_string(), Value::Str(path));
        req.insert("body".to_string(), Value::Str(req_body));
        let mut w = Interp::new();
        w.funcs = funcs.clone();
        w.scopes.push(HashMap::new());
        w.set(&param, Value::Map(req));
        let ret = match w.exec_block(&body_stmts) {
            Ok(Signal::None) => Value::Null,
            Ok(Signal::Return(v)) => v,
            Err(_) => Value::Str("handler failed".to_string()),
        };
        let (code, text) = match ret {
            Value::Str(s) => (200, s),
            Value::Map(m) => {
                let c = match m.get("code") {
                    Some(Value::Num(n)) => *n as u16,
                    _ => 200,
                };
                let b = match m.get("body") {
                    Some(Value::Str(s)) => s.clone(),
                    Some(v) => v.display(),
                    None => String::new(),
                };
                (c, b)
            }
            other => (200, other.display()),
        };
        let reason = match code {
            200 => "OK",
            201 => "Created",
            400 => "Bad Request",
            404 => "Not Found",
            500 => "Server Error",
            _ => "OK",
        };
        let resp = format!(
            "HTTP/1.0 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            code,
            reason,
            text.len(),
            text
        );
        let _ = stream.write_all(resp.as_bytes());
    }
    Ok(())
}

fn find_header_end(raw: &[u8]) -> Option<usize> {
    for i in 0..raw.len().saturating_sub(3) {
        if &raw[i..i + 4] == b"\r\n\r\n" {
            return Some(i + 4);
        }
    }
    None
}

fn content_want(head: &str) -> usize {
    for line in head.lines() {
        let low = line.to_lowercase();
        if low.starts_with("content-length:") {
            if let Some(v) = low.split(':').nth(1) {
                if let Ok(n) = v.trim().parse::<usize>() {
                    return n.min(65536);
                }
            }
        }
    }
    0
}

fn parse_http_request(raw: &[u8]) -> (String, String, String) {
    let text = String::from_utf8_lossy(raw).to_string();
    let head_end = text
        .find("\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| text.find("\n\n").map(|i| i + 2))
        .unwrap_or(text.len());
    let head = &text[..head_end.min(text.len())];
    let rest = if head_end <= text.len() {
        text[head_end..].to_string()
    } else {
        String::new()
    };
    let first = head.lines().next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    (method, path, rest)
}

fn pkg_clone(url: &str, name: &str, line: usize) -> Result<Value, ClangError> {
    if name.is_empty() || name.contains('/') || name.contains("..") {
        return Err(ClangError::new(
            line,
            format!("bad package name '{}'", name),
            "use a plain folder name".to_string(),
        ));
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let dest = format!("{}/.clng/pkgs/{}", home, name);
    if std::path::Path::new(&dest).exists() {
        return Ok(Value::Str(dest));
    }
    if let Some(parent) = std::path::Path::new(&dest).parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ClangError::new(line, format!("cache setup failed: {}", e), "check HOME".to_string())
        })?;
    }
    let out = std::process::Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(url)
        .arg(&dest)
        .output()
        .map_err(|_| {
            ClangError::new(
                line,
                "pkg_fetch needs git on PATH".to_string(),
                "install git or vendor the files".to_string(),
            )
        })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(ClangError::new(
            line,
            format!("git clone failed: {}", first_line(&err)),
            "check the url".to_string(),
        ));
    }
    Ok(Value::Str(dest))
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("unknown git error").trim().chars().take(160).collect()
}

fn stmt_kind(s: &Stmt) -> &'static str {
    match s {
        Stmt::VarDecl(..) => "var",
        Stmt::Assign(..) => "assign",
        Stmt::ExprStmt(..) => "expr",
        Stmt::Return(..) => "return",
        Stmt::Run(..) => "run",
        Stmt::If(..) => "if",
        Stmt::For(..) => "for",
        Stmt::FuncDef(..) => "def",
    }
}

fn stmt_line(s: &Stmt) -> usize {
    match s {
        Stmt::VarDecl(_, _, l)
        | Stmt::Assign(_, _, l)
        | Stmt::ExprStmt(_, l)
        | Stmt::Return(_, l)
        | Stmt::Run(_, l)
        | Stmt::If(_, _, _, l)
        | Stmt::For(_, _, _, l)
        | Stmt::FuncDef(_, _, _, l) => *l,
    }
}

fn need_args(name: &str, args: &[Expr], want: usize, line: usize) -> Result<(), ClangError> {
    if args.len() != want {
        return Err(ClangError::new(
            line,
            format!("{} wants {} inputs but got {}", name, want, args.len()),
            format!("check the call near line {}", line),
        ));
    }
    Ok(())
}

fn bad_type(line: usize, func: &str, want: &str, got: &str) -> ClangError {
    ClangError::new(
        line,
        format!("{} needs {}, got {}", func, want, got),
        format!("check the call to {}", func),
    )
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Num(x), Value::Num(y)) => x == y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::List(x), Value::List(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| values_equal(p, q))
        }
        (Value::Map(x), Value::Map(y)) => {
            x.len() == y.len()
                && x.iter().all(|(k, v)| y.get(k).map(|w| values_equal(v, w)).unwrap_or(false))
        }
        _ => false,
    }
}

fn compare_values(a: &Value, b: &Value) -> Option<i32> {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => {
            if x < y {
                Some(-1)
            } else if x > y {
                Some(1)
            } else {
                Some(0)
            }
        }
        (Value::Str(x), Value::Str(y)) => Some(match x.cmp(y) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }),
        (Value::Bool(x), Value::Bool(y)) => Some(match x.cmp(y) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }),
        _ => None,
    }
}

fn field_for_sort(v: &Value, key: &str) -> Value {
    match v {
        Value::Map(m) => m.get(key).cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn json_dump(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Num(n) => n.to_string(),
        Value::Str(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Value::List(items) => {
            let parts: Vec<String> = items.iter().map(json_dump).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Map(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|k| format!("\"{}\": {}", k, json_dump(&m[*k])))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn parse_json_value(s: &str, line: usize) -> Result<Value, ClangError> {
    let chars: Vec<char> = s.chars().collect();
    let mut pos = 0;
    skip_json_space(&chars, &mut pos);
    let v = parse_json_inner(&chars, &mut pos, line)?;
    skip_json_space(&chars, &mut pos);
    if pos != chars.len() {
        return Err(ClangError::new(
            line,
            "json has trailing text".to_string(),
            "check quotes and commas in the json".to_string(),
        ));
    }
    Ok(v)
}

fn skip_json_space(chars: &[char], pos: &mut usize) {
    while *pos < chars.len() && chars[*pos].is_whitespace() {
        *pos += 1;
    }
}

fn parse_json_inner(
    chars: &[char],
    pos: &mut usize,
    line: usize,
) -> Result<Value, ClangError> {
    skip_json_space(chars, pos);
    if *pos >= chars.len() {
        return Err(ClangError::new(
            line,
            "json ends early".to_string(),
            "check the saved file content".to_string(),
        ));
    }
    match chars[*pos] {
        '"' => Ok(Value::Str(parse_json_string(chars, pos, line)?)),
        '[' => {
            *pos += 1;
            let mut out = Vec::new();
            loop {
                skip_json_space(chars, pos);
                if *pos < chars.len() && chars[*pos] == ']' {
                    *pos += 1;
                    break;
                }
                let v = parse_json_inner(chars, pos, line)?;
                out.push(v);
                skip_json_space(chars, pos);
                if *pos < chars.len() && chars[*pos] == ',' {
                    *pos += 1;
                    continue;
                }
                skip_json_space(chars, pos);
                if *pos < chars.len() && chars[*pos] == ']' {
                    *pos += 1;
                    break;
                }
                return Err(ClangError::new(
                    line,
                    "bad json list".to_string(),
                    "separate items with commas".to_string(),
                ));
            }
            Ok(Value::List(out))
        }
        '{' => {
            *pos += 1;
            let mut m = HashMap::new();
            loop {
                skip_json_space(chars, pos);
                if *pos < chars.len() && chars[*pos] == '}' {
                    *pos += 1;
                    break;
                }
                if *pos >= chars.len() || chars[*pos] != '"' {
                    return Err(ClangError::new(
                        line,
                        "bad json object".to_string(),
                        "keys must be quoted".to_string(),
                    ));
                }
                let k = parse_json_string(chars, pos, line)?;
                skip_json_space(chars, pos);
                if *pos >= chars.len() || chars[*pos] != ':' {
                    return Err(ClangError::new(
                        line,
                        "bad json object".to_string(),
                        "write \"key\": value".to_string(),
                    ));
                }
                *pos += 1;
                let v = parse_json_inner(chars, pos, line)?;
                m.insert(k, v);
                skip_json_space(chars, pos);
                if *pos < chars.len() && chars[*pos] == ',' {
                    *pos += 1;
                    continue;
                }
                skip_json_space(chars, pos);
                if *pos < chars.len() && chars[*pos] == '}' {
                    *pos += 1;
                    break;
                }
                return Err(ClangError::new(
                    line,
                    "bad json object".to_string(),
                    "separate entries with commas".to_string(),
                ));
            }
            Ok(Value::Map(m))
        }
        c if c.is_ascii_digit() || c == '-' => {
            let start = *pos;
            if chars[*pos] == '-' {
                *pos += 1;
            }
            while *pos < chars.len() && (chars[*pos].is_ascii_digit() || chars[*pos] == '.') {
                *pos += 1;
            }
            let text: String = chars[start..*pos].iter().collect();
            text.parse::<f64>().map(Value::Num).map_err(|_| {
                ClangError::new(line, format!("bad json number '{}'", text), "check digits".to_string())
            })
        }
        _ => {
            let rest: String = chars[*pos..].iter().collect();
            if rest.starts_with("true") {
                *pos += 4;
                Ok(Value::Bool(true))
            } else if rest.starts_with("false") {
                *pos += 5;
                Ok(Value::Bool(false))
            } else if rest.starts_with("null") {
                *pos += 4;
                Ok(Value::Null)
            } else {
                Err(ClangError::new(
                    line,
                    "bad json value".to_string(),
                    "expect a string, number, list, or object".to_string(),
                ))
            }
        }
    }
}

fn parse_json_string(
    chars: &[char],
    pos: &mut usize,
    line: usize,
) -> Result<String, ClangError> {
    assert_eq!(chars[*pos], '"');
    *pos += 1;
    let mut out = String::new();
    while *pos < chars.len() {
        let c = chars[*pos];
        *pos += 1;
        if c == '"' {
            return Ok(out);
        }
        if c == '\\' {
            if *pos >= chars.len() {
                break;
            }
            let e = chars[*pos];
            *pos += 1;
            match e {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(c);
        }
    }
    Err(ClangError::new(
        line,
        "unterminated json string".to_string(),
        "close the quote".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::Interp;

    fn run_ok(src: &str) -> Vec<String> {
        let mut ip = Interp::new();
        ip.run_source(src).expect("program should run");
        ip.printed.clone()
    }

    #[test]
    fn hello_caps_first() {
        let out = run_ok("var:x = \"world\"\nemit(cap_first(x))\n");
        assert_eq!(out, vec!["World"]);
    }

    #[test]
    fn fn_single_and_multi_define() {
        let src = "fn::one(r) {\n return(r)\n}\nfn::add[a, b] {\n var:o = a + b\n return(o)\n}\nvar:x = one(7)\nvar:y = add(2, 3)\nemit(f\"{x} {y}\")\n";
        let out = run_ok(src);
        assert_eq!(out, vec!["7 5"]);
    }

    #[test]
    fn return_and_branch() {
        let src = "fn::div[a, b] {\n if(b == 0) {\n emit(\"bad\")\n return(0)\n } else {\n return(a / b)\n }\n}\nemit(div(20, 4))\nemit(div(1, 0))\n";
        let out = run_ok(src);
        assert_eq!(out, vec!["5", "bad", "0"]);
    }

    #[test]
    fn test_runner_reports_pass_and_fail() {
        let src = "fn::test_ok() {\n emit(\"fine\")\n}\nfn::test_bad() {\n emit(missing)\n}\n";
        let mut ip = Interp::new();
        let res = ip.run_test_fns(src).expect("runner runs");
        assert_eq!(res.len(), 2);
        assert!(res[0].0 == "test_bad" || res[0].0 == "test_ok");
        let bad = res.iter().find(|(n, _)| n == "test_bad").unwrap();
        let ok = res.iter().find(|(n, _)| n == "test_ok").unwrap();
        assert!(bad.1.is_err());
        assert!(ok.1.is_ok());
    }

    #[test]
    fn pkg_fetch_clones_local_git() {
        let base = format!("/tmp/clang_pkg_src_{}", std::process::id());
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("mkdir");
        assert!(std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&base)
            .status()
            .expect("git")
            .success());
        std::fs::write(format!("{}/note.txt", base), "hi").expect("write");
        let st = std::process::Command::new("git")
            .args(["-C", &base, "add", "."])
            .status()
            .expect("add");
        assert!(st.success());
        let cm = std::process::Command::new("git")
            .args([
                "-C",
                &base,
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "seed",
            ])
            .status()
            .expect("commit");
        assert!(cm.success());
        let url = format!("file://{}", base);
        let name = format!("clangt{}", std::process::id());
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let dest = format!("{}/.clng/pkgs/{}", home, name);
        let _ = std::fs::remove_dir_all(&dest);
        let src = format!("var:d = pkg_fetch(\"{}\", \"{}\")\nemit(d)\n", url, name);
        let mut ip = Interp::new();
        ip.run_source(&src).expect("fetch runs");
        assert!(std::path::Path::new(&format!("{}/note.txt", dest)).exists());
        assert_eq!(ip.printed, vec![dest.clone()]);
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn unknown_name_is_clear_error() {
        let mut ip = Interp::new();
        let err = ip.run_source("emit(missing)\n").unwrap_err();
        assert!(err.to_string().contains("unknown name"));
    }

    #[test]
    fn f_and_r_strings() {
        let src = "var:n = \"world\"\nemit(f\"hi {n}\")\nemit(r\"C:\\tmp\")\n";
        let out = run_ok(src);
        assert_eq!(out[0], "hi world");
        assert_eq!(out[1], "C:\\tmp");
    }

    #[test]
    fn split_join_round_trip() {
        let src = "var:parts = split(\"a,b,c\", \",\")\nvar:s = join(parts, \",\")\nemit(s)\nemit(len(parts))\n";
        let out = run_ok(src);
        assert_eq!(out, vec!["a,b,c", "3"]);
    }

    #[test]
    fn strings_and_math_extras() {
        let src = "emit(trim(\"  hi \"))\nemit(replace(\"aaa\", \"a\", \"b\"))\nemit(starts_with(\"hello\", \"he\"))\nemit(sqrt(16))\nemit(pow(2, 8))\nemit(min(3, 9))\n";
        let out = run_ok(src);
        assert_eq!(out, vec!["hi", "bbb", "true", "4", "256", "3"]);
    }

    #[test]
    fn system_seed_and_types() {
        let src = "rand_seed(7)\nvar:a = rand_int(1, 10)\nrand_seed(7)\nvar:b = rand_int(1, 10)\nemit(a == b)\nemit(type_of(a))\nemit(is_num(a))\nemit(is_str(\"x\"))\n";
        let out = run_ok(src);
        assert_eq!(out, vec!["true", "num", "true", "true"]);
    }

    #[test]
    fn http_get_local_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                use std::io::{Read, Write};
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let body = "hi";
                let resp = format!(
                    "HTTP/1.0 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
            }
        });
        let src = format!(
            "var:r = http_get(\"http://127.0.0.1:{}/x\")\nemit(r.code)\nemit(r.body)\n",
            port
        );
        let out = run_ok(&src);
        let _ = handle.join();
        assert_eq!(out, vec!["200", "hi"]);
    }

    #[test]
    fn db_put_get_round_trip() {
        let path = format!("/tmp/clang_test_db_{}.json", std::process::id());
        let _ = std::fs::remove_file(&path);
        let src = format!(
            "db_put(\"{}\", \"k\", \"v\")\nvar:g = db_get(\"{}\", \"k\")\nemit(g)\nemit(len(db_list(\"{}\")))\n",
            path, path, path
        );
        let out = run_ok(&src);
        let _ = std::fs::remove_file(&path);
        assert_eq!(out, vec!["v", "1"]);
    }

    #[test]
    fn pmap_keeps_order() {
        let src = "fn::dbl(r) {\n var:o = r * 2\n return(o)\n}\nvar:out = pmap([1, 2, 3], \"dbl\")\nemit(out[0])\nemit(out[2])\n";
        let out = run_ok(src);
        assert_eq!(out, vec!["2", "6"]);
    }
}

#[cfg(test)]
mod native_red {
    #[test]
    fn native_transpiles_hello() {
        let src = "var:x = \"world\"\nemit(cap_first(x))\n";
        let out = crate::codegen::transpile(src).expect("native should build hello");
        assert!(out.contains("cap_first"));
        assert!(out.contains("fn main"));
    }

    #[test]
    fn native_rejects_io_with_names() {
        let src = "var:x = file_read(\"a\")\nemit(x)\n";
        let err = crate::codegen::transpile(src).unwrap_err();
        assert!(err.to_string().contains("file_read"));
    }
}

#[cfg(test)]
mod stack5 {
    use super::Interp;

    fn run_ok(src: &str) -> Vec<String> {
        let mut ip = Interp::new();
        ip.run_source(src).expect("program should run");
        ip.printed.clone()
    }

    #[test]
    fn tbl_round_trip() {
        let path = format!("/tmp/clang_tbl_{}.json", std::process::id());
        let _ = std::fs::remove_file(&path);
        let src = format!(
            "tbl_put(\"{}\", \"users\", \"a\", \"Ann\")\nemit(tbl_get(\"{}\", \"users\", \"a\"))\nemit(len(tbl_list(\"{}\", \"users\")))\n",
            path, path, path
        );
        let out = run_ok(&src);
        let _ = std::fs::remove_file(&path);
        assert_eq!(out, vec!["Ann", "1"]);
    }

    #[test]
    fn serve_one_request() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);
        let src = format!(
            "fn::h(r) {{\n return(f\"hi {{r.path}}\")\n}}\nvar:res = http_serve({}, \"h\", 1)\nemit(\"served\")\n",
            port
        );
        let handle = std::thread::spawn(move || {
            let mut ip = Interp::new();
            ip.run_source(&src).expect("serve runs");
            ip.printed.clone()
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        let body = std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).map(|mut s| {
            use std::io::{Read, Write};
            let req = "GET /yo HTTP/1.0\r\nHost: x\r\nConnection: close\r\n\r\n";
            s.write_all(req.as_bytes()).ok();
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).ok();
            String::from_utf8_lossy(&buf).to_string()
        });
        let out = handle.join().expect("server done");
        let text = body.unwrap_or_default();
        assert!(text.contains("hi /yo"), "got: {}", text);
        assert_eq!(out, vec!["served"]);
    }
}

#[cfg(test)]
mod stack7_red {
    use super::Interp;

    #[test]
    fn quiet_records_without_printing() {
        let mut ip = Interp::new();
        ip.quiet = true;
        ip.run_source("emit(\"shh\")\n").expect("runs");
        assert_eq!(ip.printed, vec!["shh"]);
    }

    #[test]
    fn trace_collects_stmt_lines() {
        let mut ip = Interp::new();
        ip.trace = true;
        ip.run_source("var:x = 1\nemit(x)\n").expect("runs");
        assert!(ip.trace_log.len() >= 2, "got {:?}", ip.trace_log);
        assert!(ip.trace_log.iter().all(|l| l.contains(':')));
    }
}
