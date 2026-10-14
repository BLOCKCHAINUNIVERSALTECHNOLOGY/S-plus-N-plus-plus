//! Light source formatter for `snp fmt`.

/// Normalize trailing whitespace and ensure a single trailing newline.
pub fn format_source(src: &str) -> String {
    let mut out = String::new();
    for line in src.lines() {
        out.push_str(line.trim_end());
        out.push('\n');
    }
    if out.is_empty() {
        return out;
    }
    while out.contains("\n\n\n\n") {
        out = out.replace("\n\n\n\n", "\n\n\n");
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Extract `///` doc lines paired with the next `fn` / `struct` / `enum` name.
pub fn extract_docs(src: &str) -> Vec<(String, String)> {
    let mut docs: Vec<(String, String)> = Vec::new();
    let mut pending = String::new();
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with("///") {
            let body = t.trim_start_matches('/').trim();
            if !pending.is_empty() {
                pending.push(' ');
            }
            pending.push_str(body);
            continue;
        }
        if pending.is_empty() {
            continue;
        }
        let name = if let Some(rest) = t.strip_prefix("fn ") {
            rest.split(|c: char| c == '(' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_string()
        } else if let Some(rest) = t.strip_prefix("struct ") {
            rest.split(|c: char| c == '{' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_string()
        } else if let Some(rest) = t.strip_prefix("enum ") {
            rest.split(|c: char| c == '{' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_string()
        } else if let Some(rest) = t.strip_prefix("pub fn ") {
            rest.split(|c: char| c == '(' || c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_string()
        } else {
            pending.clear();
            continue;
        };
        if !name.is_empty() {
            docs.push((name, pending.clone()));
        }
        pending.clear();
    }
    docs
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trims_trailing() {
        assert_eq!(format_source("fn main() {  \n"), "fn main() {\n");
    }
    #[test]
    fn extracts_doc() {
        let s = "/// add two numbers\nfn add(a: Int, b: Int) -> Int {\n return a + b;\n}\n";
        let d = extract_docs(s);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].0, "add");
        assert!(d[0].1.contains("add two"));
    }
}
