use crate::error::ClangError;
use crate::parser::{Expr, Op as AOp, Parser, Stmt};
use crate::value::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub enum Op {
    SetLine(usize),
    Push(Value),
    Load(String),
    Store(String),
    StoreGlobal(String),
    EnterScope,
    ExitScope,
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Neg,
    BuildList(usize),
    ToStr,
    JoinStr(usize),
    IterList,
    ListLen,
    Index,
    Slice { has_start: bool, has_end: bool },
    Attr(String),
    Jump(usize),
    JumpIfFalse(usize),
    CallFn(usize, usize),
    CallBuiltin(String, usize),
    PushToVar(String),
    Return,
    Pop,
    BadReturn,
}

#[derive(Debug, Clone)]
pub struct BCFunc {
    pub name: String,
    pub params: Vec<String>,
    pub code: Vec<Op>,
}

#[derive(Debug)]
pub enum FastError {
    Unsupported(String),
    Clang(ClangError),
}

impl From<ClangError> for FastError {
    fn from(e: ClangError) -> Self {
        FastError::Clang(e)
    }
}

const PURE_BUILTINS: &[&str] = &[
    "emit",
    "cap_first",
    "lower",
    "upper",
    "contains",
    "len",
    "split",
    "join",
    "trim",
    "replace",
    "starts_with",
    "ends_with",
    "abs",
    "floor",
    "ceil",
    "sqrt",
    "pow",
    "min",
    "max",
    "str",
    "num",
    "empty_list",
    "assert",
    "rand_seed",
    "rand_int",
    "is_str",
    "is_num",
    "is_list",
    "is_map",
    "type_of",
    "keys",
];

pub fn try_run(prog: &[Stmt], cli: Vec<String>) -> Result<Vec<String>, FastError> {
    try_run_opts(prog, cli, false, false).map(|r| r.0)
}

pub fn try_run_opts(
    prog: &[Stmt],
    cli: Vec<String>,
    trace: bool,
    quiet: bool,
) -> Result<(Vec<String>, Vec<String>), FastError> {
    let unit = compile_prog(prog)?;
    let (printed, log) = exec_unit(&unit, cli, trace, quiet)?;
    Ok((printed, log))
}

struct Unit {
    funcs: Vec<BCFunc>,
    main: usize,
}

fn compile_prog(prog: &[Stmt]) -> Result<Unit, FastError> {
    let mut idx: HashMap<String, usize> = HashMap::new();
    let mut defs: Vec<(&String, &Vec<String>, &Vec<Stmt>)> = Vec::new();
    for s in prog {
        if let Stmt::FuncDef(n, ps, b, _) = s {
            if idx.contains_key(n) {
                return Err(FastError::Clang(ClangError::new(
                    1,
                    format!("function '{}' is defined twice", n),
                    "rename one of them".to_string(),
                )));
            }
            idx.insert(n.clone(), defs.len());
            defs.push((n, ps, b));
        }
    }
    let mut globals: HashSet<String> = HashSet::new();
    for s in prog {
        if let Stmt::VarDecl(n, _, _) = s {
            globals.insert(n.clone());
        }
    }
    let mut funcs: Vec<BCFunc> = Vec::new();
    for (n, ps, body) in defs {
        let mut cx = Cx {
            idx: &idx,
            globals: &globals,
            loops: 0,
        };
        let code = cx.compile_body(body, true)?;
        funcs.push(BCFunc {
            name: (*n).clone(),
            params: (*ps).clone(),
            code,
        });
    }
    let mut cx = Cx {
        idx: &idx,
        globals: &globals,
        loops: 0,
    };
    let mut main_code: Vec<Op> = Vec::new();
    for s in prog {
        if matches!(s, Stmt::FuncDef(..)) {
            continue;
        }
        cx.compile_stmt_main(s, &mut main_code)?;
    }
    main_code.push(Op::Push(Value::Null));
    main_code.push(Op::Return);
    let main = funcs.len();
    funcs.push(BCFunc {
        name: "main".to_string(),
        params: Vec::new(),
        code: main_code,
    });
    Ok(Unit { funcs, main })
}

struct Cx<'a> {
    idx: &'a HashMap<String, usize>,
    globals: &'a HashSet<String>,
    loops: usize,
}

