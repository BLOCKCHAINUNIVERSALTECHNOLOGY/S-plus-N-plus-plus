//! Structured diagnostics for S+N++.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
            Severity::Note => write!(f, "note"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourceLoc {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

/// Compute line/column from byte offset in source.
pub fn offset_to_loc(source: &str, offset: usize) -> SourceLoc {
    let mut line = 1usize;
    let mut column = 1usize;
    for (i, ch) in source.chars().enumerate() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    SourceLoc { offset, line, column }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub code: Option<&'static str>,
    pub loc: Option<SourceLoc>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            message: message.into(),
            code: None,
            loc: None,
        }
    }

    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    pub fn at(mut self, loc: SourceLoc) -> Self {
        self.loc = Some(loc);
        self
    }

    pub fn format_with_source(&self, source: &str) -> String {
        let mut out = if let Some(code) = self.code {
            format!("{}[{}]: {}", self.severity, code, self.message)
        } else {
            format!("{}: {}", self.severity, self.message)
        };
        if let Some(loc) = self.loc {
            out.push_str(&format!("\n  --> line {}:{}", loc.line, loc.column));
            if let Some(line) = source.lines().nth(loc.line.saturating_sub(1)) {
                out.push_str(&format!("\n   |\n{:>3} | {}\n   | {:>width$}^", loc.line, line, "", width = loc.column.saturating_sub(1)));
            }
        }
        out
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(code) = self.code {
            write!(f, "{}[{}]: {}", self.severity, code, self.message)
        } else {
            write!(f, "{}: {}", self.severity, self.message)
        }
    }
}

pub fn from_string(err: String) -> Diagnostic {
    Diagnostic::error(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_error() {
        let d = Diagnostic::error("undefined variable `x`").with_code("E0001");
        assert!(d.to_string().contains("error[E0001]"));
    }

    #[test]
    fn locate_finds_backtick() {
        let src = "fn main() -> Int {\n  let y = x;\n  return y;\n}";
        let loc = locate_error(src, "undefined variable `x`");
        assert_eq!(loc.line, 2);
    }

    #[test]
    fn annotate_includes_caret() {
        let src = "fn main() -> Int {\n  return z;\n}";
        let out = annotate(src, "undefined variable `z`");
        assert!(out.contains("-->"), "{out}");
        assert!(out.contains("^"), "{out}");
    }

    #[test]
    fn offset_to_loc_works() {
        let src = "fn main() {\n  let x = 1;\n}";
        let loc = offset_to_loc(src, 15);
        assert!(loc.line >= 2);
    }
}


/// Find a source location for an error string.
/// 1) Explicit "line N, column M" / "line N"
/// 2) First backtick-quoted identifier `` `name` `` occurring in source
/// 3) Fallback: first non-empty line
pub fn locate_error(source: &str, error: &str) -> SourceLoc {
    // explicit line/column
    if let Some(idx) = error.find("line ") {
        let rest = &error[idx + 5..];
        let mut nums = rest
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty());
        if let Some(n) = nums.next() {
            let line = n.parse().unwrap_or(0);
            let mut column = 1usize;
            if rest.contains("column") {
                if let Some(n2) = nums.next() {
                    column = n2.parse().unwrap_or(1);
                }
            }
            if line > 0 {
                return SourceLoc {
                    offset: 0,
                    line,
                    column,
                };
            }
        }
    }
    // backtick identifiers — prefer later ones that look like variables/methods
    let mut candidates: Vec<&str> = Vec::new();
    let mut rest = error;
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        if let Some(end) = after.find('`') {
            let name = &after[..end];
            if !name.is_empty()
                && name != "main"
                && !name.contains(' ')
                && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            {
                candidates.push(name);
            }
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    // search source for last candidate first (often the undefined name)
    for name in candidates.iter().rev() {
        // strip Type.method → method for search if needed
        let needle = name.split('.').last().unwrap_or(name);
        if let Some(pos) = source.find(needle) {
            return offset_to_loc(source, pos);
        }
    }
    // fallback: first non-empty source line
    SourceLoc {
        offset: 0,
        line: 1,
        column: 1,
    }
}

/// Format a multi-line runtime / compile error for CLI display.
pub fn format_error_report(path: &str, source: &str, error: &str) -> String {
    let loc = locate_error(source, error);
    let line_no = loc.line;
    let col = loc.column;
    let mut out = String::new();
    let mut lines = error.lines();
    if let Some(first) = lines.next() {
        let first = first.trim_start_matches("error: ").trim_start_matches("runtime error: ");
        out.push_str("error: ");
        out.push_str(first);
        out.push('\n');
    } else {
        out.push_str("error: ");
        out.push_str(error);
        out.push('\n');
    }
    if line_no > 0 && !source.is_empty() {
        let line_text = source.lines().nth(line_no.saturating_sub(1)).unwrap_or("");
        let caret = format!("{:>width$}^", "", width = col.saturating_sub(1));
        out.push_str(&format!(
            "  --> {path}:{line_no}:{col}\n   |\n{line_no:>3} | {line_text}\n   | {caret}\n"
        ));
    }
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Annotate a plain error with a source snippet (for execute / check).
pub fn annotate(source: &str, error: impl std::fmt::Display) -> String {
    format_error_report("<input>", source, &error.to_string())
}


/// Sema/type error with optional function scope (v0.3.1).
pub fn format_sema_error(function: Option<&str>, message: &str) -> String {
    match function {
        Some(f) => format!("error: in function `{f}`: {message}"),
        None => format!("error: {message}"),
    }
}
