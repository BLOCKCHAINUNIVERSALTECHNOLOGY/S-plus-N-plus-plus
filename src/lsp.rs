//! Minimal Language Server Protocol (LSP) for S+N++.
//!
//! Supports (stdio JSON-RPC):
//! - `initialize` / `initialized` / `shutdown` / `exit`
//! - `textDocument/didOpen` / `didChange` / `didClose`
//! - `textDocument/hover` — keyword / builtin hints
//! - publishes diagnostics on open/change via `check_program`
//!
//! Start: `snp lsp`

use crate::parser;
use crate::sema;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

#[derive(Clone, Default)]
struct DocState {
    text: String,
}

pub fn run() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut docs: HashMap<String, DocState> = HashMap::new();
    let mut reader = stdin.lock();
    let mut shutdown = false;

    loop {
        let msg = match read_message(&mut reader)? {
            Some(m) => m,
            None => break,
        };

        let method = msg
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Json::Object(HashMap::new()));

        match method.as_str() {
            "initialize" => {
                let result = Json::Object(HashMap::from([
                    (
                        "capabilities".into(),
                        Json::Object(HashMap::from([
                            (
                                "textDocumentSync".into(),
                                Json::Object(HashMap::from([
                                    ("openClose".into(), Json::Bool(true)),
                                    ("change".into(), Json::Number(1.0)), // full
                                ])),
                            ),
                            ("hoverProvider".into(), Json::Bool(true)),
                        ])),
                    ),
                    (
                        "serverInfo".into(),
                        Json::Object(HashMap::from([
                            ("name".into(), Json::String("snp-lsp".into())),
                            ("version".into(), Json::String(crate::VERSION.into())),
                        ])),
                    ),
                ]));
                write_response(&mut stdout, id, result)?;
            }
            "initialized" | "exit" => {
                if method == "exit" {
                    break;
                }
            }
            "shutdown" => {
                shutdown = true;
                write_response(&mut stdout, id, Json::Null)?;
            }
            "textDocument/didOpen" => {
                if let Some((uri, text)) = extract_doc_open(&params) {
                    docs.insert(uri.clone(), DocState { text: text.clone() });
                    publish_diagnostics(&mut stdout, &uri, &text)?;
                }
            }
            "textDocument/didChange" => {
                if let Some((uri, text)) = extract_doc_change(&params) {
                    docs.insert(uri.clone(), DocState { text: text.clone() });
                    publish_diagnostics(&mut stdout, &uri, &text)?;
                }
            }
            "textDocument/didClose" => {
                if let Some(uri) = params
                    .get_path(&["textDocument", "uri"])
                    .and_then(|v| v.as_str())
                {
                    docs.remove(uri);
                    // clear diagnostics
                    let empty = Json::Array(vec![]);
                    notify(
                        &mut stdout,
                        "textDocument/publishDiagnostics",
                        Json::Object(HashMap::from([
                            ("uri".into(), Json::String(uri.into())),
                            ("diagnostics".into(), empty),
                        ])),
                    )?;
                }
            }
            "textDocument/hover" => {
                let uri = params
                    .get_path(&["textDocument", "uri"])
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let line = params
                    .get_path(&["position", "line"])
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as usize;
                let character = params
                    .get_path(&["position", "character"])
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as usize;
                let text = docs.get(uri).map(|d| d.text.as_str()).unwrap_or("");
                let word = word_at(text, line, character);
                let hover = hover_text(&word);
                let result = if hover.is_empty() {
                    Json::Null
                } else {
                    Json::Object(HashMap::from([(
                        "contents".into(),
                        Json::Object(HashMap::from([
                            ("kind".into(), Json::String("markdown".into())),
                            ("value".into(), Json::String(hover)),
                        ])),
                    )]))
                };
                write_response(&mut stdout, id, result)?;
            }
            "" if id.is_some() && shutdown => {
                // ignore
            }
            _ => {
                // Respond null to unknown requests with id
                if id.is_some() {
                    write_response(&mut stdout, id, Json::Null)?;
                }
            }
        }
    }
    Ok(())
}