impl Cx<'_> {
    fn unsupported(&self, what: String) -> FastError {
        FastError::Unsupported(what)
    }

    fn compile_body(&mut self, body: &[Stmt], is_fn: bool) -> Result<Vec<Op>, FastError> {
        let mut code = Vec::new();
        for s in body {
            self.compile_stmt(s, &mut code, is_fn)?;
        }
        code.push(Op::Push(Value::Null));
        code.push(Op::Return);
        Ok(code)
    }

    fn compile_stmt(
        &mut self,
        s: &Stmt,
        code: &mut Vec<Op>,
        is_fn: bool,
    ) -> Result<(), FastError> {
        self.compile_stmt_mode(s, code, is_fn, false)
    }

    fn compile_stmt_mode(
        &mut self,
        s: &Stmt,
        code: &mut Vec<Op>,
        is_fn: bool,
        is_main: bool,
    ) -> Result<(), FastError> {
        match s {
            Stmt::VarDecl(_, e, line) | Stmt::Assign(_, e, line) => {
                let name = match s {
                    Stmt::VarDecl(n, _, _) | Stmt::Assign(n, _, _) => n.clone(),
                    _ => unreachable!(),
                };
                code.push(Op::SetLine(*line));
                self.compile_expr(e, code)?;
                if is_main && self.globals.contains(&name) {
                    code.push(Op::StoreGlobal(name));
                } else {
                    code.push(Op::Store(name));
                }
            }
            Stmt::ExprStmt(e, line) => {
                code.push(Op::SetLine(*line));
                self.compile_expr(e, code)?;
                code.push(Op::Pop);
            }
            Stmt::Return(e, line) => {
                if !is_fn {
                    code.push(Op::BadReturn);
                    return Ok(());
                }
                code.push(Op::SetLine(*line));
                self.compile_expr(e, code)?;
                code.push(Op::Return);
            }
            Stmt::Run(n, line) => {
                code.push(Op::SetLine(*line));
                match self.idx.get(n) {
                    Some(i) => {
                        code.push(Op::CallFn(*i, 0));
                        code.push(Op::Pop);
                    }
                    None => {
                        let _ = line;
                        return Err(self.unsupported(format!("call {}", n)));
                    }
                }
            }
            Stmt::If(c, a, b, line) => {
                code.push(Op::SetLine(*line));
                self.compile_expr(c, code)?;
                let jfalse = code.len();
                code.push(Op::JumpIfFalse(0));
                code.push(Op::EnterScope);
                for x in a {
                    self.compile_stmt_mode(x, code, is_fn, is_main)?;
                }
                code.push(Op::ExitScope);
                if b.is_empty() {
                    let end = code.len();
                    code[jfalse] = Op::JumpIfFalse(end);
                } else {
                    let jend = code.len();
                    code.push(Op::Jump(0));
                    let els = code.len();
                    code[jfalse] = Op::JumpIfFalse(els);
                    code.push(Op::EnterScope);
                    for x in b {
                        self.compile_stmt_mode(x, code, is_fn, is_main)?;
                    }
                    code.push(Op::ExitScope);
                    let end = code.len();
                    code[jend] = Op::Jump(end);
                }
            }
            Stmt::For(v, it, body, line) => {
                let id = self.loops;
                self.loops += 1;
                let vs = format!("#vs{}", id);
                let nn = format!("#n{}", id);
                let ii = format!("#i{}", id);
                code.push(Op::SetLine(*line));
                self.compile_expr(it, code)?;
                code.push(Op::IterList);
                code.push(Op::Store(vs.clone()));
                code.push(Op::Load(vs.clone()));
                code.push(Op::ListLen);
                code.push(Op::Store(nn.clone()));
                code.push(Op::Push(Value::Num(0.0)));
                code.push(Op::Store(ii.clone()));
                let loop_top = code.len();
                code.push(Op::Load(ii.clone()));
                code.push(Op::Load(nn.clone()));
                code.push(Op::Lt);
                let jend = code.len();
                code.push(Op::JumpIfFalse(0));
                code.push(Op::EnterScope);
                code.push(Op::Load(vs));
                code.push(Op::Load(ii.clone()));
                code.push(Op::Index);
                code.push(Op::Store(v.clone()));
                for x in body {
                    self.compile_stmt_mode(x, code, is_fn, is_main)?;
                }
                code.push(Op::ExitScope);
                code.push(Op::Load(ii.clone()));
                code.push(Op::Push(Value::Num(1.0)));
                code.push(Op::Add);
                code.push(Op::Store(ii));
                code.push(Op::Jump(loop_top));
                let end = code.len();
                code[jend] = Op::JumpIfFalse(end);
            }
            Stmt::FuncDef(..) => {}
        }
        Ok(())
    }

    fn compile_stmt_main(&mut self, s: &Stmt, code: &mut Vec<Op>) -> Result<(), FastError> {
        match s {
            Stmt::Return(_, _) => {
                code.push(Op::BadReturn);
                Ok(())
            }
            _ => self.compile_stmt_mode(s, code, false, true),
        }
    }

    fn compile_expr(&mut self, e: &Expr, code: &mut Vec<Op>) -> Result<(), FastError> {
        match e {
            Expr::Str { val, is_f, .. } => {
                if *is_f {
                    self.compile_fstr(val, code)?;
                } else {
                    code.push(Op::Push(Value::Str(val.clone())));
                }
            }
            Expr::Num(n, _) => code.push(Op::Push(Value::Num(*n))),
            Expr::Bool(b, _) => code.push(Op::Push(Value::Bool(*b))),
            Expr::Null(_) => code.push(Op::Push(Value::Null)),
            Expr::List(items, _) => {
                for x in items {
                    self.compile_expr(x, code)?;
                }
                code.push(Op::BuildList(items.len()));
            }
            Expr::Var(n, _) => code.push(Op::Load(n.clone())),
            Expr::Call(n, args, line) => {
                if n == "push" && args.len() == 2 && matches!(args[0], Expr::Var(_, _)) {
                    let target = match &args[0] {
                        Expr::Var(t, _) => t.clone(),
                        _ => unreachable!(),
                    };
                    self.compile_expr(&args[1], code)?;
                    code.push(Op::PushToVar(target));
                    return Ok(());
                }
                if PURE_BUILTINS.contains(&n.as_str()) {
                    for a in args {
                        self.compile_expr(a, code)?;
                    }
                    code.push(Op::CallBuiltin(n.clone(), args.len()));
                    return Ok(());
                }
                match self.idx.get(n) {
                    Some(i) => {
                        for a in args {
                            self.compile_expr(a, code)?;
                        }
                        code.push(Op::CallFn(*i, args.len()));
                    }
                    None => {
                        let _ = line;
                        return Err(self.unsupported(format!("call {}", n)));
                    }
                }
            }
            Expr::Attr(o, f, _) => {
                self.compile_expr(o, code)?;
                code.push(Op::Attr(f.clone()));
            }
            Expr::Index(o, i, _) => {
                self.compile_expr(o, code)?;
                self.compile_expr(i, code)?;
                code.push(Op::Index);
            }
            Expr::Slice { obj, start, end, .. } => {
                self.compile_expr(obj, code)?;
                if let Some(s) = start {
                    self.compile_expr(s, code)?;
                }
                if let Some(x) = end {
                    self.compile_expr(x, code)?;
                }
                code.push(Op::Slice {
                    has_start: start.is_some(),
                    has_end: end.is_some(),
                });
            }
            Expr::BinOp(l, op, r, _) => {
                self.compile_expr(l, code)?;
                self.compile_expr(r, code)?;
                code.push(match op {
                    AOp::Add => Op::Add,
                    AOp::Sub => Op::Sub,
                    AOp::Mul => Op::Mul,
                    AOp::Div => Op::Div,
                    AOp::Eq => Op::Eq,
                    AOp::Ne => Op::Ne,
                    AOp::Lt => Op::Lt,
                    AOp::Gt => Op::Gt,
                    AOp::Le => Op::Le,
                    AOp::Ge => Op::Ge,
                });
            }
            Expr::Neg(x, _) => {
                self.compile_expr(x, code)?;
                code.push(Op::Neg);
            }
        }
        Ok(())
    }

    fn compile_fstr(&mut self, raw: &str, code: &mut Vec<Op>) -> Result<(), FastError> {
        let chars: Vec<char> = raw.chars().collect();
        let mut parts: usize = 0;
        let mut lit = String::new();
        let mut flush = |lit: &mut String, code: &mut Vec<Op>, parts: &mut usize| {
            if !lit.is_empty() {
                code.push(Op::Push(Value::Str(std::mem::take(lit))));
                code.push(Op::ToStr);
                *parts += 1;
            }
        };
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '{' {
                if i + 1 < chars.len() && chars[i + 1] == '{' {
                    lit.push('{');
                    i += 2;
                    continue;
                }
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '}' {
                    j += 1;
                }
                if j >= chars.len() {
                    return Err(FastError::Clang(ClangError::new(
                        1,
                        "f-string has '{' without '}'".to_string(),
                        "close it like f\"hi {name}\"".to_string(),
                    )));
                }
                let inner: String = chars[i + 1..j].iter().collect();
                let mut p = Parser::new(inner.trim());
                let e = p.parse_expr().map_err(FastError::Clang)?;
                flush(&mut lit, code, &mut parts);
                self.compile_expr(&e, code)?;
                code.push(Op::ToStr);
                parts += 1;
                i = j + 1;
            } else if chars[i] == '}' && i + 1 < chars.len() && chars[i + 1] == '}' {
                lit.push('}');
                i += 2;
            } else {
                lit.push(chars[i]);
                i += 1;
            }
        }
        flush(&mut lit, code, &mut parts);
        if parts == 0 {
            code.push(Op::Push(Value::Str(String::new())));
        } else {
            code.push(Op::JoinStr(parts));
        }
        Ok(())
    }
}

