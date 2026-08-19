// Lexer for Weft. Tokens carry byte spans into the source.

use crate::diag::{Diag, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Upper(String),
    Int(i64),
    Float(f64),
    Text(String),
    Hole(String), // ?name [W27]

    // keywords
    Def,
    Type,
    Test,
    Let,
    If,
    Then,
    Else,
    Match,
    Where,
    And,
    Or,
    Not,
    True,
    False,
    UnitLit,

    // punctuation / operators
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Semi,
    Colon,
    Dot,
    DotDot,
    Arrow,    // ->
    FatArrow, // =>
    Assign,   // =
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    PlusPlus,
    Question,
    Pipe,
    Underscore,

    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub fn lex(src: &str) -> Result<Vec<Token>, Diag> {
    let bytes: Vec<char> = src.chars().collect();
    // Map char index -> byte offset so spans are byte-based for slicing/line math.
    let mut byte_of: Vec<usize> = Vec::with_capacity(bytes.len() + 1);
    {
        let mut off = 0usize;
        for c in &bytes {
            byte_of.push(off);
            off += c.len_utf8();
        }
        byte_of.push(off);
    }

    let mut toks = Vec::new();
    let mut i = 0usize;
    let n = bytes.len();

    macro_rules! span {
        ($s:expr, $e:expr) => {
            Span::new(byte_of[$s], byte_of[$e])
        };
    }

    while i < n {
        let c = bytes[i];
        // whitespace (and a UTF-8 BOM, common on Windows)
        if c == ' ' || c == '\t' || c == '\r' || c == '\n' || c == '\u{feff}' {
            i += 1;
            continue;
        }
        // comment [W4]
        if c == '#' {
            while i < n && bytes[i] != '\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        // identifiers / keywords [W5]
        if c.is_ascii_lowercase() || c == '_' {
            while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == '_') {
                i += 1;
            }
            let word: String = bytes[start..i].iter().collect();
            let tok = match word.as_str() {
                "def" => Tok::Def,
                "type" => Tok::Type,
                "test" => Tok::Test,
                "let" => Tok::Let,
                "if" => Tok::If,
                "then" => Tok::Then,
                "else" => Tok::Else,
                "match" => Tok::Match,
                "where" => Tok::Where,
                "and" => Tok::And,
                "or" => Tok::Or,
                "not" => Tok::Not,
                "true" => Tok::True,
                "false" => Tok::False,
                "unit" => Tok::UnitLit,
                "_" => Tok::Underscore,
                _ => Tok::Ident(word),
            };
            toks.push(Token { tok, span: span!(start, i) });
            continue;
        }
        // type / variant names [W5]
        if c.is_ascii_uppercase() {
            while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == '_') {
                i += 1;
            }
            let word: String = bytes[start..i].iter().collect();
            toks.push(Token { tok: Tok::Upper(word), span: span!(start, i) });
            continue;
        }
        // numbers [W6]
        if c.is_ascii_digit() {
            while i < n && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let mut is_float = false;
            if i + 1 < n && bytes[i] == '.' && bytes[i + 1].is_ascii_digit() {
                is_float = true;
                i += 1;
                while i < n && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let word: String = bytes[start..i].iter().collect();
            let sp = span!(start, i);
            if is_float {
                match word.parse::<f64>() {
                    Ok(f) => toks.push(Token { tok: Tok::Float(f), span: sp }),
                    Err(_) => return Err(Diag::new("W6", format!("invalid float literal `{}`", word), sp)),
                }
            } else {
                match word.parse::<i64>() {
                    Ok(v) => toks.push(Token { tok: Tok::Int(v), span: sp }),
                    Err(_) => return Err(Diag::new("W6", format!("integer literal `{}` out of range", word), sp)),
                }
            }
            continue;
        }
        // text literals with escapes [W6]
        if c == '"' {
            i += 1;
            let mut out = String::new();
            let mut closed = false;
            while i < n {
                let ch = bytes[i];
                if ch == '"' {
                    i += 1;
                    closed = true;
                    break;
                }
                if ch == '\\' {
                    if i + 1 >= n {
                        break;
                    }
                    let esc = bytes[i + 1];
                    match esc {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        other => {
                            return Err(Diag::new(
                                "W6",
                                format!("unknown escape `\\{}` in Text literal", other),
                                span!(i, i + 2),
                            )
                            .hint("valid escapes are \\n, \\t, \\\", \\\\"));
                        }
                    }
                    i += 2;
                    continue;
                }
                if ch == '\n' {
                    break; // unterminated on this line
                }
                out.push(ch);
                i += 1;
            }
            if !closed {
                return Err(Diag::new("W6", "unterminated Text literal", span!(start, i)));
            }
            toks.push(Token { tok: Tok::Text(out), span: span!(start, i) });
            continue;
        }
        // hole [W27]: `?` immediately followed by a lowercase identifier
        if c == '?' && i + 1 < n && (bytes[i + 1].is_ascii_lowercase() || bytes[i + 1] == '_') {
            i += 1;
            let name_start = i;
            while i < n && (bytes[i].is_ascii_alphanumeric() || bytes[i] == '_') {
                i += 1;
            }
            let name: String = bytes[name_start..i].iter().collect();
            toks.push(Token { tok: Tok::Hole(name), span: span!(start, i) });
            continue;
        }

        // operators / punctuation, longest match first
        let two: Option<(Tok, usize)> = if i + 1 < n {
            let pair = (c, bytes[i + 1]);
            match pair {
                ('-', '>') => Some((Tok::Arrow, 2)),
                ('=', '>') => Some((Tok::FatArrow, 2)),
                ('=', '=') => Some((Tok::EqEq, 2)),
                ('!', '=') => Some((Tok::Ne, 2)),
                ('<', '=') => Some((Tok::Le, 2)),
                ('>', '=') => Some((Tok::Ge, 2)),
                ('+', '+') => Some((Tok::PlusPlus, 2)),
                ('.', '.') => Some((Tok::DotDot, 2)),
                _ => None,
            }
        } else {
            None
        };
        if let Some((tok, len)) = two {
            i += len;
            toks.push(Token { tok, span: span!(start, i) });
            continue;
        }
        let one = match c {
            '(' => Some(Tok::LParen),
            ')' => Some(Tok::RParen),
            '[' => Some(Tok::LBracket),
            ']' => Some(Tok::RBracket),
            '{' => Some(Tok::LBrace),
            '}' => Some(Tok::RBrace),
            ',' => Some(Tok::Comma),
            ';' => Some(Tok::Semi),
            ':' => Some(Tok::Colon),
            '.' => Some(Tok::Dot),
            '=' => Some(Tok::Assign),
            '<' => Some(Tok::Lt),
            '>' => Some(Tok::Gt),
            '+' => Some(Tok::Plus),
            '-' => Some(Tok::Minus),
            '*' => Some(Tok::Star),
            '/' => Some(Tok::Slash),
            '%' => Some(Tok::Percent),
            '?' => Some(Tok::Question),
            '|' => Some(Tok::Pipe),
            _ => None,
        };
        match one {
            Some(tok) => {
                i += 1;
                toks.push(Token { tok, span: span!(start, i) });
            }
            None => {
                return Err(Diag::new(
                    "W4",
                    format!("unexpected character `{}`", c),
                    span!(start, start + 1),
                ));
            }
        }
    }

    let end = byte_of[n];
    toks.push(Token { tok: Tok::Eof, span: Span::new(end, end) });
    Ok(toks)
}
