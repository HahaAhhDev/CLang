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
}

impl Interp {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            funcs: HashMap::new(),
            printed: Vec::new(),
        }
    }

    pub fn run_source(&mut self, src: &str) -> Result<(), ClangError> {
        let mut p = Parser::new(src);
        let prog = p.parse_program()?;
        self.run_program(&prog)
    }

    fn run_program(&mut self, prog: &[Stmt]) -> Result<(), ClangError> {
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
                println!("{}", text);
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
                        eprintln!("note: v1 http_post is a local stub, real net lands in v2 ({})", u);
                        let mut m = HashMap::new();
                        m.insert("code".to_string(), Value::Num(200.0));
                        m.insert("body".to_string(), Value::Str(b));
                        let _ = u;
                        Ok(Value::Map(m))
                    }
                    _ => Err(ClangError::new(
                        line,
                        "http_post needs (url, body)".to_string(),
                        "call http_post(url, text)".to_string(),
                    )),
                }
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
}