struct Frame {
    func: usize,
    ip: usize,
    base: usize,
    scopes: Vec<HashMap<String, Value>>,
    line: usize,
}

struct VM {
    funcs: Vec<BCFunc>,
    stack: Vec<Value>,
    frames: Vec<Frame>,
    globals: HashMap<String, Value>,
    printed: Vec<String>,
    trace_log: Vec<String>,
    trace: bool,
    quiet: bool,
    rng: u64,
    cli: Vec<String>,
}

fn exec_unit(
    unit: &Unit,
    cli: Vec<String>,
    trace: bool,
    quiet: bool,
) -> Result<(Vec<String>, Vec<String>), ClangError> {
    let mut vm = VM {
        funcs: unit.funcs.clone(),
        stack: Vec::new(),
        frames: vec![Frame {
            func: unit.main,
            ip: 0,
            base: 0,
            scopes: vec![HashMap::new()],
            line: 1,
        }],
        globals: HashMap::new(),
        printed: Vec::new(),
        trace_log: Vec::new(),
        trace,
        quiet,
        rng: 0x12345678,
        cli,
    };
    run_vm(&mut vm)?;
    Ok((vm.printed, vm.trace_log))
}

fn vm_err(vm: &VM, msg: String, hint: String) -> ClangError {
    let line = vm.frames.last().map(|f| f.line).unwrap_or(1);
    ClangError::new(line, msg, hint)
}

fn pop(vm: &mut VM) -> Result<Value, ClangError> {
    vm.stack.pop().ok_or_else(|| vm_err(vm, "stack went empty".to_string(), "report this bug".to_string()))
}

