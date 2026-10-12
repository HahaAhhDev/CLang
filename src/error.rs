use std::fmt;

#[derive(Debug, Clone)]
pub struct ClangError {
    pub line: usize,
    pub msg: String,
    pub hint: String,
}

impl ClangError {
    pub fn new(line: usize, msg: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            line,
            msg: msg.into(),
            hint: hint.into(),
        }
    }
}

impl fmt::Display for ClangError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "clang error at line {}: {}\nhelp: {}",
            self.line, self.msg, self.hint
        )
    }
}

impl std::error::Error for ClangError {}
