use crate::error::ClangError;

#[derive(Debug, Clone)]
pub enum Op {
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
}

#[derive(Debug, Clone)]
pub enum Expr {
    Str {
        val: String,
        is_f: bool,
        is_raw: bool,
        line: usize,
    },
    Num(f64, usize),
    Bool(bool, usize),
    Null(usize),
    List(Vec<Expr>, usize),
    Var(String, usize),
    Call(String, Vec<Expr>, usize),
    Attr(Box<Expr>, String, usize),
    Index(Box<Expr>, Box<Expr>, usize),
    Slice {
        obj: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        line: usize,
    },
    BinOp(Box<Expr>, Op, Box<Expr>, usize),
    Neg(Box<Expr>, usize),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    VarDecl(String, Expr, usize),
    Assign(String, Expr, usize),
    ExprStmt(Expr, usize),
    Return(Expr, usize),
    Run(String, usize),
    If(Expr, Vec<Stmt>, Vec<Stmt>, usize),
    For(String, Expr, Vec<Stmt>, usize),
    FuncDef(String, Vec<String>, Vec<Stmt>, usize),
}

pub struct Parser {
    chars: Vec<char>,
    pos: usize,
    line: usize,
}

impl Parser {
    pub fn new(src: &str) -> Self {
        Self {
            chars: src.chars().collect(),
            pos: 0,
            line: 1,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn eof(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn err(&self, msg: String, hint: String) -> ClangError {
        ClangError::new(self.line, msg, hint)
    }

    pub fn skip_space(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c == ' ' || c == '\t' || c == '\r' || c == '\n' || c == ';' => {
                    self.next();
                }
                Some('#') => {
                    while let Some(ch) = self.next() {
                        if ch == '\n' {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
    }

    fn expect_char(&mut self, want: char, what: &str) -> Result<(), ClangError> {
        self.skip_space();
        match self.next() {
            Some(c) if c == want => Ok(()),
            Some(c) => Err(self.err(
                format!("expected {} but found '{}'", what, c),
                format!("check line {} near '{}'", self.line, what),
            )),
            None => Err(self.err(
                format!("expected {} but hit end of file", what),
                "add the missing closing character".to_string(),
            )),
        }
    }

    fn consume_word(&mut self, word: &str) -> Result<(), ClangError> {
        self.skip_space();
        for wc in word.chars() {
            match self.next() {
                Some(c) if c == wc => {}
                _ => {
                    return Err(self.err(
                        format!("expected '{}'", word),
                        "check spelling of the keyword".to_string(),
                    ))
                }
            }
        }
        Ok(())
    }

    fn parse_ident(&mut self) -> Result<String, ClangError> {
        self.skip_space();
        let mut name = String::new();
        match self.peek() {
            Some(c) if c.is_alphabetic() || c == '_' => {
                name.push(self.next().unwrap());
            }
            _ => {
                return Err(self.err(
                    "expected a name".to_string(),
                    "names start with a letter or underscore".to_string(),
                ))
            }
        }
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                name.push(self.next().unwrap());
            } else {
                break;
            }
        }
        Ok(name)
    }

    fn peek_word(&mut self) -> String {
        let save_pos = self.pos;
        let save_line = self.line;
        self.skip_space();
        let mut w = String::new();
        let mut tmp = self.pos;
        while let Some(c) = self.chars.get(tmp).copied() {
            if c.is_alphanumeric() || c == '_' {
                w.push(c);
                tmp += 1;
            } else {
                break;
            }
        }
        self.pos = save_pos;
        self.line = save_line;
        w
    }

    pub fn parse_program(&mut self) -> Result<Vec<Stmt>, ClangError> {
        let mut out = Vec::new();
        loop {
            self.skip_space();
            if self.eof() {
                break;
            }
            if self.peek() == Some('}') {
                break;
            }
            out.push(self.parse_stmt()?);
        }
        Ok(out)
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ClangError> {
        self.skip_space();
        let line = self.line;

        if self.peek_slice("var:") {
            self.consume_word("var:")?;
            let name = self.parse_ident()?;
            self.skip_space();
            self.expect_char('=', "'=' after variable name")?;
            let e = self.parse_expr()?;
            return Ok(Stmt::VarDecl(name, e, line));
        }

        if self.peek_slice("fn::") {
            self.consume_word("fn::")?;
            let name = self.parse_ident()?;
            self.skip_space();
            let params = self.parse_params()?;
            let body = self.parse_block()?;
            return Ok(Stmt::FuncDef(name, params, body, line));
        }

        let w = self.peek_word();
        if w == "return" {
            self.consume_word("return")?;
            self.skip_space();
            self.expect_char('(', "'(' after return")?;
            let e = self.parse_expr()?;
            self.expect_char(')', "')' to close return")?;
            return Ok(Stmt::Return(e, line));
        }
        if w == "run" {
            let save = self.pos;
            let save_line = self.line;
            self.consume_word("run")?;
            self.skip_space();
            if self.peek() == Some('(') {
                self.next();
                self.skip_space();
                let name = self.parse_ident()?;
                self.skip_space();
                self.expect_char(')', "')' to close run")?;
                return Ok(Stmt::Run(name, line));
            }
            self.pos = save;
            self.line = save_line;
        }
        if w == "if" {
            self.consume_word("if")?;
            self.skip_space();
            self.expect_char('(', "'(' after if")?;
            let cond = self.parse_expr()?;
            self.expect_char(')', "')' to close if condition")?;
            let then_b = self.parse_block()?;
            self.skip_space();
            let mut else_b = Vec::new();
            if self.peek_word() == "else" {
                self.consume_word("else")?;
                else_b = self.parse_block()?;
            }
            return Ok(Stmt::If(cond, then_b, else_b, line));
        }
        if w == "for" {
            self.consume_word("for")?;
            self.skip_space();
            let has_paren = self.peek() == Some('(');
            if has_paren {
                self.next();
            }
            let var = self.parse_ident()?;
            self.skip_space();
            let iw = self.peek_word();
            if iw != "in" {
                return Err(self.err(
                    "expected 'in' inside for loop".to_string(),
                    "write for(x in items) { ... }".to_string(),
                ));
            }
            self.consume_word("in")?;
            let iter = self.parse_expr()?;
            self.skip_space();
            if has_paren {
                self.expect_char(')', "')' to close for header")?;
            }
            let body = self.parse_block()?;
            return Ok(Stmt::For(var, iter, body, line));
        }

        if self.is_assign_ahead() {
            let name = self.parse_ident()?;
            self.skip_space();
            self.expect_char('=', "'=' in assignment")?;
            let e = self.parse_expr()?;
            return Ok(Stmt::Assign(name, e, line));
        }

        let e = self.parse_expr()?;
        Ok(Stmt::ExprStmt(e, line))
    }

    fn peek_slice(&self, s: &str) -> bool {
        let mut tmp = self.pos;
        let chars: Vec<char> = s.chars().collect();
        let mut p = tmp;
        while p < self.chars.len() && (self.chars[p] == ' ' || self.chars[p] == '\t') {
            p += 1;
        }
        tmp = p;
        for wc in chars {
            if self.chars.get(tmp).copied() != Some(wc) {
                return false;
            }
            tmp += 1;
        }
        true
    }

    fn is_assign_ahead(&self) -> bool {
        let mut tmp = self.pos;
        while tmp < self.chars.len()
            && (self.chars[tmp] == ' ' || self.chars[tmp] == '\t')
        {
            tmp += 1;
        }
        let mut has_ident = false;
        let mut t = tmp;
        if let Some(c) = self.chars.get(t).copied() {
            if c.is_alphabetic() || c == '_' {
                has_ident = true;
                t += 1;
                while let Some(ch) = self.chars.get(t).copied() {
                    if ch.is_alphanumeric() || ch == '_' {
                        t += 1;
                    } else {
                        break;
                    }
                }
            }
        }
        if !has_ident {
            return false;
        }
        while t < self.chars.len() && (self.chars[t] == ' ' || self.chars[t] == '\t') {
            t += 1;
        }
        if self.chars.get(t).copied() != Some('=') {
            return false;
        }
        if self.chars.get(t + 1).copied() == Some('=') {
            return false;
        }
        true
    }

    fn parse_params(&mut self) -> Result<Vec<String>, ClangError> {
        self.skip_space();
        match self.peek() {
            Some('(') => {
                self.next();
                self.skip_space();
                let mut out = Vec::new();
                if self.peek() == Some(')') {
                    self.next();
                    return Ok(out);
                }
                loop {
                    self.skip_space();
                    if self.peek() == Some(')') {
                        self.next();
                        break;
                    }
                    let name = self.parse_ident()?;
                    out.push(name);
                    self.skip_space();
                    if self.peek() == Some(',') {
                        self.next();
                        continue;
                    }
                    if self.peek() == Some(')') {
                        self.next();
                        break;
                    }
                    return Err(self.err(
                        "expected ',' or ')' in parameter list".to_string(),
                        "write fn::name(a, b) or fn::name[a, b]".to_string(),
                    ));
                }
                Ok(out)
            }
            Some('[') => {
                self.next();
                let mut out = Vec::new();
                loop {
                    self.skip_space();
                    if self.peek() == Some(']') {
                        self.next();
                        break;
                    }
                    let name = self.parse_ident()?;
                    out.push(name);
                    self.skip_space();
                    if self.peek() == Some(',') {
                        self.next();
                        continue;
                    }
                    if self.peek() == Some(']') {
                        self.next();
                        break;
                    }
                    return Err(self.err(
                        "expected ',' or ']' in parameter list".to_string(),
                        "write fn::name[a, b]".to_string(),
                    ));
                }
                Ok(out)
            }
            _ => Err(self.err(
                "expected '(' or '[' after function name".to_string(),
                "one input uses fn::name(r), several use fn::name[a, b]".to_string(),
            )),
        }
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, ClangError> {
        self.skip_space();
        self.expect_char('{', "'{' to open a block")?;
        let mut out = Vec::new();
        loop {
            self.skip_space();
            if self.eof() {
                return Err(self.err(
                    "block was never closed".to_string(),
                    "add a matching '}'".to_string(),
                ));
            }
            if self.peek() == Some('}') {
                self.next();
                break;
            }
            out.push(self.parse_stmt()?);
        }
        Ok(out)
    }

    pub fn parse_expr(&mut self) -> Result<Expr, ClangError> {
        self.parse_equality()
    }

    fn parse_equality(&mut self) -> Result<Expr, ClangError> {
        let mut left = self.parse_comparison()?;
        loop {
            self.skip_space();
            let line = self.line;
            if self.peek() == Some('=')
                && self.peek2() == Some('=')
            {
                self.next();
                self.next();
                let right = self.parse_comparison()?;
                left = Expr::BinOp(Box::new(left), Op::Eq, Box::new(right), line);
            } else if self.peek() == Some('!')
                && self.peek2() == Some('=')
            {
                self.next();
                self.next();
                let right = self.parse_comparison()?;
                left = Expr::BinOp(Box::new(left), Op::Ne, Box::new(right), line);
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ClangError> {
        let mut left = self.parse_term()?;
        loop {
            self.skip_space();
            let line = self.line;
            let op = if self.peek() == Some('<') && self.peek2() == Some('=') {
                self.next();
                self.next();
                Some(Op::Le)
            } else if self.peek() == Some('>') && self.peek2() == Some('=') {
                self.next();
                self.next();
                Some(Op::Ge)
            } else if self.peek() == Some('<') {
                self.next();
                Some(Op::Lt)
            } else if self.peek() == Some('>') {
                self.next();
                Some(Op::Gt)
            } else {
                None
            };
            match op {
                Some(o) => {
                    let right = self.parse_term()?;
                    left = Expr::BinOp(Box::new(left), o, Box::new(right), line);
                }
                None => break,
            }
        }
        Ok(left)
    }

    fn parse_term(&mut self) -> Result<Expr, ClangError> {
        let mut left = self.parse_factor()?;
        loop {
            self.skip_space();
            let line = self.line;
            if self.peek() == Some('+') {
                self.next();
                let right = self.parse_factor()?;
                left = Expr::BinOp(Box::new(left), Op::Add, Box::new(right), line);
            } else if self.peek() == Some('-') {
                if self.is_negative_number_ahead() {
                    break;
                }
                self.next();
                let right = self.parse_factor()?;
                left = Expr::BinOp(Box::new(left), Op::Sub, Box::new(right), line);
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn is_negative_number_ahead(&self) -> bool {
        false
    }

    fn parse_factor(&mut self) -> Result<Expr, ClangError> {
        let mut left = self.parse_unary()?;
        loop {
            self.skip_space();
            let line = self.line;
            if self.peek() == Some('*') {
                self.next();
                let right = self.parse_unary()?;
                left = Expr::BinOp(Box::new(left), Op::Mul, Box::new(right), line);
            } else if self.peek() == Some('/') {
                self.next();
                let right = self.parse_unary()?;
                left = Expr::BinOp(Box::new(left), Op::Div, Box::new(right), line);
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ClangError> {
        self.skip_space();
        let line = self.line;
        if self.peek() == Some('-') {
            let save = self.pos;
            self.next();
            self.skip_space();
            if let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.pos = save;
                    self.line = line;
                    return self.parse_postfix();
                }
            }
            let e = self.parse_unary()?;
            return Ok(Expr::Neg(Box::new(e), line));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ClangError> {
        let mut node = self.parse_primary()?;
        loop {
            self.skip_space();
            let line = self.line;
            if self.peek() == Some('.') {
                self.next();
                let name = self.parse_ident()?;
                node = Expr::Attr(Box::new(node), name, line);
            } else if self.peek() == Some('[') {
                self.next();
                self.skip_space();
                if self.peek() == Some(':') {
                    self.next();
                    self.skip_space();
                    let end = if self.peek() == Some(']') {
                        None
                    } else {
                        Some(Box::new(self.parse_expr()?))
                    };
                    self.skip_space();
                    self.expect_char(']', "']' to close slice")?;
                    node = Expr::Slice {
                        obj: Box::new(node),
                        start: None,
                        end,
                        line,
                    };
                } else {
                    let start = self.parse_expr()?;
                    self.skip_space();
                    if self.peek() == Some(':') {
                        self.next();
                        self.skip_space();
                        let end = if self.peek() == Some(']') {
                            None
                        } else {
                            Some(Box::new(self.parse_expr()?))
                        };
                        self.skip_space();
                        self.expect_char(']', "']' to close slice")?;
                        node = Expr::Slice {
                            obj: Box::new(node),
                            start: Some(Box::new(start)),
                            end,
                            line,
                        };
                    } else {
                        self.expect_char(']', "']' to close index")?;
                        node = Expr::Index(Box::new(node), Box::new(start), line);
                    }
                }
            } else {
                break;
            }
        }
        Ok(node)
    }

    fn parse_primary(&mut self) -> Result<Expr, ClangError> {
        self.skip_space();
        let line = self.line;
        match self.peek() {
            Some('(') => {
                self.next();
                let e = self.parse_expr()?;
                self.skip_space();
                self.expect_char(')', "')' to close group")?;
                Ok(e)
            }
            Some('[') => {
                self.next();
                let mut items = Vec::new();
                loop {
                    self.skip_space();
                    if self.peek() == Some(']') {
                        self.next();
                        break;
                    }
                    if self.eof() {
                        return Err(self.err(
                            "list was never closed".to_string(),
                            "add a matching ']'".to_string(),
                        ));
                    }
                    let e = self.parse_expr()?;
                    items.push(e);
                    self.skip_space();
                    if self.peek() == Some(',') {
                        self.next();
                        continue;
                    }
                    self.skip_space();
                    if self.peek() == Some(']') {
                        self.next();
                        break;
                    }
                    return Err(self.err(
                        "expected ',' or ']' in list".to_string(),
                        "separate items with commas".to_string(),
                    ));
                }
                Ok(Expr::List(items, line))
            }
            Some('"') | Some('\'') => self.parse_string(),
            Some(c) if c.is_ascii_digit() => self.parse_number(),
            Some(c) if c.is_alphabetic() || c == '_' || c == 'f' || c == 'r' => {
                if self.is_string_prefix_ahead() {
                    return self.parse_string();
                }
                let name = self.parse_ident()?;
                if name == "true" {
                    return Ok(Expr::Bool(true, line));
                }
                if name == "false" {
                    return Ok(Expr::Bool(false, line));
                }
                if name == "null" {
                    return Ok(Expr::Null(line));
                }
                self.skip_space();
                if self.peek() == Some('(') {
                    self.next();
                    let mut args = Vec::new();
                    loop {
                        self.skip_space();
                        if self.peek() == Some(')') {
                            self.next();
                            break;
                        }
                        if self.eof() {
                            return Err(self.err(
                                format!("call {} was never closed", name),
                                "add a matching ')'".to_string(),
                            ));
                        }
                        let e = self.parse_expr()?;
                        args.push(e);
                        self.skip_space();
                        if self.peek() == Some(',') {
                            self.next();
                            continue;
                        }
                        self.skip_space();
                        if self.peek() == Some(')') {
                            self.next();
                            break;
                        }
                        return Err(self.err(
                            "expected ',' or ')' in call".to_string(),
                            format!("check call {} near line {}", name, line),
                        ));
                    }
                    Ok(Expr::Call(name, args, line))
                } else {
                    Ok(Expr::Var(name, line))
                }
            }
            Some(c) => Err(self.err(
                format!("unexpected character '{}'", c),
                "check for a missing quote or bracket".to_string(),
            )),
            None => Err(self.err(
                "unexpected end of file".to_string(),
                "the last block may be missing '}'".to_string(),
            )),
        }
    }

    fn is_string_prefix_ahead(&self) -> bool {
        let mut tmp = self.pos;
        let mut seen_f = false;
        let mut seen_r = false;
        for _ in 0..2 {
            if let Some(c) = self.chars.get(tmp).copied() {
                if (c == 'f' || c == 'F') && !seen_f {
                    seen_f = true;
                    tmp += 1;
                } else if (c == 'r' || c == 'R') && !seen_r {
                    seen_r = true;
                    tmp += 1;
                } else {
                    break;
                }
            }
        }
        if !seen_f && !seen_r {
            return false;
        }
        matches!(self.chars.get(tmp).copied(), Some('"') | Some('\''))
    }

    fn parse_string(&mut self) -> Result<Expr, ClangError> {
        let line = self.line;
        let mut is_f = false;
        let mut is_raw = false;
        for _ in 0..2 {
            match self.peek() {
                Some('f') | Some('F') if !is_f => {
                    let save = self.pos;
                    self.next();
                    if matches!(self.peek(), Some('"') | Some('\'')) {
                        is_f = true;
                    } else if matches!(self.peek(), Some('r') | Some('R'))
                        && matches!(
                            self.chars.get(self.pos + 1).copied(),
                            Some('"') | Some('\'')
                        )
                    {
                        is_f = true;
                    } else {
                        self.pos = save;
                        break;
                    }
                }
                Some('r') | Some('R') if !is_raw => {
                    let save = self.pos;
                    self.next();
                    if matches!(self.peek(), Some('"') | Some('\'')) {
                        is_raw = true;
                    } else if matches!(self.peek(), Some('f') | Some('F'))
                        && matches!(
                            self.chars.get(self.pos + 1).copied(),
                            Some('"') | Some('\'')
                        )
                    {
                        is_raw = true;
                    } else {
                        self.pos = save;
                        break;
                    }
                }
                _ => break,
            }
        }
        let quote = match self.next() {
            Some(q @ '"') | Some(q @ '\'') => q,
            _ => {
                return Err(self.err(
                    "expected opening quote for string".to_string(),
                    "wrap text in double quotes".to_string(),
                ))
            }
        };
        let mut s = String::new();
        loop {
            match self.next() {
                None => {
                    return Err(self.err(
                        "string was never closed".to_string(),
                        "add the closing quote".to_string(),
                    ))
                }
                Some(c) if c == quote => break,
                Some('\\') if !is_raw => match self.next() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('"') => s.push('"'),
                    Some('\'') => s.push('\''),
                    Some('\\') => s.push('\\'),
                    Some('0') => s.push('\0'),
                    Some(o) => {
                        s.push('\\');
                        s.push(o);
                    }
                    None => {
                        return Err(self.err(
                            "string ends with a lone backslash".to_string(),
                            "remove the trailing backslash".to_string(),
                        ))
                    }
                },
                Some(c) => {
                    if c == '\n' {
                        return Err(self.err(
                            "string runs over two lines".to_string(),
                            "close the quote before the line break".to_string(),
                        ));
                    }
                    s.push(c);
                }
            }
        }
        Ok(Expr::Str {
            val: s,
            is_f,
            is_raw,
            line,
        })
    }

    fn parse_number(&mut self) -> Result<Expr, ClangError> {
        let line = self.line;
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(self.next().unwrap());
            } else {
                break;
            }
        }
        if self.peek() == Some('.') {
            let after_dot = self.peek2();
            if matches!(after_dot, Some(d) if d.is_ascii_digit()) {
                s.push(self.next().unwrap());
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        s.push(self.next().unwrap());
                    } else {
                        break;
                    }
                }
            }
        }
        match s.parse::<f64>() {
            Ok(n) => Ok(Expr::Num(n, line)),
            Err(_) => Err(self.err(
                format!("bad number '{}'", s),
                "use digits like 12 or 12.5".to_string(),
            )),
        }
    }

    pub fn expr_line(e: &Expr) -> usize {
        match e {
            Expr::Str { line, .. } => *line,
            Expr::Num(_, l) => *l,
            Expr::Bool(_, l) => *l,
            Expr::Null(l) => *l,
            Expr::List(_, l) => *l,
            Expr::Var(_, l) => *l,
            Expr::Call(_, _, l) => *l,
            Expr::Attr(_, _, l) => *l,
            Expr::Index(_, _, l) => *l,
            Expr::Slice { line, .. } => *line,
            Expr::BinOp(_, _, _, l) => *l,
            Expr::Neg(_, l) => *l,
        }
    }
}