fn run_vm(vm: &mut VM) -> Result<(), ClangError> {
    loop {
        let (op, func) = {
            let fr = vm.frames.last().ok_or_else(|| {
                ClangError::new(1, "ran past the last frame".to_string(), "report this bug".to_string())
            })?;
            if fr.ip >= vm.funcs[fr.func].code.len() {
                return Err(vm_err(
                    vm,
                    "function fell off its end".to_string(),
                    "report this bug".to_string(),
                ));
            }
            (vm.funcs[fr.func].code[fr.ip].clone(), fr.func)
        };
        vm.frames.last_mut().unwrap().ip += 1;
        match op {
            Op::SetLine(n) => {
                vm.frames.last_mut().unwrap().line = n;
                if vm.trace && vm.trace_log.len() < 5000 {
                    vm.trace_log.push(format!("step:{}", n));
                }
            }
            Op::Push(v) => vm.stack.push(v),
            Op::Load(n) => {
                let fr = vm.frames.last().unwrap();
                let mut hit: Option<Value> = None;
                for scope in fr.scopes.iter().rev() {
                    if let Some(v) = scope.get(&n) {
                        hit = Some(v.clone());
                        break;
                    }
                }
                let v = match hit {
                    Some(x) => x,
                    None => vm.globals.get(&n).cloned().ok_or_else(|| {
                        vm_err(
                            vm,
                            format!("unknown name '{}'", n),
                            format!("declare it first with var:{} = ...", n),
                        )
                    })?,
                };
                vm.stack.push(v);
            }
            Op::Store(n) => {
                let v = pop(vm)?;
                let fr = vm.frames.last_mut().unwrap();
                let mut done = false;
                for scope in fr.scopes.iter_mut().rev() {
                    if scope.contains_key(&n) {
                        scope.insert(n.clone(), v.clone());
                        done = true;
                        break;
                    }
                }
                if done {
                } else if vm.globals.contains_key(&n) {
                    vm.globals.insert(n, v);
                } else {
                    vm.frames
                        .last_mut()
                        .unwrap()
                        .scopes
                        .last_mut()
                        .unwrap()
                        .insert(n, v);
                }
            }
            Op::StoreGlobal(n) => {
                let v = pop(vm)?;
                vm.globals.insert(n, v);
            }
            Op::EnterScope => vm.frames.last_mut().unwrap().scopes.push(HashMap::new()),
            Op::ExitScope => {
                vm.frames.last_mut().unwrap().scopes.pop();
            }
            Op::Add => {
                let b = pop(vm)?;
                let a = pop(vm)?;
                vm.stack.push(v_add(a, b)?);
            }
            Op::Sub => {
                let b = pop(vm)?;
                let a = pop(vm)?;
                match (a, b) {
                    (Value::Num(x), Value::Num(y)) => vm.stack.push(Value::Num(x - y)),
                    (x, y) => {
                        return Err(vm_err(
                            vm,
                            format!("cannot subtract {} and {}", x.type_name(), y.type_name()),
                            "use - only on numbers".to_string(),
                        ))
                    }
                }
            }
            Op::Mul => {
                let b = pop(vm)?;
                let a = pop(vm)?;
                match (a, b) {
                    (Value::Num(x), Value::Num(y)) => vm.stack.push(Value::Num(x * y)),
                    (Value::Str(s), Value::Num(n)) | (Value::Num(n), Value::Str(s)) => {
                        let k = (n as i64).max(0).min(10000) as usize;
                        vm.stack.push(Value::Str(s.repeat(k)));
                    }
                    (x, y) => {
                        return Err(vm_err(
                            vm,
                            format!("cannot multiply {} and {}", x.type_name(), y.type_name()),
                            "use * on numbers".to_string(),
                        ))
                    }
                }
            }
            Op::Div => {
                let b = pop(vm)?;
                let a = pop(vm)?;
                match (a, b) {
                    (Value::Num(_), Value::Num(y)) if y == 0.0 => {
                        return Err(vm_err(
                            vm,
                            "division by zero".to_string(),
                            "guard with if(b == 0) before dividing".to_string(),
                        ))
                    }
                    (Value::Num(x), Value::Num(y)) => vm.stack.push(Value::Num(x / y)),
                    (x, y) => {
                        return Err(vm_err(
                            vm,
                            format!("cannot divide {} and {}", x.type_name(), y.type_name()),
                            "use / only on numbers".to_string(),
                        ))
                    }
                }
            }
            Op::Eq => {
                let b = pop(vm)?;
                let a = pop(vm)?;
                vm.stack.push(Value::Bool(values_equal(&a, &b)));
            }
            Op::Ne => {
                let b = pop(vm)?;
                let a = pop(vm)?;
                vm.stack.push(Value::Bool(!values_equal(&a, &b)));
            }
            Op::Lt => {
                let b = pop(vm)?;
                let a = pop(vm)?;
                vm.stack.push(Value::Bool(v_lt(&a, &b)?));
            }
            Op::Gt => {
                let b = pop(vm)?;
                let a = pop(vm)?;
                vm.stack.push(Value::Bool(v_lt(&b, &a)?));
            }
            Op::Le => {
                let b = pop(vm)?;
                let a = pop(vm)?;
                vm.stack.push(Value::Bool(v_lt(&a, &b)? || values_equal(&a, &b)));
            }
            Op::Ge => {
                let b = pop(vm)?;
                let a = pop(vm)?;
                vm.stack.push(Value::Bool(v_lt(&b, &a)? || values_equal(&a, &b)));
            }
            Op::Neg => {
                let a = pop(vm)?;
                match a {
                    Value::Num(n) => vm.stack.push(Value::Num(-n)),
                    other => {
                        return Err(vm_err(
                            vm,
                            format!("cannot negate {}", other.type_name()),
                            "use - only on numbers".to_string(),
                        ))
                    }
                }
            }
            Op::BuildList(n) => {
                let base = vm.stack.len().checked_sub(n).ok_or_else(|| {
                    vm_err(vm, "stack went empty".to_string(), "report this bug".to_string())
                })?;
                let items = vm.stack.split_off(base);
                vm.stack.push(Value::List(items));
            }
            Op::ToStr => {
                let a = pop(vm)?;
                vm.stack.push(Value::Str(a.display()));
            }
            Op::JoinStr(n) => {
                let base = vm.stack.len().checked_sub(n).ok_or_else(|| {
                    vm_err(vm, "stack went empty".to_string(), "report this bug".to_string())
                })?;
                let items = vm.stack.split_off(base);
                let mut s = String::new();
                for x in items {
                    match x {
                        Value::Str(t) => s.push_str(&t),
                        other => s.push_str(&other.display()),
                    }
                }
                vm.stack.push(Value::Str(s));
            }
            Op::IterList => {
                let a = pop(vm)?;
                match a {
                    Value::List(v) => vm.stack.push(Value::List(v)),
                    Value::Str(s) => vm.stack.push(Value::List(
                        s.chars().map(|c| Value::Str(c.to_string())).collect(),
                    )),
                    other => {
                        return Err(vm_err(
                            vm,
                            format!("for loop needs a list, got {}", other.type_name()),
                            "pass a list like [1, 2]".to_string(),
                        ))
                    }
                }
            }
            Op::ListLen => {
                let a = pop(vm)?;
                match a {
                    Value::List(v) => vm.stack.push(Value::Num(v.len() as f64)),
                    _ => {
                        return Err(vm_err(
                            vm,
                            "loop list vanished".to_string(),
                            "report this bug".to_string(),
                        ))
                    }
                }
            }
            Op::Index => {
                let i = pop(vm)?;
                let o = pop(vm)?;
                vm.stack.push(v_index(o, i)?);
            }
            Op::Slice { has_start, has_end } => {
                let e = if has_end { Some(pop(vm)?) } else { None };
                let s = if has_start { Some(pop(vm)?) } else { None };
                let o = pop(vm)?;
                vm.stack.push(v_slice(o, s, e)?);
            }
            Op::Attr(f) => {
                let o = pop(vm)?;
                vm.stack.push(v_attr(o, &f)?);
            }
            Op::Jump(t) => vm.frames.last_mut().unwrap().ip = t,
            Op::JumpIfFalse(t) => {
                let c = pop(vm)?;
                if !c.is_truthy() {
                    vm.frames.last_mut().unwrap().ip = t;
                }
            }
            Op::CallFn(i, argc) => {
                let want = vm.funcs[i].params.len();
                if want != argc {
                    let name = vm.funcs[i].name.clone();
                    return Err(vm_err(
                        vm,
                        format!(
                            "function '{}' wants {} inputs but got {}",
                            name, want, argc
                        ),
                        format!("check the call near line {}", vm.frames.last().unwrap().line),
                    ));
                }
                let base = vm.stack.len().checked_sub(argc).ok_or_else(|| {
                    vm_err(vm, "stack went empty".to_string(), "report this bug".to_string())
                })?;
                let args = vm.stack.split_off(base);
                let mut scope = HashMap::new();
                for (k, v) in vm.funcs[i].params.clone().into_iter().zip(args) {
                    scope.insert(k, v);
                }
                vm.frames.push(Frame {
                    func: i,
                    ip: 0,
                    base,
                    scopes: vec![scope],
                    line: vm.frames.last().map(|f| f.line).unwrap_or(1),
                });
                let _ = func;
            }
            Op::CallBuiltin(n, argc) => {
                let base = vm.stack.len().checked_sub(argc).ok_or_else(|| {
                    vm_err(vm, "stack went empty".to_string(), "report this bug".to_string())
                })?;
                let args = vm.stack.split_off(base);
                let out = call_builtin(vm, &n, args)?;
                vm.stack.push(out);
            }
            Op::PushToVar(n) => {
                let item = pop(vm)?;
                let fr = vm.frames.last_mut().unwrap();
                let mut done = false;
                for scope in fr.scopes.iter_mut().rev() {
                    if let Some(Value::List(v)) = scope.get_mut(&n) {
                        v.push(item.clone());
                        done = true;
                        break;
                    }
                }
                if !done {
                    if let Some(Value::List(v)) = vm.globals.get_mut(&n) {
                        v.push(item);
                        done = true;
                    }
                }
                if !done {
                    return Err(vm_err(
                        vm,
                        format!("push needs a list holding '{}'", n),
                        "start it with var:name = []".to_string(),
                    ));
                }
                vm.stack.push(Value::Num(0.0));
            }
            Op::Return => {
                let v = pop(vm)?;
                let fr = vm.frames.pop().ok_or_else(|| {
                    ClangError::new(1, "ran past the last frame".to_string(), "report this bug".to_string())
                })?;
                vm.stack.truncate(fr.base);
                if vm.frames.is_empty() {
                    return Ok(());
                }
                vm.stack.push(v);
            }
            Op::Pop => {
                pop(vm)?;
            }
            Op::BadReturn => {
                return Err(vm_err(
                    vm,
                    "return outside a function".to_string(),
                    "move return inside fn::name { ... }".to_string(),
                ))
            }
        }
    }
}

