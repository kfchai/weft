// Recursive-descent parser for Weft.

use crate::ast::*;
use crate::diag::{Diag, Span};
use crate::lexer::{Tok, Token};

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
    /// true while parsing a match scrutinee, where `Upper {` must be read as
    /// the start of match arms, not a nominal-record construction [W42]
    no_struct: bool,
}

type PResult<T> = Result<T, Diag>;

impl Parser {
    pub fn new(toks: Vec<Token>) -> Self {
        Parser { toks, pos: 0, no_struct: false }
    }

    /// Run a sub-parse with the struct-literal restriction lifted (inside
    /// parens, brackets, or braces the ambiguity is gone).
    fn allow_structs<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.no_struct;
        self.no_struct = false;
        let out = f(self);
        self.no_struct = saved;
        out
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, off: usize) -> &Tok {
        let i = (self.pos + off).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.peek() == tok {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, tok: Tok, what: &str, rule: &str) -> PResult<Token> {
        if self.peek() == &tok {
            Ok(self.bump())
        } else {
            Err(Diag::new(
                rule,
                format!("expected {}, found {}", what, describe(self.peek())),
                self.span(),
            ))
        }
    }

    // ---- program ----

    pub fn parse_program(&mut self) -> PResult<Program> {
        let mut items = Vec::new();
        loop {
            match self.peek() {
                Tok::Eof => break,
                Tok::Type => items.push(Item::TypeDef(self.parse_typedef()?)),
                Tok::Def => items.push(Item::Def(self.parse_def()?)),
                Tok::Test => items.push(Item::Test(self.parse_test()?)),
                _ => {
                    return Err(Diag::new(
                        "W7",
                        format!(
                            "expected a top-level form (`type`, `def`, or `test`), found {}",
                            describe(self.peek())
                        ),
                        self.span(),
                    ));
                }
            }
        }
        Ok(Program { items })
    }

    // ---- type definitions ----

    fn parse_typedef(&mut self) -> PResult<TypeDef> {
        let start = self.span();
        self.bump(); // `type`
        let name = self.expect_upper("a type name (CamelCase)", "W5")?;
        self.expect(Tok::Assign, "`=`", "W11")?;

        // Variants iff the RHS is Upper followed by `(`/`|`/end-of-decl, i.e. not
        // a type application like `List[...]` used as an alias, and not a record.
        let is_variants = match self.peek() {
            Tok::Upper(_) => !matches!(self.peek_at(1), Tok::LBracket),
            _ => false,
        };

        // nominal record with invariant [W42]: `type N = { ... } where expr`
        if self.peek() == &Tok::LBrace {
            let rec = self.parse_type()?;
            if self.eat(&Tok::Where) {
                let invariant = self.parse_expr()?;
                let fields = match rec {
                    TypeExpr::Record(fs, _) => fs,
                    _ => unreachable!("LBrace type is a record"),
                };
                return Ok(TypeDef {
                    name,
                    decl: TypeDecl::Nominal { fields, invariant },
                    span: start.merge(self.prev_span()),
                });
            }
            return Ok(TypeDef {
                name,
                decl: TypeDecl::Alias(rec),
                span: start.merge(self.prev_span()),
            });
        }

        if is_variants {
            let mut variants = Vec::new();
            loop {
                let vstart = self.span();
                let vname = self.expect_upper("a variant name", "W12")?;
                let mut payload = Vec::new();
                if self.eat(&Tok::LParen) {
                    loop {
                        payload.push(self.parse_type()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(Tok::RParen, "`)`", "W12")?;
                }
                variants.push(VariantDef {
                    name: vname,
                    payload,
                    span: vstart.merge(self.prev_span()),
                });
                if !self.eat(&Tok::Pipe) {
                    break;
                }
            }
            Ok(TypeDef {
                name,
                decl: TypeDecl::Variants(variants),
                span: start.merge(self.prev_span()),
            })
        } else {
            let ty = self.parse_type()?;
            Ok(TypeDef {
                name,
                decl: TypeDecl::Alias(ty),
                span: start.merge(self.prev_span()),
            })
        }
    }

    // ---- defs ----

    fn parse_def(&mut self) -> PResult<Def> {
        let start = self.span();
        self.bump(); // `def`
        let name = self.expect_ident("a definition name (snake_case)", "W16")?;

        let mut tparams = Vec::new();
        if self.eat(&Tok::LBracket) {
            loop {
                tparams.push(self.expect_upper("a type parameter", "W13")?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(Tok::RBracket, "`]`", "W13")?;
        }

        if self.eat(&Tok::Colon) {
            // constant: `def name: T = expr`
            if !tparams.is_empty() {
                return Err(Diag::new("W16", "constants cannot have type parameters", start));
            }
            let ty = self.parse_type()?;
            self.expect(Tok::Assign, "`=`", "W16")?;
            let body = self.parse_expr()?;
            return Ok(Def {
                name,
                tparams,
                params: None,
                ty,
                body,
                span: start.merge(self.prev_span()),
            });
        }

        self.expect(Tok::LParen, "`(` or `:` after the definition name", "W16")?;
        let mut params = Vec::new();
        if self.peek() != &Tok::RParen {
            loop {
                params.push(self.parse_param()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(Tok::RParen, "`)`", "W16")?;
        self.expect(Tok::Arrow, "`->` and a return type", "W18")?;
        let ty = self.parse_type()?;
        self.expect(Tok::Assign, "`=`", "W16")?;
        let body = self.parse_expr()?;
        Ok(Def {
            name,
            tparams,
            params: Some(params),
            ty,
            body,
            span: start.merge(self.prev_span()),
        })
    }

    fn parse_param(&mut self) -> PResult<Param> {
        let start = self.span();
        let name = self.expect_ident("a parameter name", "W16")?;
        self.expect(Tok::Colon, "`:` and a parameter type", "W18")?;
        let ty = self.parse_type()?;
        let contract = if self.eat(&Tok::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        Ok(Param {
            name,
            ty,
            contract,
            span: start.merge(self.prev_span()),
        })
    }

    // ---- tests ----

    fn parse_test(&mut self) -> PResult<Test> {
        let start = self.span();
        self.bump(); // `test`
        let name = match self.bump() {
            Token { tok: Tok::Text(s), .. } => s,
            t => {
                return Err(Diag::new(
                    "W34",
                    format!("expected a test name in quotes, found {}", describe(&t.tok)),
                    t.span,
                ));
            }
        };
        let mut params = Vec::new();
        if self.eat(&Tok::LParen) {
            loop {
                params.push(self.parse_param()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(Tok::RParen, "`)`", "W35")?;
        }
        self.expect(Tok::Assign, "`=`", "W34")?;
        let body = self.parse_expr()?;
        Ok(Test {
            name,
            params,
            body,
            span: start.merge(self.prev_span()),
        })
    }

    // ---- types ----

    fn parse_type(&mut self) -> PResult<TypeExpr> {
        let start = self.span();
        match self.peek().clone() {
            Tok::Upper(name) => {
                self.bump();
                let mut args = Vec::new();
                if self.eat(&Tok::LBracket) {
                    loop {
                        args.push(self.parse_type()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(Tok::RBracket, "`]`", "W9")?;
                }
                Ok(TypeExpr::Name(name, args, start.merge(self.prev_span())))
            }
            Tok::LBrace => {
                self.bump();
                let mut fields = Vec::new();
                if self.peek() != &Tok::RBrace {
                    loop {
                        let fname = self.expect_ident("a field name", "W11")?;
                        self.expect(Tok::Colon, "`:`", "W11")?;
                        let fty = self.parse_type()?;
                        fields.push((fname, fty));
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                        if self.peek() == &Tok::RBrace {
                            break; // trailing comma
                        }
                    }
                }
                self.expect(Tok::RBrace, "`}`", "W11")?;
                Ok(TypeExpr::Record(fields, start.merge(self.prev_span())))
            }
            Tok::LParen => {
                // function type `(A, B) -> C` [W10]
                self.bump();
                let mut args = Vec::new();
                if self.peek() != &Tok::RParen {
                    loop {
                        args.push(self.parse_type()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                }
                self.expect(Tok::RParen, "`)`", "W10")?;
                self.expect(Tok::Arrow, "`->` in a function type", "W10")?;
                let ret = self.parse_type()?;
                Ok(TypeExpr::Fn(args, Box::new(ret), start.merge(self.prev_span())))
            }
            other => Err(Diag::new(
                "W9",
                format!("expected a type, found {}", describe(&other)),
                start,
            )),
        }
    }

    // ---- expressions, precedence climbing [W25] ----

    pub fn parse_expr(&mut self) -> PResult<Expr> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_and()?;
        while self.peek() == &Tok::Or {
            self.bump();
            let rhs = self.parse_and()?;
            let span = lhs.span.merge(rhs.span);
            lhs = Expr { kind: ExprKind::Bin(BinOp::Or, Box::new(lhs), Box::new(rhs)), span };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_cmp()?;
        while self.peek() == &Tok::And {
            self.bump();
            let rhs = self.parse_cmp()?;
            let span = lhs.span.merge(rhs.span);
            lhs = Expr { kind: ExprKind::Bin(BinOp::And, Box::new(lhs), Box::new(rhs)), span };
        }
        Ok(lhs)
    }

    fn parse_cmp(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_add()?;
        loop {
            let op = match self.peek() {
                Tok::EqEq => BinOp::Eq,
                Tok::Ne => BinOp::Ne,
                Tok::Lt => BinOp::Lt,
                Tok::Le => BinOp::Le,
                Tok::Gt => BinOp::Gt,
                Tok::Ge => BinOp::Ge,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_add()?;
            let span = lhs.span.merge(rhs.span);
            lhs = Expr { kind: ExprKind::Bin(op, Box::new(lhs), Box::new(rhs)), span };
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                Tok::PlusPlus => BinOp::Concat,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_mul()?;
            let span = lhs.span.merge(rhs.span);
            lhs = Expr { kind: ExprKind::Bin(op, Box::new(lhs), Box::new(rhs)), span };
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> PResult<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Rem,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_unary()?;
            let span = lhs.span.merge(rhs.span);
            lhs = Expr { kind: ExprKind::Bin(op, Box::new(lhs), Box::new(rhs)), span };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        let start = self.span();
        if self.peek() == &Tok::Not {
            self.bump();
            let inner = self.parse_unary()?;
            let span = start.merge(inner.span);
            return Ok(Expr { kind: ExprKind::NotOp(Box::new(inner)), span });
        }
        // negative literal, or unary minus [W25]
        if self.peek() == &Tok::Minus {
            match self.peek_at(1).clone() {
                Tok::Int(v) => {
                    self.bump();
                    let t = self.bump();
                    return Ok(Expr {
                        kind: ExprKind::Int(-v),
                        span: start.merge(t.span),
                    });
                }
                Tok::Float(f) => {
                    self.bump();
                    let t = self.bump();
                    return Ok(Expr {
                        kind: ExprKind::Float(-f),
                        span: start.merge(t.span),
                    });
                }
                _ => {
                    self.bump();
                    let inner = self.parse_unary()?;
                    let span = start.merge(inner.span);
                    return Ok(Expr { kind: ExprKind::NegOp(Box::new(inner)), span });
                }
            }
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> PResult<Expr> {
        let mut e = self.parse_primary()?;
        loop {
            match self.peek() {
                Tok::LParen => {
                    self.bump();
                    let mut args = Vec::new();
                    if self.peek() != &Tok::RParen {
                        loop {
                            let arg = self.allow_structs(|p| p.parse_expr())?;
                            args.push(arg);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(Tok::RParen, "`)`", "W19")?;
                    let span = e.span.merge(self.prev_span());
                    // A call on a bare constructor is variant construction [W12].
                    if let ExprKind::Ctor(name, existing) = &e.kind {
                        if existing.is_empty() {
                            let name = name.clone();
                            e = Expr { kind: ExprKind::Ctor(name, args), span };
                            continue;
                        }
                    }
                    e = Expr { kind: ExprKind::Call(Box::new(e), args), span };
                }
                Tok::Dot => {
                    self.bump();
                    let field = self.expect_ident("a field name after `.`", "W11")?;
                    let span = e.span.merge(self.prev_span());
                    e = Expr { kind: ExprKind::Field(Box::new(e), field), span };
                }
                Tok::Question => {
                    self.bump();
                    let span = e.span.merge(self.prev_span());
                    e = Expr { kind: ExprKind::Propagate(Box::new(e)), span };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        let start = self.span();
        match self.peek().clone() {
            Tok::Int(v) => {
                self.bump();
                Ok(Expr { kind: ExprKind::Int(v), span: start })
            }
            Tok::Float(f) => {
                self.bump();
                Ok(Expr { kind: ExprKind::Float(f), span: start })
            }
            Tok::Text(s) => {
                self.bump();
                Ok(Expr { kind: ExprKind::Text(s), span: start })
            }
            Tok::True => {
                self.bump();
                Ok(Expr { kind: ExprKind::Bool(true), span: start })
            }
            Tok::False => {
                self.bump();
                Ok(Expr { kind: ExprKind::Bool(false), span: start })
            }
            Tok::UnitLit => {
                self.bump();
                Ok(Expr { kind: ExprKind::Unit, span: start })
            }
            Tok::Hole(name) => {
                self.bump();
                Ok(Expr { kind: ExprKind::Hole(name), span: start })
            }
            Tok::Ident(name) => {
                self.bump();
                Ok(Expr { kind: ExprKind::Var(name), span: start })
            }
            Tok::Upper(name) => {
                self.bump();
                // Nominal record construction `Name{...}` [W42], unless we are
                // parsing a match scrutinee where `{` opens the arms.
                if self.peek() == &Tok::LBrace && !self.no_struct {
                    return self.parse_named_rec(name, start);
                }
                // Bare constructor; a following `(` is handled in parse_postfix.
                Ok(Expr { kind: ExprKind::Ctor(name, Vec::new()), span: start })
            }
            Tok::If => {
                self.bump();
                let cond = self.parse_expr()?;
                self.expect(Tok::Then, "`then`", "W22")?;
                let then = self.parse_expr()?;
                self.expect(Tok::Else, "`else` (the else branch is required)", "W22")?;
                let els = self.parse_expr()?;
                let span = start.merge(els.span);
                Ok(Expr {
                    kind: ExprKind::If { cond: Box::new(cond), then: Box::new(then), els: Box::new(els) },
                    span,
                })
            }
            Tok::Match => {
                self.bump();
                let saved = self.no_struct;
                self.no_struct = true;
                let scrutinee = self.parse_expr();
                self.no_struct = saved;
                let scrutinee = scrutinee?;
                self.expect(Tok::LBrace, "`{` to open match arms", "W23")?;
                let mut arms = Vec::new();
                while self.peek() != &Tok::RBrace {
                    let pat = self.parse_pattern()?;
                    self.expect(Tok::FatArrow, "`=>`", "W23")?;
                    let body = self.allow_structs(|p| p.parse_expr())?;
                    arms.push((pat, body));
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RBrace, "`}` to close match arms", "W23")?;
                if arms.is_empty() {
                    return Err(Diag::new("W24", "match must have at least one arm", start));
                }
                let span = start.merge(self.prev_span());
                Ok(Expr {
                    kind: ExprKind::Match { scrutinee: Box::new(scrutinee), arms },
                    span,
                })
            }
            Tok::LBracket => {
                self.bump();
                let mut elems = Vec::new();
                if self.peek() != &Tok::RBracket {
                    loop {
                        let elem = self.allow_structs(|p| p.parse_expr())?;
                        elems.push(elem);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                        if self.peek() == &Tok::RBracket {
                            break; // trailing comma
                        }
                    }
                }
                self.expect(Tok::RBracket, "`]`", "W6")?;
                let span = start.merge(self.prev_span());
                Ok(Expr { kind: ExprKind::List(elems), span })
            }
            Tok::LBrace => self.allow_structs(|p| p.parse_brace(start)),
            Tok::LParen => {
                if self.lambda_ahead() {
                    return self.parse_lambda(start);
                }
                self.bump();
                let inner = self.allow_structs(|p| p.parse_expr())?;
                self.expect(Tok::RParen, "`)`", "W25")?;
                Ok(inner)
            }
            other => Err(Diag::new(
                "W25",
                format!("expected an expression, found {}", describe(&other)),
                start,
            )),
        }
    }

    /// `Name{f: e, ...}` or `Name{..base, f: e, ...}` [W42].
    fn parse_named_rec(&mut self, name: String, start: Span) -> PResult<Expr> {
        self.expect(Tok::LBrace, "`{`", "W42")?;
        self.allow_structs(|p| {
            let spread = if p.peek() == &Tok::DotDot {
                p.bump();
                Some(Box::new(p.parse_expr()?))
            } else {
                None
            };
            let mut fields = Vec::new();
            let mut first = spread.is_none();
            loop {
                if p.peek() == &Tok::RBrace {
                    break;
                }
                if !first {
                    if !p.eat(&Tok::Comma) {
                        break;
                    }
                    if p.peek() == &Tok::RBrace {
                        break; // trailing comma
                    }
                }
                first = false;
                let fname = p.expect_ident("a field name", "W42")?;
                p.expect(Tok::Colon, "`:`", "W42")?;
                let val = p.parse_expr()?;
                fields.push((fname, val));
            }
            p.expect(Tok::RBrace, "`}`", "W42")?;
            let span = start.merge(p.prev_span());
            Ok(Expr { kind: ExprKind::NamedRec { name, spread, fields }, span })
        })
    }

    /// At `{`: decide block vs record literal [W11]/[W21].
    fn parse_brace(&mut self, start: Span) -> PResult<Expr> {
        self.bump(); // `{`
        // record spread `{..base, ...}`
        if self.peek() == &Tok::DotDot {
            self.bump();
            let base = self.parse_expr()?;
            let mut fields = Vec::new();
            while self.eat(&Tok::Comma) {
                if self.peek() == &Tok::RBrace {
                    break;
                }
                let fname = self.expect_ident("a field name", "W11")?;
                self.expect(Tok::Colon, "`:`", "W11")?;
                let val = self.parse_expr()?;
                fields.push((fname, val));
            }
            self.expect(Tok::RBrace, "`}`", "W11")?;
            let span = start.merge(self.prev_span());
            return Ok(Expr {
                kind: ExprKind::Record { spread: Some(Box::new(base)), fields },
                span,
            });
        }
        // record literal iff `ident :` (or empty record `{}`)
        let is_record = match (self.peek(), self.peek_at(1)) {
            (Tok::RBrace, _) => true,
            (Tok::Ident(_), Tok::Colon) => true,
            _ => false,
        };
        if is_record {
            let mut fields = Vec::new();
            if self.peek() != &Tok::RBrace {
                loop {
                    let fname = self.expect_ident("a field name", "W11")?;
                    self.expect(Tok::Colon, "`:`", "W11")?;
                    let val = self.parse_expr()?;
                    fields.push((fname, val));
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                    if self.peek() == &Tok::RBrace {
                        break; // trailing comma
                    }
                }
            }
            self.expect(Tok::RBrace, "`}`", "W11")?;
            let span = start.merge(self.prev_span());
            return Ok(Expr { kind: ExprKind::Record { spread: None, fields }, span });
        }
        // block: `let` statements then a tail expression [W21]
        let mut lets = Vec::new();
        loop {
            if self.peek() == &Tok::Let {
                self.bump();
                let name = if self.eat(&Tok::Underscore) {
                    "_".to_string()
                } else {
                    self.expect_ident("a name (or `_`) after `let`", "W21")?
                };
                self.expect(Tok::Assign, "`=`", "W21")?;
                let val = self.parse_expr()?;
                self.expect(Tok::Semi, "`;` after a let statement", "W21")?;
                lets.push((name, val));
            } else {
                let tail = self.parse_expr()?;
                self.expect(Tok::RBrace, "`}` after the block's final expression", "W21")?;
                let span = start.merge(self.prev_span());
                return Ok(Expr {
                    kind: ExprKind::Block { lets, tail: Box::new(tail) },
                    span,
                });
            }
        }
    }

    /// Look ahead from a `(` to see whether it starts a lambda [W20]:
    /// scan to the matching `)` and check the next token is `=>`.
    fn lambda_ahead(&self) -> bool {
        debug_assert_eq!(self.peek(), &Tok::LParen);
        let mut depth = 0usize;
        let mut i = self.pos;
        while i < self.toks.len() {
            match &self.toks[i].tok {
                Tok::LParen | Tok::LBracket | Tok::LBrace => depth += 1,
                Tok::RParen | Tok::RBracket | Tok::RBrace => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return matches!(self.toks.get(i + 1).map(|t| &t.tok), Some(Tok::FatArrow));
                    }
                }
                Tok::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    fn parse_lambda(&mut self, start: Span) -> PResult<Expr> {
        self.bump(); // `(`
        let mut params = Vec::new();
        if self.peek() != &Tok::RParen {
            loop {
                let pname = if self.eat(&Tok::Underscore) {
                    "_".to_string()
                } else {
                    self.expect_ident("a lambda parameter name", "W20")?
                };
                let ty = if self.eat(&Tok::Colon) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                params.push((pname, ty));
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(Tok::RParen, "`)`", "W20")?;
        self.expect(Tok::FatArrow, "`=>`", "W20")?;
        let body = self.parse_expr()?;
        let span = start.merge(body.span);
        Ok(Expr {
            kind: ExprKind::Lambda { params, body: Box::new(body) },
            span,
        })
    }

    // ---- patterns [W23] ----

    fn parse_pattern(&mut self) -> PResult<Pattern> {
        let start = self.span();
        match self.peek().clone() {
            Tok::Underscore => {
                self.bump();
                Ok(Pattern { kind: PatKind::Wildcard, span: start })
            }
            Tok::Ident(name) => {
                self.bump();
                Ok(Pattern { kind: PatKind::Bind(name), span: start })
            }
            Tok::Int(v) => {
                self.bump();
                Ok(Pattern { kind: PatKind::LitInt(v), span: start })
            }
            Tok::Minus => match self.peek_at(1).clone() {
                Tok::Int(v) => {
                    self.bump();
                    let t = self.bump();
                    Ok(Pattern { kind: PatKind::LitInt(-v), span: start.merge(t.span) })
                }
                Tok::Float(f) => {
                    self.bump();
                    let t = self.bump();
                    Ok(Pattern { kind: PatKind::LitFloat(-f), span: start.merge(t.span) })
                }
                _ => Err(Diag::new("W23", "`-` in a pattern may only prefix a number literal", start)),
            },
            Tok::Float(f) => {
                self.bump();
                Ok(Pattern { kind: PatKind::LitFloat(f), span: start })
            }
            Tok::Text(s) => {
                self.bump();
                Ok(Pattern { kind: PatKind::LitText(s), span: start })
            }
            Tok::True => {
                self.bump();
                Ok(Pattern { kind: PatKind::LitBool(true), span: start })
            }
            Tok::False => {
                self.bump();
                Ok(Pattern { kind: PatKind::LitBool(false), span: start })
            }
            Tok::Upper(name) => {
                self.bump();
                let mut sub = Vec::new();
                if self.eat(&Tok::LParen) {
                    loop {
                        sub.push(self.parse_pattern()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(Tok::RParen, "`)`", "W23")?;
                }
                let span = start.merge(self.prev_span());
                Ok(Pattern { kind: PatKind::Ctor(name, sub), span })
            }
            Tok::LBracket => {
                self.bump();
                let mut heads = Vec::new();
                let mut rest: Option<String> = None;
                if self.peek() != &Tok::RBracket {
                    loop {
                        if self.peek() == &Tok::DotDot {
                            self.bump();
                            let binder = if self.eat(&Tok::Underscore) {
                                "_".to_string()
                            } else {
                                self.expect_ident("a binder (or `_`) after `..`", "W23")?
                            };
                            rest = Some(binder);
                            break;
                        }
                        heads.push(self.parse_pattern()?);
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                }
                self.expect(Tok::RBracket, "`]`", "W23")?;
                let span = start.merge(self.prev_span());
                Ok(Pattern { kind: PatKind::List { heads, rest }, span })
            }
            other => Err(Diag::new(
                "W23",
                format!("expected a pattern, found {}", describe(&other)),
                start,
            )),
        }
    }

    // ---- small helpers ----

    fn expect_ident(&mut self, what: &str, rule: &str) -> PResult<String> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                self.bump();
                Ok(s)
            }
            other => Err(Diag::new(
                rule,
                format!("expected {}, found {}", what, describe(&other)),
                self.span(),
            )),
        }
    }

    fn expect_upper(&mut self, what: &str, rule: &str) -> PResult<String> {
        match self.peek().clone() {
            Tok::Upper(s) => {
                self.bump();
                Ok(s)
            }
            other => Err(Diag::new(
                rule,
                format!("expected {}, found {}", what, describe(&other)),
                self.span(),
            )),
        }
    }
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Ident(s) => format!("`{}`", s),
        Tok::Upper(s) => format!("`{}`", s),
        Tok::Int(v) => format!("`{}`", v),
        Tok::Float(f) => format!("`{}`", f),
        Tok::Text(_) => "a Text literal".to_string(),
        Tok::Hole(n) => format!("hole `?{}`", n),
        Tok::Eof => "end of file".to_string(),
        Tok::Def => "`def`".to_string(),
        Tok::Type => "`type`".to_string(),
        Tok::Test => "`test`".to_string(),
        Tok::Let => "`let`".to_string(),
        Tok::If => "`if`".to_string(),
        Tok::Then => "`then`".to_string(),
        Tok::Else => "`else`".to_string(),
        Tok::Match => "`match`".to_string(),
        Tok::Where => "`where`".to_string(),
        Tok::And => "`and`".to_string(),
        Tok::Or => "`or`".to_string(),
        Tok::Not => "`not`".to_string(),
        Tok::True => "`true`".to_string(),
        Tok::False => "`false`".to_string(),
        Tok::UnitLit => "`unit`".to_string(),
        Tok::LParen => "`(`".to_string(),
        Tok::RParen => "`)`".to_string(),
        Tok::LBracket => "`[`".to_string(),
        Tok::RBracket => "`]`".to_string(),
        Tok::LBrace => "`{`".to_string(),
        Tok::RBrace => "`}`".to_string(),
        Tok::Comma => "`,`".to_string(),
        Tok::Semi => "`;`".to_string(),
        Tok::Colon => "`:`".to_string(),
        Tok::Dot => "`.`".to_string(),
        Tok::DotDot => "`..`".to_string(),
        Tok::Arrow => "`->`".to_string(),
        Tok::FatArrow => "`=>`".to_string(),
        Tok::Assign => "`=`".to_string(),
        Tok::EqEq => "`==`".to_string(),
        Tok::Ne => "`!=`".to_string(),
        Tok::Lt => "`<`".to_string(),
        Tok::Le => "`<=`".to_string(),
        Tok::Gt => "`>`".to_string(),
        Tok::Ge => "`>=`".to_string(),
        Tok::Plus => "`+`".to_string(),
        Tok::Minus => "`-`".to_string(),
        Tok::Star => "`*`".to_string(),
        Tok::Slash => "`/`".to_string(),
        Tok::Percent => "`%`".to_string(),
        Tok::PlusPlus => "`++`".to_string(),
        Tok::Question => "`?`".to_string(),
        Tok::Pipe => "`|`".to_string(),
        Tok::Underscore => "`_`".to_string(),
    }
}
