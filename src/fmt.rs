use crate::error::ClangError;
use crate::parser::{Expr, Op, Parser, Stmt};

pub fn format_src(src: &str) -> Result<String, ClangError> {
    let mut p = Parser::new(src);
    let prog = p.parse_program()?;
    let mut out = String::new();
    for s in &prog {
        stmt_fmt(s, 0, &mut out)?;
    }
    Ok(out)
}

fn indent(n: usize, out: &mut String) {
    for _ in 0..n {
        out.push_str("  ");
    }
}

fn stmt_fmt(s: &Stmt, depth: usize, out: &mut String) -> Result<(), ClangError> {
    match s {
        Stmt::VarDecl(n, e, _) => {
            indent(depth, out);
            out.push_str(&format!("var:{} = {}\n", n, expr_fmt(e)?));
        }
        Stmt::Assign(n, e, _) => {
            indent(depth, out);
            out.push_str(&format!("{} = {}\n", n, expr_fmt(e)?));
        }
        Stmt::ExprStmt(e, _) => {
            indent(depth, out);
            out.push_str(&format!("{}\n", expr_fmt(e)?));
        }
        Stmt::Return(e, _) => {
            indent(depth, out);
            out.push_str(&format!("return({})\n", expr_fmt(e)?));
        }
        Stmt::Run(n, _) => {
            indent(depth, out);
            out.push_str(&format!("run({})\n", n));
        }
        Stmt::If(c, a, b, _) => {
            indent(depth, out);
            out.push_str(&format!("if({}) {{\n", expr_fmt(c)?));
            for x in a {
                stmt_fmt(x, depth + 1, out)?;
            }
            indent(depth, out);
            if b.is_empty() {
                out.push_str("}\n");
            } else {
                out.push_str("} else {\n");
                for x in b {
                    stmt_fmt(x, depth + 1, out)?;
                }
                indent(depth, out);
                out.push_str("}\n");
            }
        }
        Stmt::For(v, it, body, _) => {
            indent(depth, out);
            out.push_str(&format!("for({} in {}) {{\n", v, expr_fmt(it)?));
            for x in body {
                stmt_fmt(x, depth + 1, out)?;
            }
            indent(depth, out);
            out.push_str("}\n");
        }
        Stmt::Loop(c, body, _) => {
            indent(depth, out);
            out.push_str(&format!("loop({}) {{\n", expr_fmt(c)?));
            for x in body {
                stmt_fmt(x, depth + 1, out)?;
            }
            indent(depth, out);
            out.push_str("}\n");
        }
        Stmt::Stop(_) => {
            indent(depth, out);
            out.push_str("stop\n");
        }
        Stmt::Skip(_) => {
            indent(depth, out);
            out.push_str("skip\n");
        }
        Stmt::Safe(a, b, _) => {
            indent(depth, out);
            out.push_str("safe {\n");
            for x in a {
                stmt_fmt(x, depth + 1, out)?;
            }
            indent(depth, out);
            if b.is_empty() {
                out.push_str("}\n");
            } else {
                out.push_str("} else {\n");
                for x in b {
                    stmt_fmt(x, depth + 1, out)?;
                }
                indent(depth, out);
                out.push_str("}\n");
            }
        }
        Stmt::Select(on, arms, els, _) => {
            indent(depth, out);
            out.push_str(&format!("select({}) {{\n", expr_fmt(on)?));
            for (v, body) in arms {
                indent(depth + 1, out);
                out.push_str(&format!("is({}) {{\n", expr_fmt(v)?));
                for x in body {
                    stmt_fmt(x, depth + 2, out)?;
                }
                indent(depth + 1, out);
                out.push_str("}\n");
            }
            if !els.is_empty() {
                indent(depth + 1, out);
                out.push_str("else {\n");
                for x in els {
                    stmt_fmt(x, depth + 2, out)?;
                }
                indent(depth + 1, out);
                out.push_str("}\n");
            }
            indent(depth, out);
            out.push_str("}\n");
        }
        Stmt::Unpack(names, e, _) => {
            indent(depth, out);
            out.push_str(&format!("var:[{}] = {}\n", names.join(", "), expr_fmt(e)?));
        }
        Stmt::FuncDef(n, ps, body, _) => {
            indent(depth, out);
            if ps.len() <= 1 {
                out.push_str(&format!("fn::{}({}) {{\n", n, ps.join(", ")));
            } else {
                out.push_str(&format!("fn::{}[{}] {{\n", n, ps.join(", ")));
            }
            for x in body {
                stmt_fmt(x, depth + 1, out)?;
            }
            indent(depth, out);
            out.push_str("}\n");
        }
    }
    Ok(())
}