fn publish_diagnostics(out: &mut impl Write, uri: &str, text: &str) -> io::Result<()> {
    let diags = analyze(text);
    notify(
        out,
        "textDocument/publishDiagnostics",
        Json::Object(HashMap::from([
            ("uri".into(), Json::String(uri.into())),
            ("diagnostics".into(), Json::Array(diags)),
        ])),
    )
}

fn analyze(text: &str) -> Vec<Json> {
    let mut out = Vec::new();
    match parser::parse_program(text) {
        Ok(mut prog) => {
            if let Err(e) = crate::stdlib::resolve(&prog.imports) {
                out.push(diag(1, 0, &e));
                return out;
            }
            if let Err(e) = sema::expand_trait_defaults(&mut prog) {
                out.push(diag(1, 0, &e));
                return out;
            }
            if let Err(e) = sema::check_program(&prog) {
                out.push(diag(1, 0, &e));
            }
        }
        Err(e) => {
            // try extract line from "line N, column M"
            let (line, col) = parse_line_col(&e);
            out.push(diag(line, col, &e));
        }
    }
    out
}

fn parse_line_col(msg: &str) -> (u32, u32) {
    // formats: "(line X, column Y)" or "line X"
    let mut line = 1u32;
    let mut col = 0u32;
    if let Some(i) = msg.find("line ") {
        let rest = &msg[i + 5..];
        if let Some(num) = rest
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u32>()
            .ok()
        {
            line = num;
        }
        if let Some(j) = rest.find("column ") {
            let r2 = &rest[j + 7..];
            if let Some(num) = r2
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u32>()
                .ok()
            {
                col = num.saturating_sub(1);
            }
        }
    }
    (line.saturating_sub(1), col)
}

fn diag(line: u32, col: u32, message: &str) -> Json {
    Json::Object(HashMap::from([
        (
            "range".into(),
            Json::Object(HashMap::from([
                (
                    "start".into(),
                    Json::Object(HashMap::from([
                        ("line".into(), Json::Number(line as f64)),
                        ("character".into(), Json::Number(col as f64)),
                    ])),
                ),
                (
                    "end".into(),
                    Json::Object(HashMap::from([
                        ("line".into(), Json::Number(line as f64)),
                        ("character".into(), Json::Number((col + 1) as f64)),
                    ])),
                ),
            ])),
        ),
        ("severity".into(), Json::Number(1.0)), // Error
        ("source".into(), Json::String("snp".into())),
        ("message".into(), Json::String(message.into())),
    ]))
}

fn word_at(text: &str, line: usize, character: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if line >= lines.len() {
        return String::new();
    }
    let row = lines[line];
    if character > row.len() {
        return String::new();
    }
    let bytes = row.as_bytes();
    let mut start = character.min(bytes.len());
    let mut end = start;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
        start -= 1;
    }
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    row[start..end].to_string()
}

fn hover_text(word: &str) -> String {
    if word.is_empty() {
        return String::new();
    }
    let builtins: &[(&str, &str)] = &[
        ("fn", "**fn** — define a function"),
        ("let", "**let** — bind a local variable"),
        ("struct", "**struct** — product type"),
        ("enum", "**enum** — tagged union"),
        ("trait", "**trait** — interface (0.4)"),
        ("impl", "**impl** — methods / trait implementation"),
        ("match", "**match** — pattern matching"),
        ("mod", "**mod** — nested module"),
        ("pub", "**pub** — public visibility"),
        ("where", "**where** — trait bound (`Type: Trait`)"),
        ("if", "**if** / else control flow"),
        ("while", "**while** loop"),
        ("for", "**for** range / for-in loop"),
        ("return", "**return** from function"),
        ("break", "**break** out of loop"),
        ("continue", "**continue** next iteration"),
        ("true", "`Bool` literal"),
        ("false", "`Bool` literal"),
        ("Int", "integer type"),
        ("String", "string type"),
        ("Bool", "boolean type"),
        ("print", "`print(x)` — write to stdout"),
        ("list", "`list(...)` — create list"),
        ("map", "`map()` — create map"),
        ("Result", "prelude enum: `Result.Ok` / `Result.Err`"),
        ("abs", "`abs(x: Int) -> Int`"),
        ("min", "`min(a, b) -> Int`"),
        ("max", "`max(a, b) -> Int`"),
    ];
    for (k, v) in builtins {
        if *k == word {
            return format!("{v}\n\n_S+N++ {}_", crate::VERSION);
        }
    }
    format!("`{word}`\n\n_S+N++ identifier_")
}