fn v_add(a: Value, b: Value) -> Result<Value, ClangError> {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => Ok(Value::Num(x + y)),
        (Value::Str(x), Value::Str(y)) => Ok(Value::Str(x + &y)),
        (Value::Str(x), Value::Num(y)) => {
            Ok(Value::Str(format!("{}{}", x, Value::Num(y).display())))
        }
        (Value::Num(x), Value::Str(y)) => {
            Ok(Value::Str(format!("{}{}", Value::Num(x).display(), y)))
        }
        (Value::List(mut x), Value::List(mut y)) => {
            x.append(&mut y);
            Ok(Value::List(x))
        }
        (x, y) => Err(ClangError::new(
            1,
            format!("cannot add {} and {}", x.type_name(), y.type_name()),
            "add numbers, join strings, or concat lists".to_string(),
        )),
    }
}

fn v_lt(a: &Value, b: &Value) -> Result<bool, ClangError> {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => Ok(x < y),
        (Value::Str(x), Value::Str(y)) => Ok(x < y),
        (Value::Bool(x), Value::Bool(y)) => Ok(x < y),
        _ => Err(ClangError::new(
            1,
            format!("cannot compare {} and {}", a.type_name(), b.type_name()),
            "compare numbers or strings of the same kind".to_string(),
        )),
    }
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
                && x.iter()
                    .all(|(k, v)| y.get(k).map(|w| values_equal(v, w)).unwrap_or(false))
        }
        _ => false,
    }
}

