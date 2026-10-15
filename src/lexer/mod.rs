//! S+N++ lexer and token definitions.

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Fn, Let, If, Else, While, For, In, Return, Break, Continue,
    True, False, Mut, Import, Struct, Match, Mod, Extern, Impl, Enum, Pub, Trait, Where,
    // Literals & identifiers
    Ident(String), Int(i64), Str(String),
    // Operators
    Amp, Plus, Minus, Star, Slash, Bang, Eq, EqEq, BangEq,
    Lt, Lte, Gt, Gte,
    // Delimiters
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Comma, Colon, Dot, DotDot, Arrow, FatArrow, Semi, Pipe,
    // String interpolation: "a{expr}b" produces StrPart / InterpStart / ...
    // For simplicity, full strings with { } are handled in parser via raw Str
    // and a secondary scan. Lexer still emits plain Str.
    Eof,
}

/// Token with source offset for diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub struct SpannedToken {
    pub token: Token,
    pub start: usize,
    pub end: usize,
}

pub fn lex(src: &str) -> Result<Vec<Token>, String> {
    Ok(lex_spanned(src)?.into_iter().map(|s| s.token).collect())
}

pub fn lex_spanned(src: &str) -> Result<Vec<SpannedToken>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();

    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() { i += 1; continue; }
        if c == '#' {
            while i < chars.len() && chars[i] != '\n' { i += 1; }
            continue;
        }
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' { i += 1; }
            continue;
        }
        if c.is_ascii_digit() {
            let s = i;
            while i < chars.len() && chars[i].is_ascii_digit() { i += 1; }
            let text: String = chars[s..i].iter().collect();
            out.push(SpannedToken {
                token: Token::Int(text.parse().map_err(|_| format!("invalid integer `{text}`"))?),
                start: s, end: i,
            });
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let s = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') { i += 1; }
            let word: String = chars[s..i].iter().collect();
            let token = match word.as_str() {
                "fn" => Token::Fn, "let" => Token::Let, "if" => Token::If,
                "else" => Token::Else, "while" => Token::While, "for" => Token::For,
                "in" => Token::In, "return" => Token::Return, "break" => Token::Break,
                "continue" => Token::Continue, "true" => Token::True, "false" => Token::False,
                "mut" => Token::Mut, "import" => Token::Import, "struct" => Token::Struct,
                "match" => Token::Match, "mod" => Token::Mod, "extern" => Token::Extern, "impl" => Token::Impl, "enum" => Token::Enum, "pub" => Token::Pub, "trait" => Token::Trait, "where" => Token::Where,
                _ => Token::Ident(word),
            };
            out.push(SpannedToken { token, start: s, end: i });
            continue;
        }
        if c == '"' {
            let s = i;
            i += 1;
            let mut buf = String::new();
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    match chars[i + 1] {
                        'n' => buf.push('\n'),
                        't' => buf.push('\t'),
                        '"' => buf.push('"'),
                        '\\' => buf.push('\\'),
                        '{' => buf.push('{'),
                        '}' => buf.push('}'),
                        other => buf.push(other),
                    }
                    i += 2;
                } else {
                    buf.push(chars[i]);
                    i += 1;
                }
            }
            if i == chars.len() { return Err("unterminated string literal".into()); }
            i += 1; // closing "
            out.push(SpannedToken { token: Token::Str(buf), start: s, end: i });
            continue;
        }

        let start = i;
        let token = match c {
            '&' => Token::Amp,
            '+' => Token::Plus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '|' => Token::Pipe,
            '(' => Token::LParen,
            ')' => Token::RParen,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '[' => Token::LBracket,
            ']' => Token::RBracket,
            ',' => Token::Comma,
            ':' => Token::Colon,
            ';' => Token::Semi,
            '.' => {
                if i + 1 < chars.len() && chars[i + 1] == '.' { i += 1; Token::DotDot } else { Token::Dot }
            }
            '-' => {
                if i + 1 < chars.len() && chars[i + 1] == '>' { i += 1; Token::Arrow } else { Token::Minus }
            }
            '=' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' { i += 1; Token::EqEq }
                else if i + 1 < chars.len() && chars[i + 1] == '>' { i += 1; Token::FatArrow }
                else { Token::Eq }
            }
            '!' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' { i += 1; Token::BangEq } else { Token::Bang }
            }
            '<' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' { i += 1; Token::Lte } else { Token::Lt }
            }
            '>' => {
                if i + 1 < chars.len() && chars[i + 1] == '=' { i += 1; Token::Gte } else { Token::Gt }
            }
            _ => return Err(format!("unexpected character `{c}` at offset {i}")),
        };
        i += 1;
        out.push(SpannedToken { token, start, end: i });
    }
    out.push(SpannedToken { token: Token::Eof, start: chars.len(), end: chars.len() });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_keywords_and_operators() {
        let tokens = lex("for i in 0..10 { break; continue; !x -y struct match }").unwrap();
        assert!(tokens.contains(&Token::For));
        assert!(tokens.contains(&Token::Struct));
        assert!(tokens.contains(&Token::Match));
        assert!(tokens.contains(&Token::DotDot));
    }

    #[test]
    fn lexes_brackets() {
        let tokens = lex("xs[0]").unwrap();
        assert!(tokens.contains(&Token::LBracket));
    }

    #[test]
    fn lexes_fat_arrow() {
        let tokens = lex("1 => {}").unwrap();
        assert!(tokens.contains(&Token::FatArrow));
    }
}
