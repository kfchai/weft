// AST for Weft, spans throughout for diagnostics.

use crate::diag::Span;

#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    /// `type Name = ...` — alias [W11] or variant declaration [W12]
    TypeDef(TypeDef),
    /// `def name(params) -> R = expr` or `def name: T = expr` [W16]
    Def(Def),
    /// `test "name" = expr` [W34] / `test "name" (params) = expr` [W35]
    Test(Test),
}

#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: String,
    pub decl: TypeDecl,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeDecl {
    Alias(TypeExpr),
    Variants(Vec<VariantDef>),
    /// nominal record with an invariant [W42]
    Nominal {
        fields: Vec<(String, TypeExpr)>,
        invariant: Expr,
    },
}

#[derive(Debug, Clone)]
pub struct VariantDef {
    pub name: String,
    pub payload: Vec<TypeExpr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Def {
    pub name: String,
    pub tparams: Vec<String>,
    /// None for constants (`def name: T = expr`)
    pub params: Option<Vec<Param>>,
    /// Return type for functions; declared type for constants
    pub ty: TypeExpr,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    /// `where` contract [W17]
    pub contract: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Test {
    pub name: String,
    pub params: Vec<Param>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeExpr {
    /// `Int`, `List[T]`, `User` — name plus type arguments
    Name(String, Vec<TypeExpr>, Span),
    /// `{f: T, ...}` structural record [W11]
    Record(Vec<(String, TypeExpr)>, Span),
    /// `(A, B) -> C` [W10]
    Fn(Vec<TypeExpr>, Box<TypeExpr>, Span),
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            TypeExpr::Name(_, _, s) => *s,
            TypeExpr::Record(_, s) => *s,
            TypeExpr::Fn(_, _, s) => *s,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Concat, // ++
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
    Unit,
    List(Vec<Expr>),
    Var(String),
    /// Variant construction: `Circle(2.0)`, `None` [W12]
    Ctor(String, Vec<Expr>),
    /// `{f: e, ...}` with optional spread `{..base, f: e}` [W11]
    Record {
        spread: Option<Box<Expr>>,
        fields: Vec<(String, Expr)>,
    },
    /// `Account{f: e, ...}` / `Account{..base, f: e}` — nominal construction [W42]
    NamedRec {
        name: String,
        spread: Option<Box<Expr>>,
        fields: Vec<(String, Expr)>,
    },
    Field(Box<Expr>, String),
    Call(Box<Expr>, Vec<Expr>),
    Lambda {
        params: Vec<(String, Option<TypeExpr>)>,
        body: Box<Expr>,
    },
    /// `{ let a = e; ...; tail }` [W21]
    Block {
        lets: Vec<(String, Expr)>,
        tail: Box<Expr>,
    },
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<(Pattern, Expr)>,
    },
    Bin(BinOp, Box<Expr>, Box<Expr>),
    NotOp(Box<Expr>),
    /// unary minus [W25]
    NegOp(Box<Expr>),
    /// `expr?` [W26]
    Propagate(Box<Expr>),
    /// `?name` [W27]
    Hole(String),
}

/// Compact source-like rendering of an expression, used in diagnostics
/// (e.g. showing a violated contract [W28]). Blocks and matches abbreviate.
pub fn expr_text(e: &Expr) -> String {
    fn atom(e: &Expr) -> String {
        match &e.kind {
            ExprKind::Bin(_, _, _) | ExprKind::If { .. } | ExprKind::Lambda { .. } => {
                format!("({})", expr_text(e))
            }
            _ => expr_text(e),
        }
    }
    match &e.kind {
        ExprKind::Int(v) => v.to_string(),
        ExprKind::Float(f) => {
            let s = format!("{}", f);
            if s.contains('.') || s.contains('e') { s } else { format!("{}.0", s) }
        }
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Text(s) => format!("{:?}", s),
        ExprKind::Unit => "unit".into(),
        ExprKind::Var(n) => n.clone(),
        ExprKind::Hole(n) => format!("?{}", n),
        ExprKind::Ctor(n, args) if args.is_empty() => n.clone(),
        ExprKind::Ctor(n, args) => {
            let inner: Vec<String> = args.iter().map(expr_text).collect();
            format!("{}({})", n, inner.join(", "))
        }
        ExprKind::List(es) => {
            let inner: Vec<String> = es.iter().map(expr_text).collect();
            format!("[{}]", inner.join(", "))
        }
        ExprKind::Record { spread, fields } => {
            let mut parts = Vec::new();
            if let Some(b) = spread {
                parts.push(format!("..{}", atom(b)));
            }
            for (n, v) in fields {
                parts.push(format!("{}: {}", n, expr_text(v)));
            }
            format!("{{{}}}", parts.join(", "))
        }
        ExprKind::NamedRec { name, spread, fields } => {
            let mut parts = Vec::new();
            if let Some(b) = spread {
                parts.push(format!("..{}", atom(b)));
            }
            for (n, v) in fields {
                parts.push(format!("{}: {}", n, expr_text(v)));
            }
            format!("{}{{{}}}", name, parts.join(", "))
        }
        ExprKind::Field(b, f) => format!("{}.{}", atom(b), f),
        ExprKind::Call(c, args) => {
            let inner: Vec<String> = args.iter().map(expr_text).collect();
            format!("{}({})", atom(c), inner.join(", "))
        }
        ExprKind::Lambda { params, body } => {
            let ps: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
            format!("({}) => {}", ps.join(", "), expr_text(body))
        }
        ExprKind::Block { .. } => "{ ... }".into(),
        ExprKind::If { cond, then, els } => {
            format!("if {} then {} else {}", expr_text(cond), expr_text(then), expr_text(els))
        }
        ExprKind::Match { scrutinee, .. } => format!("match {} {{ ... }}", expr_text(scrutinee)),
        ExprKind::Bin(op, l, r) => {
            let sym = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Rem => "%",
                BinOp::Concat => "++",
                BinOp::Eq => "==",
                BinOp::Ne => "!=",
                BinOp::Lt => "<",
                BinOp::Le => "<=",
                BinOp::Gt => ">",
                BinOp::Ge => ">=",
                BinOp::And => "and",
                BinOp::Or => "or",
            };
            format!("{} {} {}", atom(l), sym, atom(r))
        }
        ExprKind::NotOp(i) => format!("not {}", atom(i)),
        ExprKind::NegOp(i) => format!("-{}", atom(i)),
        ExprKind::Propagate(i) => format!("{}?", atom(i)),
    }
}

#[derive(Debug, Clone)]
pub struct Pattern {
    pub kind: PatKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PatKind {
    Wildcard,
    Bind(String),
    LitInt(i64),
    LitFloat(f64),
    LitBool(bool),
    LitText(String),
    /// `Circle(p1, p2)` / bare `None`
    Ctor(String, Vec<Pattern>),
    /// `[p1, p2]` exact, or `[p1, ..rest]` — rest binder is `Some(name)` / `Some("_")` [W23]
    List {
        heads: Vec<Pattern>,
        rest: Option<String>,
    },
}