fn extract_doc_open(params: &Json) -> Option<(String, String)> {
    let uri = params.get_path(&["textDocument", "uri"])?.as_str()?.to_string();
    let text = params.get_path(&["textDocument", "text"])?.as_str()?.to_string();
    Some((uri, text))
}

fn extract_doc_change(params: &Json) -> Option<(String, String)> {
    let uri = params.get_path(&["textDocument", "uri"])?.as_str()?.to_string();
    // full sync: contentChanges[0].text
    let text = params
        .get_path(&["contentChanges"])?
        .as_array()?
        .first()?
        .get("text")?
        .as_str()?
        .to_string();
    Some((uri, text))
}

// --- Minimal JSON + LSP framing (no external deps) ---

#[derive(Clone, Debug)]
enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(HashMap<String, Json>),
}

impl Json {
    fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }
    fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }
    fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(a) => Some(a),
            _ => None,
        }
    }
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(m) => m.get(key),
            _ => None,
        }
    }
    fn get_path(&self, path: &[&str]) -> Option<&Json> {
        let mut cur = self;
        for p in path {
            cur = cur.get(p)?;
        }
        Some(cur)
    }
    fn stringify(&self) -> String {
        match self {
            Json::Null => "null".into(),
            Json::Bool(b) => b.to_string(),
            Json::Number(n) => {
                if n.fract() == 0.0 && *n >= i64::MIN as f64 && *n <= i64::MAX as f64 {
                    format!("{}", *n as i64)
                } else {
                    n.to_string()
                }
            }
            Json::String(s) => format!("\"{}\"", escape_json(s)),
            Json::Array(a) => {
                let parts: Vec<String> = a.iter().map(|x| x.stringify()).collect();
                format!("[{}]", parts.join(","))
            }
            Json::Object(m) => {
                let mut parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("\"{}\":{}", escape_json(k), v.stringify()))
                    .collect();
                parts.sort();
                format!("{{{}}}", parts.join(","))
            }
        }
    }
}

fn escape_json(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn parse_json(s: &str) -> Option<Json> {
    let mut i = 0;
    let b = s.as_bytes();
    parse_value(b, &mut i)
}

fn skip_ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && b[*i].is_ascii_whitespace() {
        *i += 1;
    }
}

fn parse_value(b: &[u8], i: &mut usize) -> Option<Json> {
    skip_ws(b, i);
    if *i >= b.len() {
        return None;
    }
    match b[*i] {
        b'n' => {
            *i += 4;
            Some(Json::Null)
        }
        b't' => {
            *i += 4;
            Some(Json::Bool(true))
        }
        b'f' => {
            *i += 5;
            Some(Json::Bool(false))
        }
        b'"' => parse_string(b, i).map(Json::String),
        b'[' => {
            *i += 1;
            let mut arr = Vec::new();
            skip_ws(b, i);
            if *i < b.len() && b[*i] == b']' {
                *i += 1;
                return Some(Json::Array(arr));
            }
            loop {
                arr.push(parse_value(b, i)?);
                skip_ws(b, i);
                if *i < b.len() && b[*i] == b']' {
                    *i += 1;
                    break;
                }
                if *i < b.len() && b[*i] == b',' {
                    *i += 1;
                    continue;
                }
                return None;
            }
            Some(Json::Array(arr))
        }
        b'{' => {
            *i += 1;
            let mut map = HashMap::new();
            skip_ws(b, i);
            if *i < b.len() && b[*i] == b'}' {
                *i += 1;
                return Some(Json::Object(map));
            }
            loop {
                skip_ws(b, i);
                let key = parse_string(b, i)?;
                skip_ws(b, i);
                if *i >= b.len() || b[*i] != b':' {
                    return None;
                }
                *i += 1;
                let val = parse_value(b, i)?;
                map.insert(key, val);
                skip_ws(b, i);
                if *i < b.len() && b[*i] == b'}' {
                    *i += 1;
                    break;
                }
                if *i < b.len() && b[*i] == b',' {
                    *i += 1;
                    continue;
                }
                return None;
            }
            Some(Json::Object(map))
        }
        b'-' | b'0'..=b'9' => {
            let start = *i;
            if b[*i] == b'-' {
                *i += 1;
            }
            while *i < b.len() && b[*i].is_ascii_digit() {
                *i += 1;
            }
            if *i < b.len() && b[*i] == b'.' {
                *i += 1;
                while *i < b.len() && b[*i].is_ascii_digit() {
                    *i += 1;
                }
            }
            let n: f64 = std::str::from_utf8(&b[start..*i]).ok()?.parse::<f64>().ok()?;
            Some(Json::Number(n))
        }
        _ => None,
    }
}