fn v_index(o: Value, i: Value) -> Result<Value, ClangError> {
    match (o, i) {
        (Value::List(v), Value::Num(n)) => {
            let j = n as i64;
            let pos = if j < 0 { v.len() as i64 + j } else { j } as usize;
            v.get(pos).cloned().ok_or_else(|| {
                ClangError::new(
                    1,
                    format!("index {} is past the list", j),
                    "keep the index inside it".to_string(),
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
            chars
                .get(pos)
                .map(|c| Value::Str(c.to_string()))
                .ok_or_else(|| {
                    ClangError::new(
                        1,
                        format!("index {} is past the string", j),
                        "keep the index inside the text".to_string(),
                    )
                })
        }
        (Value::Map(m), Value::Str(k)) => m.get(&k).cloned().ok_or_else(|| {
            ClangError::new(1, format!("missing key '{}'", k), "check the map keys".to_string())
        }),
        (b, k) => Err(ClangError::new(
            1,
            format!("cannot index {} with {}", b.type_name(), k.type_name()),
            "use a number for lists and strings".to_string(),
        )),
    }
}

fn v_slice(o: Value, s: Option<Value>, e: Option<Value>) -> Result<Value, ClangError> {
    let len: i64 = match &o {
        Value::List(v) => v.len() as i64,
        Value::Str(x) => x.chars().count() as i64,
        other => {
            return Err(ClangError::new(
                1,
                format!("cannot slice {}", other.type_name()),
                "slice a list or a string".to_string(),
            ))
        }
    };
    let edge = |v: Option<Value>| -> Result<Option<i64>, ClangError> {
        match v {
            None => Ok(None),
            Some(Value::Num(n)) => Ok(Some(n as i64)),
            Some(other) => Err(ClangError::new(
                1,
                format!("slice edge must be a number, got {}", other.type_name()),
                "write r[1:3]".to_string(),
            )),
        }
    };
    let norm = |v: i64| {
        if v < 0 {
            (len + v).max(0).min(len)
        } else {
            v.max(0).min(len)
        }
    };
    let a = match edge(s)? {
        None => 0,
        Some(v) => norm(v),
    } as usize;
    let b = match edge(e)? {
        None => len,
        Some(v) => norm(v),
    } as usize;
    let (from, to) = if a <= b { (a, b) } else { (b, a) };
    match o {
        Value::List(v) => Ok(Value::List(v[from.min(v.len())..to.min(v.len())].to_vec())),
        Value::Str(x) => {
            let chars: Vec<char> = x.chars().collect();
            Ok(Value::Str(
                chars[from.min(chars.len())..to.min(chars.len())].iter().collect(),
            ))
        }
        _ => unreachable!(),
    }
}

fn v_attr(o: Value, f: &str) -> Result<Value, ClangError> {
    match o {
        Value::Map(m) => m.get(f).cloned().ok_or_else(|| {
            ClangError::new(1, format!("missing field '{}'", f), "check the names".to_string())
        }),
        Value::Str(s) if f == "len" => Ok(Value::Num(s.chars().count() as f64)),
        Value::List(v) if f == "len" => Ok(Value::Num(v.len() as f64)),
        other => Err(ClangError::new(
            1,
            format!("no field '{}' on {}", f, other.type_name()),
            "only maps carry named fields".to_string(),
        )),
    }
}

fn call_builtin(vm: &mut VM, name: &str, args: Vec<Value>) -> Result<Value, ClangError> {
    let line = vm.frames.last().map(|f| f.line).unwrap_or(1);
    let want = |n: usize| -> Result<(), ClangError> {
        if args.len() != n {
            return Err(ClangError::new(
                line,
                format!("{} wants {} inputs but got {}", name, n, args.len()),
                "check the call".to_string(),
            ));
        }
        Ok(())
    };
    match name {
        "emit" => {
            let parts: Vec<String> = args.iter().map(|v| v.display()).collect();
            let text = parts.join(" ");
            if !vm.quiet {
                println!("{}", text);
            }
            vm.printed.push(text);
            Ok(Value::Null)
        }
        "cap_first" => {
            want(1)?;
            match &args[0] {
                Value::Str(s) => {
                    let mut ch: Vec<char> = s.chars().collect();
                    if let Some(f) = ch.first_mut() {
                        *f = f.to_uppercase().next().unwrap_or(*f);
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
            want(1)?;
            match &args[0] {
                Value::Str(s) => Ok(Value::Str(s.to_lowercase())),
                other => Err(ClangError::new(
                    line,
                    format!("lower needs string, got {}", other.type_name()),
                    "check the call".to_string(),
                )),
            }
        }
        "upper" => {
            want(1)?;
            match &args[0] {
                Value::Str(s) => Ok(Value::Str(s.to_uppercase())),
                other => Err(ClangError::new(
                    line,
                    format!("upper needs string, got {}", other.type_name()),
                    "check the call".to_string(),
                )),
            }
        }
        "contains" => {
            want(2)?;
            match (&args[0], &args[1]) {
                (Value::Str(h), Value::Str(n)) => Ok(Value::Bool(h.contains(n.as_str()))),
                (Value::List(v), item) => {
                    Ok(Value::Bool(v.iter().any(|x| values_equal(x, item))))
                }
                _ => Err(ClangError::new(
                    line,
                    "contains cannot search those".to_string(),
                    "use contains(text, part)".to_string(),
                )),
            }
        }
        "len" => {
            want(1)?;
            match &args[0] {
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
        "split" => {
            want(2)?;
            match (&args[0], &args[1]) {
                (Value::Str(s), Value::Str(d)) => {
                    if d.is_empty() {
                        Ok(Value::List(
                            s.chars().map(|c| Value::Str(c.to_string())).collect(),
                        ))
                    } else {
                        Ok(Value::List(
                            s.split(d.as_str()).map(|p| Value::Str(p.to_string())).collect(),
                        ))
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
            want(2)?;
            match (&args[0], &args[1]) {
                (Value::List(v), Value::Str(d)) => {
                    Ok(Value::Str(v.iter().map(|x| x.display()).collect::<Vec<_>>().join(d)))
                }
                _ => Err(ClangError::new(
                    line,
                    "join needs (list, sep)".to_string(),
                    "call join(parts, \",\")".to_string(),
                )),
            }
        }
        "trim" => {
            want(1)?;
            match &args[0] {
                Value::Str(s) => Ok(Value::Str(s.trim().to_string())),
                other => Err(ClangError::new(
                    line,
                    format!("trim needs string, got {}", other.type_name()),
                    "check the call".to_string(),
                )),
            }
        }
        "replace" => {
            want(3)?;
            match (&args[0], &args[1], &args[2]) {
                (Value::Str(s), Value::Str(a), Value::Str(b)) => {
                    Ok(Value::Str(s.replace(a.as_str(), b.as_str())))
                }
                _ => Err(ClangError::new(
                    line,
                    "replace needs (text, old, new)".to_string(),
                    "check the call".to_string(),
                )),
            }
        }
        "starts_with" => {
            want(2)?;
            match (&args[0], &args[1]) {
                (Value::Str(s), Value::Str(p)) => Ok(Value::Bool(s.starts_with(p.as_str()))),
                _ => Err(ClangError::new(line, "starts_with needs strings".to_string(), "check the call".to_string())),
            }
        }
        "ends_with" => {
            want(2)?;
            match (&args[0], &args[1]) {
                (Value::Str(s), Value::Str(p)) => Ok(Value::Bool(s.ends_with(p.as_str()))),
                _ => Err(ClangError::new(line, "ends_with needs strings".to_string(), "check the call".to_string())),
            }
        }
        "abs" => {
            want(1)?;
            match &args[0] {
                Value::Num(n) => Ok(Value::Num(n.abs())),
                other => Err(ClangError::new(
                    line,
                    format!("abs needs number, got {}", other.type_name()),
                    "check the call".to_string(),
                )),
            }
        }
        "floor" => {
            want(1)?;
            match &args[0] {
                Value::Num(n) => Ok(Value::Num(n.floor())),
                other => Err(ClangError::new(
                    line,
                    format!("floor needs number, got {}", other.type_name()),
                    "check the call".to_string(),
                )),
            }
        }
        "ceil" => {
            want(1)?;
            match &args[0] {
                Value::Num(n) => Ok(Value::Num(n.ceil())),
                other => Err(ClangError::new(
                    line,
                    format!("ceil needs number, got {}", other.type_name()),
                    "check the call".to_string(),
                )),
            }
        }
        "sqrt" => {
            want(1)?;
            match &args[0] {
                Value::Num(n) if *n >= 0.0 => Ok(Value::Num(n.sqrt())),
                _ => Err(ClangError::new(line, "sqrt needs zero or more".to_string(), "guard negatives".to_string())),
            }
        }
        "pow" => {
            want(2)?;
            match (&args[0], &args[1]) {
                (Value::Num(a), Value::Num(b)) => Ok(Value::Num(a.powf(*b))),
                _ => Err(ClangError::new(line, "pow needs numbers".to_string(), "call pow(2, 8)".to_string())),
            }
        }
        "min" => {
            want(2)?;
            match (&args[0], &args[1]) {
                (Value::Num(a), Value::Num(b)) => Ok(Value::Num(a.min(*b))),
                _ => Err(ClangError::new(line, "min needs numbers".to_string(), "call min(a, b)".to_string())),
            }
        }
        "max" => {
            want(2)?;
            match (&args[0], &args[1]) {
                (Value::Num(a), Value::Num(b)) => Ok(Value::Num(a.max(*b))),
                _ => Err(ClangError::new(line, "max needs numbers".to_string(), "call max(a, b)".to_string())),
            }
        }
        "str" => {
            want(1)?;
            Ok(Value::Str(args[0].display()))
        }
        "num" => {
            want(1)?;
            match &args[0] {
                Value::Num(n) => Ok(Value::Num(*n)),
                Value::Str(s) => s
                    .trim()
                    .parse::<f64>()
                    .map(Value::Num)
                    .map_err(|_| ClangError::new(line, format!("cannot turn '{}' into a number", s), "pass digits".to_string())),
                Value::Bool(b) => Ok(Value::Num(if *b { 1.0 } else { 0.0 })),
                other => Err(ClangError::new(
                    line,
                    format!("num cannot convert {}", other.type_name()),
                    "pass a string or number".to_string(),
                )),
            }
        }
        "empty_list" => {
            want(0)?;
            Ok(Value::List(Vec::new()))
        }
        "assert" => {
            if args.len() != 1 && args.len() != 2 {
                return Err(ClangError::new(
                    line,
                    format!("assert wants 1 or 2 inputs but got {}", args.len()),
                    "call assert(ok)".to_string(),
                ));
            }
            if !args[0].is_truthy() {
                let msg = if args.len() == 2 {
                    args[1].display()
                } else {
                    "check failed".to_string()
                };
                return Err(ClangError::new(
                    line,
                    format!("assert failed: {}", msg),
                    "inspect the values above".to_string(),
                ));
            }
            Ok(Value::Null)
        }
        "rand_seed" => {
            want(1)?;
            match &args[0] {
                Value::Num(n) => {
                    vm.rng = (*n as u64) ^ 0x9e3779b97f4a7c15;
                    if vm.rng == 0 {
                        vm.rng = 0x12345678;
                    }
                    Ok(Value::Null)
                }
                other => Err(ClangError::new(
                    line,
                    format!("rand_seed needs number, got {}", other.type_name()),
                    "check the call".to_string(),
                )),
            }
        }
        "rand_int" => {
            want(2)?;
            match (&args[0], &args[1]) {
                (Value::Num(a), Value::Num(b)) => {
                    let lo = *a as i64;
                    let hi = *b as i64;
                    if lo > hi {
                        return Err(ClangError::new(
                            line,
                            "rand_int needs min <= max".to_string(),
                            "call rand_int(1, 10)".to_string(),
                        ));
                    }
                    vm.rng = vm
                        .rng
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let r = ((vm.rng >> 33) & 0x7fffffff) % ((hi - lo + 1) as u64);
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
            want(1)?;
            Ok(Value::Bool(matches!(args[0], Value::Str(_))))
        }
        "is_num" => {
            want(1)?;
            Ok(Value::Bool(matches!(args[0], Value::Num(_))))
        }
        "is_list" => {
            want(1)?;
            Ok(Value::Bool(matches!(args[0], Value::List(_))))
        }
        "is_map" => {
            want(1)?;
            Ok(Value::Bool(matches!(args[0], Value::Map(_))))
        }
        "type_of" => {
            want(1)?;
            Ok(Value::Str(args[0].type_name().to_string()))
        }
        "keys" => {
            want(1)?;
            match &args[0] {
                Value::Map(m) => {
                    let mut ks: Vec<Value> =
                        m.keys().map(|k| Value::Str(k.clone())).collect();
                    ks.sort_by(|a, b| a.display().cmp(&b.display()));
                    Ok(Value::List(ks))
                }
                other => Err(ClangError::new(
                    line,
                    format!("keys needs map, got {}", other.type_name()),
                    "check the call".to_string(),
                )),
            }
        }
        _ => Err(ClangError::new(
            line,
            format!("unknown builtin '{}'", name),
            "report this bug".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use crate::interp::Interp;
    use crate::parser::Parser;

    fn both(src: &str) -> (Vec<String>, Vec<String>) {
        let mut p = Parser::new(src);
        let prog = p.parse_program().expect("parses");
        let mut w = Interp::new();
        w.run_program(&prog).expect("walk runs");
        let fast = super::try_run(&prog, Vec::new());
        match fast {
            Ok(out) => (w.printed.clone(), out),
            Err(super::FastError::Unsupported(u)) => panic!("vm should support pure src, got unsupported {}", u),
            Err(super::FastError::Clang(e)) => panic!("vm failed: {}", e),
        }
    }

    #[test]
    fn parity_hello() {
        let (a, b) = both("var:x = \"world\"\nemit(cap_first(x))\n");
        assert_eq!(a, b);
        assert_eq!(a, vec!["World"]);
    }

    #[test]
    fn parity_calc_core() {
        let src = "var:last = 0\nfn::add[a, b] {\n var:o = a + b\n var:last = o\n emit(f\"add {a} + {b} = {o}\")\n return(o)\n}\nfn::boot() {\n add(2, 3)\n emit(last)\n}\nrun(boot)\n";
        let (a, b) = both(src);
        assert_eq!(a, b);
        assert_eq!(a, vec!["add 2 + 3 = 5", "5"]);
    }

    #[test]
    fn parity_loops_slices() {
        let src = "var:xs = [1, 2, 3, 4]\nvar:tot = 0\nfor(x in xs) {\n var:tot = tot + x\n}\nemit(tot)\nemit(xs[1:3])\nfor(y in [10, 20]) {\n for(z in [1, 2]) {\n emit(y + z)\n }\n}\n";
        let (a, b) = both(src);
        assert_eq!(a, b);
        assert_eq!(a[0], "10");
    }

    #[test]
    fn parity_push_and_rand() {
        let src = "var:h = empty_list()\nrand_seed(7)\nvar:a = rand_int(1, 10)\npush(h, a)\npush(h, 99)\nemit(len(h))\nemit(h[0] == a)\n";
        let (a, b) = both(src);
        assert_eq!(a, b);
        assert_eq!(a, vec!["2", "true"]);
    }

    #[test]
    fn io_falls_back() {
        let mut p = Parser::new("var:x = file_read(\"a\")\nemit(x)\n");
        let prog = p.parse_program().expect("parses");
        assert!(matches!(super::try_run(&prog, Vec::new()), Err(super::FastError::Unsupported(_))));
    }
}
