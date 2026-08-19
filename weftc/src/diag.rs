// Structured diagnostics [W40]: every error is JSON citing a spec rule.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diag {
    pub rule: String,
    pub message: String,
    pub span: Span,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub hint: Option<String>,
}

impl Diag {
    pub fn new(rule: &str, message: impl Into<String>, span: Span) -> Self {
        Diag {
            rule: rule.to_string(),
            message: message.into(),
            span,
            expected: None,
            actual: None,
            hint: None,
        }
    }

    pub fn expected(mut self, e: impl Into<String>) -> Self {
        self.expected = Some(e.into());
        self
    }

    pub fn actual(mut self, a: impl Into<String>) -> Self {
        self.actual = Some(a.into());
        self
    }

    pub fn hint(mut self, h: impl Into<String>) -> Self {
        self.hint = Some(h.into());
        self
    }

    pub fn to_json(&self, src: &str) -> String {
        let (line, col) = line_col(src, self.span.start);
        let (eline, ecol) = line_col(src, self.span.end);
        let mut s = String::from("{");
        push_field(&mut s, "rule", &self.rule);
        s.push(',');
        push_field(&mut s, "message", &self.message);
        s.push_str(&format!(
            ",\"span\":{{\"line\":{},\"col\":{},\"endLine\":{},\"endCol\":{}}}",
            line, col, eline, ecol
        ));
        if let Some(e) = &self.expected {
            s.push(',');
            push_field(&mut s, "expected", e);
        }
        if let Some(a) = &self.actual {
            s.push(',');
            push_field(&mut s, "actual", a);
        }
        if let Some(h) = &self.hint {
            s.push(',');
            push_field(&mut s, "hint", h);
        }
        s.push('}');
        s
    }

    pub fn render_human(&self, src: &str, file: &str) -> String {
        let (line, col) = line_col(src, self.span.start);
        let mut out = format!("error[{}]: {}\n  --> {}:{}:{}", self.rule, self.message, file, line, col);
        if let Some(src_line) = src.lines().nth(line - 1) {
            out.push_str(&format!("\n   | {}", src_line));
        }
        if let Some(e) = &self.expected {
            out.push_str(&format!("\n   expected: {}", e));
        }
        if let Some(a) = &self.actual {
            out.push_str(&format!("\n   actual:   {}", a));
        }
        if let Some(h) = &self.hint {
            out.push_str(&format!("\n   hint: {}", h));
        }
        out
    }
}

fn push_field(s: &mut String, key: &str, val: &str) {
    s.push('"');
    s.push_str(key);
    s.push_str("\":\"");
    for c in val.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\t' => s.push_str("\\t"),
            '\r' => s.push_str("\\r"),
            c if (c as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", c as u32)),
            c => s.push(c),
        }
    }
    s.push('"');
}

pub fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, c) in src.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