fn expr_fmt(e: &Expr) -> Result<String, ClangError> {
    match e {
        Expr::Str { val, is_f, is_raw, .. } => {
            let mut pre = String::new();
            if *is_f {
                pre.push('f');
            }
            if *is_raw {
                pre.push('r');
            }
            Ok(format!("{}\"{}\"", pre, val.replace('"', "\\\"")))
        }
        Expr::Num(n, _) => Ok({
            if n.fract() == 0.0 && n.abs() < 9e15 {
                format!("{}", *n as i64)
            } else {
                format!("{}", n)
            }
        }),
        Expr::Bool(b, _) => Ok(b.to_string()),
        Expr::Null(_) => Ok("null".to_string()),
        Expr::List(items, _) => {
            let mut parts = Vec::new();
            for x in items {
                parts.push(expr_fmt(x)?);
            }
            Ok(format!("[{}]", parts.join(", ")))
        }
        Expr::Var(n, _) => Ok(n.clone()),
        Expr::Call(n, args, _) => {
            let mut parts = Vec::new();
            for a in args {
                parts.push(expr_fmt(a)?);
            }
            Ok(format!("{}({})", n, parts.join(", ")))
        }
        Expr::Attr(o, f, _) => Ok(format!("{}.{}", expr_fmt(o)?, f)),
        Expr::Index(o, i, _) => Ok(format!("{}[{}]", expr_fmt(o)?, expr_fmt(i)?)),
        Expr::Slice { obj, start, end, .. } => {
            let s = match start {
                Some(x) => expr_fmt(x)?,
                None => String::new(),
            };
            let x = match end {
                Some(v) => expr_fmt(v)?,
                None => String::new(),
            };
            Ok(format!("{}[{}:{}]", expr_fmt(obj)?, s, x))
        }
        Expr::BinOp(l, op, r, _) => {
            let mark = match op {
                Op::Add => "+",
                Op::Sub => "-",
                Op::Mul => "*",
                Op::Div => "/",
                Op::Mod => "%",
                Op::Eq => "==",
                Op::Ne => "!=",
                Op::Lt => "<",
                Op::Gt => ">",
                Op::Le => "<=",
                Op::Ge => ">=",
            };
            Ok(format!("{} {} {}", expr_fmt(l)?, mark, expr_fmt(r)?))
        }
        Expr::Neg(x, _) => Ok(format!("-{}", expr_fmt(x)?)),
        Expr::Both(a, b, _) => Ok(format!("both({}, {})", expr_fmt(a)?, expr_fmt(b)?)),
        Expr::Either(a, b, _) => Ok(format!("either({}, {})", expr_fmt(a)?, expr_fmt(b)?)),
        Expr::Flip(x, _) => Ok(format!("flip({})", expr_fmt(x)?)),
        Expr::Pick(c, a, b, _) => Ok(format!(
            "pick({}, {}, {})",
            expr_fmt(c)?,
            expr_fmt(a)?,
            expr_fmt(b)?
        )),
        Expr::Soft(x, _) => Ok(format!("soft({})", expr_fmt(x)?)),
    }
}


#[cfg(test)]
mod tests {
    use super::doc_stubs;

    #[test]
    fn doc_picks_notes() {
        let src = "# adds two\n# keep small\nfn::add[a, b] {\n return(a + b)\n}\n";
        let out = doc_stubs(src);
        assert!(out.contains("## add"));
        assert!(out.contains("adds two"));
    }

    use super::format_src;

    #[test]
    fn fmt_is_idempotent_on_hello() {
        let src = "var:response = \"world\"\nfn::allerg(r) {\nvar:out = cap_first(r)\nemit(out)\n}\nrun(boot)\n";
        let once = format_src(src).expect("fmt runs");
        let twice = format_src(&once).expect("fmt reruns");
        assert_eq!(once, twice);
        assert!(once.contains("var:response"));
        assert!(once.contains("fn::allerg(r)"));
    }
}

pub fn doc_stubs(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = String::from("# CLang docs\n");
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim();
        if t.starts_with("fn::") {
            let name = t[4..]
                .split(|c| c == '(' || c == '[' || c == ' ' || c == '{')
                .next()
                .unwrap_or("?");
            let mut notes: Vec<&str> = Vec::new();
            let mut j = i;
            while j > 0 {
                j -= 1;
                let prev = lines[j].trim();
                if prev.starts_with('#') {
                    notes.push(prev[1..].trim());
                } else if prev.is_empty() {
                    continue;
                } else {
                    break;
                }
            }
            notes.reverse();
            out.push_str(&format!("\n## {}\n", name));
            if notes.is_empty() {
                out.push_str("no notes yet\n");
            } else {
                for n in notes {
                    out.push_str(n);
                    out.push('\n');
                }
            }
        }
        i += 1;
    }
    out
}