fn parse_string(b: &[u8], i: &mut usize) -> Option<String> {
    if *i >= b.len() || b[*i] != b'"' {
        return None;
    }
    *i += 1;
    let mut s = String::new();
    while *i < b.len() {
        match b[*i] {
            b'"' => {
                *i += 1;
                return Some(s);
            }
            b'\\' => {
                *i += 1;
                if *i >= b.len() {
                    return None;
                }
                match b[*i] {
                    b'"' => s.push('"'),
                    b'\\' => s.push('\\'),
                    b'n' => s.push('\n'),
                    b'r' => s.push('\r'),
                    b't' => s.push('\t'),
                    b'u' => {
                        // skip 4 hex
                        *i += 1;
                        let hex = std::str::from_utf8(b.get(*i..*i + 4)?).ok()?;
                        let cp = u32::from_str_radix(hex, 16).ok()?;
                        if let Some(ch) = char::from_u32(cp) {
                            s.push(ch);
                        }
                        *i += 3;
                    }
                    c => s.push(c as char),
                }
                *i += 1;
            }
            c => {
                s.push(c as char);
                *i += 1;
            }
        }
    }
    None
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Json>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse::<usize>().ok();
        }
    }
    let len = match content_length {
        Some(l) => l,
        None => return Ok(None),
    };
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    let s = String::from_utf8_lossy(&buf);
    Ok(parse_json(&s))
}

fn write_response(out: &mut impl Write, id: Option<Json>, result: Json) -> io::Result<()> {
    let mut obj = HashMap::new();
    obj.insert("jsonrpc".into(), Json::String("2.0".into()));
    if let Some(id) = id {
        obj.insert("id".into(), id);
    }
    obj.insert("result".into(), result);
    write_raw(out, &Json::Object(obj).stringify())
}

fn notify(out: &mut impl Write, method: &str, params: Json) -> io::Result<()> {
    let obj = Json::Object(HashMap::from([
        ("jsonrpc".into(), Json::String("2.0".into())),
        ("method".into(), Json::String(method.into())),
        ("params".into(), params),
    ]));
    write_raw(out, &obj.stringify())
}

fn write_raw(out: &mut impl Write, body: &str) -> io::Result<()> {
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    out.write_all(header.as_bytes())?;
    out.write_all(body.as_bytes())?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_object() {
        let j = parse_json(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#).unwrap();
        assert_eq!(j.get("method").and_then(|x| x.as_str()), Some("initialize"));
    }

    #[test]
    fn hover_fn_keyword() {
        assert!(hover_text("fn").contains("function"));
    }

    #[test]
    fn word_at_ident() {
        let t = "fn main() {\n    let hp = 1;\n}";
        assert_eq!(word_at(t, 1, 8), "hp");
    }

    #[test]
    fn analyze_bad_program() {
        let d = analyze("fn main() -> Int { return missing; }");
        assert!(!d.is_empty());
    }

    #[test]
    fn analyze_ok_program() {
        let d = analyze("fn main() -> Int { return 1; }");
        assert!(d.is_empty());
    }
}
