// Typechecker for Weft. Two passes: collect declarations, then check bodies.
// Unification-based inference for locals and generic instantiation; declared
// types on defs are the source of truth [W18].

use crate::ast::*;
use crate::diag::{Diag, Span};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    Text,
    Unit,
    List(Box<Ty>),
    Opt(Box<Ty>),
    Res(Box<Ty>, Box<Ty>),
    Rec(Vec<(String, Ty)>),
    Fn(Vec<Ty>, Box<Ty>),
    /// user variant type (nominal) [W12]
    Named(String),
    /// capability [W30]
    Cap(String),
    /// rigid type parameter inside a generic def body [W13]
    Rigid(String),
    /// unification variable
    Var(usize),
    /// poison type after an error; unifies with anything to stop cascades
    Err,
}

#[derive(Debug, Clone)]
pub struct Scheme {
    pub tparams: Vec<String>,
    pub params: Vec<Ty>,
    pub ret: Ty,
}

struct Binding {
    name: String,
    ty: Ty,
    is_cap: bool,
    lambda_depth: usize,
}

pub struct HoleNote {
    pub name: String,
    pub ty: String,
    pub span: Span,
}

pub struct CheckResult {
    pub diags: Vec<Diag>,
    pub holes: Vec<HoleNote>,
    pub defs: usize,
    pub tests: usize,
}

pub struct Checker {
    aliases: HashMap<String, TypeExpr>,
    /// variant type name -> ordered ctor list
    variants: HashMap<String, Vec<(String, Vec<Ty>)>>,
    /// nominal record types with invariants [W42]: name -> sorted fields
    nominals: HashMap<String, Vec<(String, Ty)>>,
    /// ctor name -> (owning type, payload)
    ctors: HashMap<String, (String, Vec<Ty>)>,
    sigs: HashMap<String, Scheme>,
    consts: HashMap<String, Ty>,
    builtins: HashMap<String, Scheme>,
    vars: Vec<Option<Ty>>,
    env: Vec<Binding>,
    lambda_depth: usize,
    cur_ret: Option<Ty>,
    cur_tparams: Vec<String>,
    in_contract: bool,
    diags: Vec<Diag>,
    holes: Vec<(String, usize, Span)>, // name, var index, span
}

const CAPS: [&str; 4] = ["Io", "Fs", "Rand", "Clock"];

pub fn check_program(prog: &Program) -> CheckResult {
    let mut c = Checker {
        aliases: HashMap::new(),
        variants: HashMap::new(),
        nominals: HashMap::new(),
        ctors: HashMap::new(),
        sigs: HashMap::new(),
        consts: HashMap::new(),
        builtins: builtin_sigs(),
        vars: Vec::new(),
        env: Vec::new(),
        lambda_depth: 0,
        cur_ret: None,
        cur_tparams: Vec::new(),
        in_contract: false,
        diags: Vec::new(),
        holes: Vec::new(),
    };
    c.collect(prog);
    c.check_bodies(prog);
    let holes = c
        .holes
        .iter()
        .map(|(name, v, span)| HoleNote {
            name: name.clone(),
            ty: c.show(&Ty::Var(*v)),
            span: *span,
        })
        .collect();
    let defs = prog.items.iter().filter(|i| matches!(i, Item::Def(_))).count();
    let tests = prog.items.iter().filter(|i| matches!(i, Item::Test(_))).count();
    CheckResult { diags: c.diags, holes, defs, tests }
}

impl Checker {
    // ---------- pass 1: declarations ----------

    fn collect(&mut self, prog: &Program) {
        let mut names: HashSet<String> = HashSet::new();

        // type declarations first (defs reference them)
        for item in &prog.items {
            if let Item::TypeDef(td) = item {
                if !names.insert(td.name.clone()) {
                    self.diags.push(Diag::new(
                        "W7",
                        format!("duplicate top-level name `{}`", td.name),
                        td.span,
                    ));
                    continue;
                }
                match &td.decl {
                    TypeDecl::Alias(te) => {
                        self.aliases.insert(td.name.clone(), te.clone());
                    }
                    TypeDecl::Variants(_) => {
                        // ctor payload conversion happens below, after all
                        // type names are known
                        self.variants.insert(td.name.clone(), Vec::new());
                    }
                    TypeDecl::Nominal { .. } => {
                        self.nominals.insert(td.name.clone(), Vec::new());
                    }
                }
            }
        }
        // validate alias bodies (cap positions, unknown names, cycles)
        let alias_names: Vec<String> = self.aliases.keys().cloned().collect();
        for name in alias_names {
            let te = self.aliases.get(&name).cloned().unwrap();
            let mut seen = HashSet::new();
            seen.insert(name.clone());
            let _ = self.conv_type(&te, &[], false, &mut seen);
        }
        // nominal record fields [W42]
        for item in &prog.items {
            if let Item::TypeDef(td) = item {
                if let TypeDecl::Nominal { fields, .. } = &td.decl {
                    let mut out = Vec::new();
                    for (n, te) in fields {
                        let mut seen = HashSet::new();
                        out.push((n.clone(), self.conv_type(te, &[], false, &mut seen)));
                    }
                    out.sort_by(|a, b| a.0.cmp(&b.0));
                    self.nominals.insert(td.name.clone(), out);
                }
            }
        }
        // now ctor payloads
        for item in &prog.items {
            if let Item::TypeDef(td) = item {
                if let TypeDecl::Variants(vs) = &td.decl {
                    let mut list = Vec::new();
                    for v in vs {
                        if self.ctors.contains_key(&v.name) || is_builtin_ctor(&v.name) {
                            self.diags.push(Diag::new(
                                "W12",
                                format!("duplicate variant name `{}` (variant names must be unique across the file)", v.name),
                                v.span,
                            ));
                            continue;
                        }
                        let mut payload = Vec::new();
                        for te in &v.payload {
                            let mut seen = HashSet::new();
                            payload.push(self.conv_type(te, &[], false, &mut seen));
                        }
                        self.ctors.insert(v.name.clone(), (td.name.clone(), payload.clone()));
                        list.push((v.name.clone(), payload));
                    }
                    self.variants.insert(td.name.clone(), list);
                }
            }
        }
        // def signatures
        for item in &prog.items {
            match item {
                Item::Def(d) => {
                    if !names.insert(d.name.clone()) {
                        self.diags.push(Diag::new(
                            "W7",
                            format!("duplicate top-level name `{}`", d.name),
                            d.span,
                        ));
                        continue;
                    }
                    // A def may shadow a stdlib name [W7]; user sigs are
                    // consulted before builtins during inference.
                    match &d.params {
                        Some(ps) => {
                            let mut params = Vec::new();
                            for p in ps {
                                let mut seen = HashSet::new();
                                params.push(self.conv_type(&p.ty, &d.tparams, true, &mut seen));
                            }
                            let mut seen = HashSet::new();
                            let ret = self.conv_type(&d.ty, &d.tparams, false, &mut seen);
                            self.sigs.insert(
                                d.name.clone(),
                                Scheme { tparams: d.tparams.clone(), params, ret },
                            );
                        }
                        None => {
                            let mut seen = HashSet::new();
                            let ty = self.conv_type(&d.ty, &[], false, &mut seen);
                            self.consts.insert(d.name.clone(), ty);
                        }
                    }
                }
                Item::Test(_) | Item::TypeDef(_) => {}
            }
        }
        // entry point [W8]
        match self.sigs.get("main") {
            None => {
                let span = prog.items.first().map(|i| item_span(i)).unwrap_or(Span::new(0, 0));
                self.diags.push(
                    Diag::new("W8", "program has no entry point", span)
                        .hint("add `def main(io: Io) -> Int = ...`"),
                );
            }
            Some(s) => {
                let ok = s.tparams.is_empty()
                    && s.params.len() == 1
                    && matches!(&s.params[0], Ty::Cap(c) if c == "Io")
                    && s.ret == Ty::Int;
                if !ok {
                    let span = prog
                        .items
                        .iter()
                        .find_map(|i| match i {
                            Item::Def(d) if d.name == "main" => Some(d.span),
                            _ => None,
                        })
                        .unwrap_or(Span::new(0, 0));
                    self.diags.push(
                        Diag::new("W8", "main must have exactly the signature `def main(io: Io) -> Int`", span),
                    );
                }
            }
        }
    }

    /// Convert a syntactic type to an internal one, expanding aliases and
    /// enforcing capability placement [W33].
    fn conv_type(
        &mut self,
        te: &TypeExpr,
        tparams: &[String],
        allow_cap: bool,
        expanding: &mut HashSet<String>,
    ) -> Ty {
        match te {
            TypeExpr::Name(name, args, span) => {
                if CAPS.contains(&name.as_str()) {
                    if !args.is_empty() {
                        self.diags.push(Diag::new("W30", format!("`{}` takes no type arguments", name), *span));
                        return Ty::Err;
                    }
                    if !allow_cap {
                        self.diags.push(
                            Diag::new(
                                "W33",
                                format!("capability type `{}` may only appear as a function parameter", name),
                                *span,
                            )
                            .hint("capabilities cannot be stored, returned, or captured"),
                        );
                        return Ty::Err;
                    }
                    return Ty::Cap(name.clone());
                }
                if tparams.contains(name) {
                    if !args.is_empty() {
                        self.diags.push(Diag::new("W13", format!("type parameter `{}` takes no arguments", name), *span));
                        return Ty::Err;
                    }
                    return Ty::Rigid(name.clone());
                }
                match (name.as_str(), args.len()) {
                    ("Int", 0) => return Ty::Int,
                    ("Float", 0) => return Ty::Float,
                    ("Bool", 0) => return Ty::Bool,
                    ("Text", 0) => return Ty::Text,
                    ("Unit", 0) => return Ty::Unit,
                    ("List", 1) => {
                        let inner = self.conv_type(&args[0], tparams, false, expanding);
                        return Ty::List(Box::new(inner));
                    }
                    ("Option", 1) => {
                        let inner = self.conv_type(&args[0], tparams, false, expanding);
                        return Ty::Opt(Box::new(inner));
                    }
                    ("Result", 2) => {
                        let a = self.conv_type(&args[0], tparams, false, expanding);
                        let b = self.conv_type(&args[1], tparams, false, expanding);
                        return Ty::Res(Box::new(a), Box::new(b));
                    }
                    ("Int" | "Float" | "Bool" | "Text" | "Unit" | "List" | "Option" | "Result", _) => {
                        self.diags.push(Diag::new("W9", format!("wrong number of type arguments for `{}`", name), *span));
                        return Ty::Err;
                    }
                    _ => {}
                }
                if self.variants.contains_key(name) || self.nominals.contains_key(name) {
                    if !args.is_empty() {
                        self.diags.push(Diag::new("W12", format!("`{}` takes no type arguments", name), *span));
                        return Ty::Err;
                    }
                    return Ty::Named(name.clone());
                }
                if let Some(body) = self.aliases.get(name).cloned() {
                    if !args.is_empty() {
                        self.diags.push(Diag::new("W11", format!("alias `{}` takes no type arguments", name), *span));
                        return Ty::Err;
                    }
                    if !expanding.insert(name.clone()) {
                        self.diags.push(Diag::new("W11", format!("type alias `{}` refers to itself", name), *span));
                        return Ty::Err;
                    }
                    let t = self.conv_type(&body, tparams, allow_cap, expanding);
                    expanding.remove(name);
                    return t;
                }
                self.diags.push(Diag::new("W9", format!("unknown type `{}`", name), *span));
                Ty::Err
            }
            TypeExpr::Record(fields, _) => {
                let mut out = Vec::new();
                for (n, t) in fields {
                    out.push((n.clone(), self.conv_type(t, tparams, false, expanding)));
                }
                out.sort_by(|a, b| a.0.cmp(&b.0));
                Ty::Rec(out)
            }
            TypeExpr::Fn(args, ret, _) => {
                let a: Vec<Ty> = args.iter().map(|t| self.conv_type(t, tparams, false, expanding)).collect();
                let r = self.conv_type(ret, tparams, false, expanding);
                Ty::Fn(a, Box::new(r))
            }
        }
    }

    // ---------- pass 2: bodies ----------

    fn check_bodies(&mut self, prog: &Program) {
        for item in &prog.items {
            match item {
                Item::Def(d) => self.check_def(d),
                Item::Test(t) => self.check_test(t),
                Item::TypeDef(td) => {
                    if let TypeDecl::Nominal { invariant, .. } = &td.decl {
                        self.check_invariant(&td.name, invariant, td.span);
                    }
                }
            }
        }
    }

    /// Validate a nominal type's invariant: fields in scope, pure Bool [W42].
    fn check_invariant(&mut self, tyname: &str, invariant: &Expr, span: Span) {
        if contains_hole(invariant) {
            self.diags.push(Diag::new(
                "W42",
                format!("invariant of `{}` contains a hole; invariants must be complete", tyname),
                span,
            ));
            return;
        }
        self.env.clear();
        self.lambda_depth = 0;
        self.cur_ret = None;
        let fields = self.nominals.get(tyname).cloned().unwrap_or_default();
        for (n, t) in fields {
            self.env.push(Binding { name: n, ty: t, is_cap: false, lambda_depth: 0 });
        }
        self.in_contract = true;
        self.infer(invariant, Some(&Ty::Bool));
        self.in_contract = false;
        self.env.clear();
    }

    fn check_def(&mut self, d: &Def) {
        self.env.clear();
        self.lambda_depth = 0;
        match &d.params {
            Some(ps) => {
                let scheme = match self.sigs.get(&d.name) {
                    Some(s) => s.clone(),
                    None => return, // duplicate; already reported
                };
                self.cur_ret = Some(scheme.ret.clone());
                self.cur_tparams = d.tparams.clone();
                for (i, p) in ps.iter().enumerate() {
                    let ty = scheme.params.get(i).cloned().unwrap_or(Ty::Err);
                    let is_cap = matches!(ty, Ty::Cap(_));
                    self.env.push(Binding { name: p.name.clone(), ty, is_cap, lambda_depth: 0 });
                    // contract sees this and earlier params [W17]
                    if let Some(contract) = &p.contract {
                        if contains_hole(contract) {
                            self.diags.push(Diag::new(
                                "W29",
                                format!("contract on `{}` contains a hole; contracts must be complete", p.name),
                                p.span,
                            ));
                        } else {
                            self.in_contract = true;
                            let ct = self.infer(contract, Some(&Ty::Bool));
                            self.in_contract = false;
                            let _ = ct;
                        }
                    }
                }
                let ret = scheme.ret.clone();
                self.infer(&d.body, Some(&ret));
            }
            None => {
                let ty = match self.consts.get(&d.name) {
                    Some(t) => t.clone(),
                    None => return,
                };
                self.cur_ret = None;
                self.cur_tparams.clear();
                self.infer(&d.body, Some(&ty));
            }
        }
        self.cur_ret = None;
        self.cur_tparams.clear();
    }

    fn check_test(&mut self, t: &Test) {
        self.env.clear();
        self.lambda_depth = 0;
        self.cur_ret = None;
        for p in &t.params {
            let mut seen = HashSet::new();
            let ty = self.conv_type(&p.ty, &[], false, &mut seen);
            if !prop_type_ok(&ty) {
                self.diags.push(
                    Diag::new(
                        "W35",
                        format!("property-test parameter `{}` must be Int, Float, Bool, Text, or a List of these", p.name),
                        p.span,
                    ),
                );
            }
            self.env.push(Binding { name: p.name.clone(), ty, is_cap: false, lambda_depth: 0 });
            if let Some(contract) = &p.contract {
                self.in_contract = true;
                self.infer(contract, Some(&Ty::Bool));
                self.in_contract = false;
            }
        }
        self.infer(&t.body, Some(&Ty::Bool));
    }

    // ---------- inference ----------

    fn fresh(&mut self) -> Ty {
        self.vars.push(None);
        Ty::Var(self.vars.len() - 1)
    }

    fn shallow(&self, t: &Ty) -> Ty {
        let mut cur = t.clone();
        while let Ty::Var(i) = cur {
            match &self.vars[i] {
                Some(next) => cur = next.clone(),
                None => break,
            }
        }
        cur
    }

    /// Fully resolve a type for display / structural checks.
    fn resolve(&self, t: &Ty) -> Ty {
        match self.shallow(t) {
            Ty::List(x) => Ty::List(Box::new(self.resolve(&x))),
            Ty::Opt(x) => Ty::Opt(Box::new(self.resolve(&x))),
            Ty::Res(a, b) => Ty::Res(Box::new(self.resolve(&a)), Box::new(self.resolve(&b))),
            Ty::Rec(fs) => Ty::Rec(fs.iter().map(|(n, x)| (n.clone(), self.resolve(x))).collect()),
            Ty::Fn(args, r) => Ty::Fn(args.iter().map(|x| self.resolve(x)).collect(), Box::new(self.resolve(&r))),
            other => other,
        }
    }

    fn occurs(&self, v: usize, t: &Ty) -> bool {
        match self.shallow(t) {
            Ty::Var(i) => i == v,
            Ty::List(x) | Ty::Opt(x) => self.occurs(v, &x),
            Ty::Res(a, b) => self.occurs(v, &a) || self.occurs(v, &b),
            Ty::Rec(fs) => fs.iter().any(|(_, x)| self.occurs(v, x)),
            Ty::Fn(args, r) => args.iter().any(|x| self.occurs(v, x)) || self.occurs(v, &r),
            _ => false,
        }
    }

    fn unify_raw(&mut self, a: &Ty, b: &Ty) -> bool {
        let a = self.shallow(a);
        let b = self.shallow(b);
        match (&a, &b) {
            (Ty::Err, _) | (_, Ty::Err) => true,
            (Ty::Var(i), _) => {
                if let Ty::Var(j) = b {
                    if *i == j {
                        return true;
                    }
                }
                if self.occurs(*i, &b) {
                    return false;
                }
                self.vars[*i] = Some(b);
                true
            }
            (_, Ty::Var(j)) => {
                if self.occurs(*j, &a) {
                    return false;
                }
                self.vars[*j] = Some(a);
                true
            }
            (Ty::Int, Ty::Int)
            | (Ty::Float, Ty::Float)
            | (Ty::Bool, Ty::Bool)
            | (Ty::Text, Ty::Text)
            | (Ty::Unit, Ty::Unit) => true,
            (Ty::List(x), Ty::List(y)) | (Ty::Opt(x), Ty::Opt(y)) => self.unify_raw(x, y),
            (Ty::Res(a1, b1), Ty::Res(a2, b2)) => self.unify_raw(a1, a2) && self.unify_raw(b1, b2),
            (Ty::Named(x), Ty::Named(y)) => x == y,
            (Ty::Cap(x), Ty::Cap(y)) => x == y,
            (Ty::Rigid(x), Ty::Rigid(y)) => x == y,
            (Ty::Rec(xs), Ty::Rec(ys)) => {
                if xs.len() != ys.len() {
                    return false;
                }
                let mut xs = xs.clone();
                let mut ys = ys.clone();
                xs.sort_by(|p, q| p.0.cmp(&q.0));
                ys.sort_by(|p, q| p.0.cmp(&q.0));
                xs.iter().zip(ys.iter()).all(|((n1, t1), (n2, t2))| n1 == n2 && self.unify_raw(t1, t2))
            }
            (Ty::Fn(a1, r1), Ty::Fn(a2, r2)) => {
                a1.len() == a2.len()
                    && a1.iter().zip(a2.iter()).all(|(x, y)| self.unify_raw(x, y))
                    && self.unify_raw(r1, r2)
            }
            _ => false,
        }
    }

    fn unify(&mut self, expected: &Ty, actual: &Ty, span: Span, rule: &str, ctx: &str) -> Ty {
        if self.unify_raw(expected, actual) {
            self.shallow(expected)
        } else {
            self.diags.push(
                Diag::new(rule, format!("type mismatch {}", ctx), span)
                    .expected(self.show(expected))
                    .actual(self.show(actual)),
            );
            Ty::Err
        }
    }

    pub fn show(&self, t: &Ty) -> String {
        match self.shallow(t) {
            Ty::Int => "Int".into(),
            Ty::Float => "Float".into(),
            Ty::Bool => "Bool".into(),
            Ty::Text => "Text".into(),
            Ty::Unit => "Unit".into(),
            Ty::List(x) => format!("List[{}]", self.show(&x)),
            Ty::Opt(x) => format!("Option[{}]", self.show(&x)),
            Ty::Res(a, b) => format!("Result[{}, {}]", self.show(&a), self.show(&b)),
            Ty::Rec(fs) => {
                let inner: Vec<String> = fs.iter().map(|(n, x)| format!("{}: {}", n, self.show(x))).collect();
                format!("{{{}}}", inner.join(", "))
            }
            Ty::Fn(args, r) => {
                let inner: Vec<String> = args.iter().map(|x| self.show(x)).collect();
                format!("({}) -> {}", inner.join(", "), self.show(&r))
            }
            Ty::Named(n) => n,
            Ty::Cap(c) => c,
            Ty::Rigid(p) => p,
            Ty::Var(_) => "_".into(),
            Ty::Err => "<error>".into(),
        }
    }

    fn instantiate(&mut self, s: &Scheme) -> (Vec<Ty>, Ty) {
        let mut map: HashMap<String, Ty> = HashMap::new();
        for tp in &s.tparams {
            let v = self.fresh();
            map.insert(tp.clone(), v);
        }
        let params = s.params.iter().map(|t| subst(t, &map)).collect();
        let ret = subst(&s.ret, &map);
        (params, ret)
    }

    fn lookup_local(&self, name: &str) -> Option<usize> {
        self.env.iter().rposition(|b| b.name == name)
    }

    fn infer(&mut self, e: &Expr, expected: Option<&Ty>) -> Ty {
        let t = self.infer_inner(e, expected);
        if let Some(exp) = expected {
            self.unify(exp, &t, e.span, "W18", "");
        }
        t
    }

    fn infer_inner(&mut self, e: &Expr, expected: Option<&Ty>) -> Ty {
        match &e.kind {
            ExprKind::Int(_) => Ty::Int,
            ExprKind::Float(_) => Ty::Float,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Text(_) => Ty::Text,
            ExprKind::Unit => Ty::Unit,
            ExprKind::Hole(name) => {
                let v = self.fresh();
                if let Ty::Var(i) = v {
                    self.holes.push((name.clone(), i, e.span));
                }
                v
            }
            ExprKind::List(elems) => {
                let elem = match expected.map(|t| self.shallow(t)) {
                    Some(Ty::List(inner)) => *inner,
                    _ => self.fresh(),
                };
                for el in elems {
                    let t = self.infer(el, Some(&elem));
                    self.forbid_cap_value(&t, el.span, "a list");
                }
                Ty::List(Box::new(elem))
            }
            ExprKind::Var(name) => {
                if let Some(i) = self.lookup_local(name) {
                    let (ty, is_cap, depth) = {
                        let b = &self.env[i];
                        (b.ty.clone(), b.is_cap, b.lambda_depth)
                    };
                    if is_cap && self.in_contract {
                        self.diags.push(Diag::new(
                            "W29",
                            format!("contract uses capability `{}`; contracts must be pure", name),
                            e.span,
                        ));
                    }
                    if is_cap && depth < self.lambda_depth {
                        self.diags.push(
                            Diag::new(
                                "W33",
                                format!("lambda captures capability `{}`", name),
                                e.span,
                            )
                            .hint("pass the capability as a parameter of a named def instead"),
                        );
                    }
                    return ty;
                }
                if let Some(t) = self.consts.get(name) {
                    return t.clone();
                }
                if let Some(s) = self.sigs.get(name).cloned() {
                    let (params, ret) = self.instantiate(&s);
                    return Ty::Fn(params, Box::new(ret));
                }
                if let Some(s) = self.builtins.get(name).cloned() {
                    let (params, ret) = self.instantiate(&s);
                    return Ty::Fn(params, Box::new(ret));
                }
                self.diags.push(Diag::new("W3", format!("unknown name `{}`", name), e.span));
                Ty::Err
            }
            ExprKind::Ctor(name, args) => self.infer_ctor(name, args, e.span, expected),
            ExprKind::Record { spread, fields } => self.infer_record(spread, fields, e.span, expected),
            ExprKind::NamedRec { name, spread, fields } => {
                let decl = match self.nominals.get(name).cloned() {
                    Some(fs) => fs,
                    None => {
                        let rule = if self.variants.contains_key(name) { "W12" } else { "W42" };
                        let msg = if rule == "W12" {
                            format!("`{}` is a variant type; construct it with a variant, not `{}{{...}}`", name, name)
                        } else {
                            format!("unknown nominal type `{}`", name)
                        };
                        self.diags.push(Diag::new(rule, msg, e.span));
                        for (_, v) in fields {
                            self.infer(v, None);
                        }
                        return Ty::Err;
                    }
                };
                match spread {
                    Some(base) => {
                        self.infer(base, Some(&Ty::Named(name.clone())));
                        for (fname, fval) in fields {
                            match decl.iter().find(|(n, _)| n == fname) {
                                Some((_, ft)) => {
                                    self.infer(fval, Some(&ft.clone()));
                                }
                                None => {
                                    self.diags.push(Diag::new(
                                        "W42",
                                        format!("`{}` has no field `{}`", name, fname),
                                        e.span,
                                    ));
                                    self.infer(fval, None);
                                }
                            }
                        }
                    }
                    None => {
                        let mut given = HashSet::new();
                        for (fname, fval) in fields {
                            if !given.insert(fname.clone()) {
                                self.diags.push(Diag::new("W42", format!("duplicate field `{}`", fname), e.span));
                                continue;
                            }
                            match decl.iter().find(|(n, _)| n == fname) {
                                Some((_, ft)) => {
                                    self.infer(fval, Some(&ft.clone()));
                                }
                                None => {
                                    self.diags.push(Diag::new(
                                        "W42",
                                        format!("`{}` has no field `{}`", name, fname),
                                        e.span,
                                    ));
                                    self.infer(fval, None);
                                }
                            }
                        }
                        let missing: Vec<&str> = decl
                            .iter()
                            .map(|(n, _)| n.as_str())
                            .filter(|n| !given.contains(*n))
                            .collect();
                        if !missing.is_empty() {
                            self.diags.push(Diag::new(
                                "W42",
                                format!("`{}{{...}}` is missing field(s): {}", name, missing.join(", ")),
                                e.span,
                            ));
                        }
                    }
                }
                Ty::Named(name.clone())
            }
            ExprKind::Field(base, field) => {
                let bt = self.infer(base, None);
                match self.resolve(&bt) {
                    Ty::Rec(fs) => match fs.iter().find(|(n, _)| n == field) {
                        Some((_, t)) => t.clone(),
                        None => {
                            self.diags.push(
                                Diag::new("W11", format!("record has no field `{}`", field), e.span)
                                    .actual(self.show(&bt)),
                            );
                            Ty::Err
                        }
                    },
                    // nominal records expose their fields too [W42]
                    Ty::Named(tyname) if self.nominals.contains_key(&tyname) => {
                        let fs = self.nominals.get(&tyname).cloned().unwrap_or_default();
                        match fs.iter().find(|(n, _)| n == field) {
                            Some((_, t)) => t.clone(),
                            None => {
                                self.diags.push(
                                    Diag::new("W42", format!("`{}` has no field `{}`", tyname, field), e.span),
                                );
                                Ty::Err
                            }
                        }
                    }
                    Ty::Err => Ty::Err,
                    other => {
                        self.diags.push(
                            Diag::new("W11", format!("`.{}` requires a record", field), e.span)
                                .actual(self.show(&other)),
                        );
                        Ty::Err
                    }
                }
            }
            ExprKind::Call(callee, args) => self.infer_call(callee, args, e.span),
            ExprKind::Lambda { params, body } => {
                // Seed parameter types from annotation or the expected type [W20]
                let expected_fn = expected.map(|t| self.shallow(t));
                let (exp_params, exp_ret) = match &expected_fn {
                    Some(Ty::Fn(ps, r)) if ps.len() == params.len() => (Some(ps.clone()), Some((**r).clone())),
                    _ => (None, None),
                };
                let mut ptys = Vec::new();
                for (i, (pname, ann)) in params.iter().enumerate() {
                    let ty = match ann {
                        Some(te) => {
                            let mut seen = HashSet::new();
                            let tparams = self.cur_tparams.clone();
                            let t = self.conv_type(te, &tparams, false, &mut seen);
                            if let Some(eps) = &exp_params {
                                self.unify(&eps[i], &t, e.span, "W20", "on a lambda parameter");
                            }
                            t
                        }
                        None => match &exp_params {
                            Some(eps) => eps[i].clone(),
                            None => {
                                let v = self.fresh();
                                v
                            }
                        },
                    };
                    ptys.push((pname.clone(), ty));
                }
                let mark = self.env.len();
                self.lambda_depth += 1;
                for (pname, ty) in &ptys {
                    self.env.push(Binding {
                        name: pname.clone(),
                        ty: ty.clone(),
                        is_cap: false,
                        lambda_depth: self.lambda_depth,
                    });
                }
                let rt = match &exp_ret {
                    Some(r) => {
                        self.infer(body, Some(r));
                        r.clone()
                    }
                    None => self.infer(body, None),
                };
                self.env.truncate(mark);
                self.lambda_depth -= 1;
                Ty::Fn(ptys.into_iter().map(|(_, t)| t).collect(), Box::new(rt))
            }
            ExprKind::Block { lets, tail } => {
                let mark = self.env.len();
                for (name, val) in lets {
                    let t = self.infer(val, None);
                    let is_cap = matches!(self.shallow(&t), Ty::Cap(_));
                    self.env.push(Binding {
                        name: name.clone(),
                        ty: t,
                        is_cap,
                        lambda_depth: self.lambda_depth,
                    });
                }
                let t = self.infer(tail, expected);
                self.env.truncate(mark);
                t
            }
            ExprKind::If { cond, then, els } => {
                self.infer(cond, Some(&Ty::Bool));
                let tt = self.infer(then, expected);
                let et = self.infer(els, expected.or(Some(&tt)));
                if expected.is_none() {
                    self.unify(&tt, &et, els.span, "W22", "between if branches");
                }
                tt
            }
            ExprKind::Match { scrutinee, arms } => {
                let st = self.infer(scrutinee, None);
                let result = expected.cloned().unwrap_or_else(|| self.fresh());
                for (pat, body) in arms {
                    let mark = self.env.len();
                    self.check_pattern(pat, &st);
                    self.infer(body, Some(&result));
                    self.env.truncate(mark);
                }
                if let Some(missing) = self.missing_cases(&st, arms) {
                    self.diags.push(
                        Diag::new("W24", "match is not exhaustive", e.span)
                            .hint(format!("unhandled: {}", missing)),
                    );
                }
                result
            }
            ExprKind::Bin(op, l, r) => self.infer_bin(*op, l, r, e.span),
            ExprKind::NotOp(inner) => {
                self.infer(inner, Some(&Ty::Bool));
                Ty::Bool
            }
            ExprKind::NegOp(inner) => {
                let it = self.infer(inner, None);
                match self.shallow(&it) {
                    Ty::Int => Ty::Int,
                    Ty::Float => Ty::Float,
                    Ty::Err => Ty::Err,
                    Ty::Var(_) => {
                        self.unify(&Ty::Int, &it, e.span, "W25", "in unary `-`");
                        Ty::Int
                    }
                    other => {
                        self.diags.push(
                            Diag::new("W25", "unary `-` needs Int or Float", e.span)
                                .actual(self.show(&other)),
                        );
                        Ty::Err
                    }
                }
            }
            ExprKind::Propagate(inner) => {
                let it = self.infer(inner, None);
                let ok = self.fresh();
                let err = self.fresh();
                let want = Ty::Res(Box::new(ok.clone()), Box::new(err.clone()));
                self.unify(&want, &it, inner.span, "W26", "(`?` needs a Result)");
                if self.lambda_depth > 0 {
                    self.diags.push(
                        Diag::new("W26", "`?` cannot be used inside a lambda", e.span)
                            .hint("`?` returns from the enclosing def; use a match here"),
                    );
                    return ok;
                }
                match self.cur_ret.clone() {
                    Some(ret) => {
                        let ru = self.fresh();
                        let want_ret = Ty::Res(Box::new(ru), Box::new(err));
                        let shown = self.show(&ret);
                        if !self.unify_raw(&want_ret, &ret) {
                            self.diags.push(
                                Diag::new(
                                    "W26",
                                    "`?` requires the enclosing def to return a Result with the same Err type",
                                    e.span,
                                )
                                .actual(shown),
                            );
                        }
                    }
                    None => {
                        self.diags.push(Diag::new(
                            "W26",
                            "`?` can only be used inside a function def that returns Result",
                            e.span,
                        ));
                    }
                }
                ok
            }
        }
    }

    fn infer_ctor(&mut self, name: &str, args: &[Expr], span: Span, expected: Option<&Ty>) -> Ty {
        // builtin ctors [W9]
        match name {
            "Some" | "None" | "Ok" | "Err" => {
                let (want_arity, result, payload): (usize, Ty, Vec<Ty>) = match name {
                    "Some" => {
                        let v = match expected.map(|t| self.shallow(t)) {
                            Some(Ty::Opt(inner)) => *inner,
                            _ => self.fresh(),
                        };
                        (1, Ty::Opt(Box::new(v.clone())), vec![v])
                    }
                    "None" => {
                        let v = match expected.map(|t| self.shallow(t)) {
                            Some(Ty::Opt(inner)) => *inner,
                            _ => self.fresh(),
                        };
                        (0, Ty::Opt(Box::new(v)), vec![])
                    }
                    "Ok" => {
                        let (a, b) = match expected.map(|t| self.shallow(t)) {
                            Some(Ty::Res(x, y)) => (*x, *y),
                            _ => (self.fresh(), self.fresh()),
                        };
                        (1, Ty::Res(Box::new(a.clone()), Box::new(b)), vec![a])
                    }
                    _ => {
                        let (a, b) = match expected.map(|t| self.shallow(t)) {
                            Some(Ty::Res(x, y)) => (*x, *y),
                            _ => (self.fresh(), self.fresh()),
                        };
                        (1, Ty::Res(Box::new(a), Box::new(b.clone())), vec![b])
                    }
                };
                if args.len() != want_arity {
                    self.diags.push(Diag::new(
                        "W9",
                        format!("`{}` takes {} argument(s), got {}", name, want_arity, args.len()),
                        span,
                    ));
                    return result;
                }
                for (arg, pt) in args.iter().zip(payload.iter()) {
                    let t = self.infer(arg, Some(pt));
                    self.forbid_cap_value(&t, arg.span, "an Option/Result");
                }
                result
            }
            _ => match self.ctors.get(name).cloned() {
                Some((tyname, payload)) => {
                    if args.len() != payload.len() {
                        self.diags.push(Diag::new(
                            "W12",
                            format!("`{}` takes {} argument(s), got {}", name, payload.len(), args.len()),
                            span,
                        ));
                        return Ty::Named(tyname);
                    }
                    for (arg, pt) in args.iter().zip(payload.iter()) {
                        let t = self.infer(arg, Some(pt));
                        self.forbid_cap_value(&t, arg.span, "a variant");
                    }
                    Ty::Named(tyname)
                }
                None => {
                    self.diags.push(Diag::new("W12", format!("unknown variant `{}`", name), span));
                    Ty::Err
                }
            },
        }
    }

    fn infer_record(
        &mut self,
        spread: &Option<Box<Expr>>,
        fields: &[(String, Expr)],
        span: Span,
        expected: Option<&Ty>,
    ) -> Ty {
        match spread {
            Some(base) => {
                // copy-with-changes: result type is the base's type [W11]
                let bt = self.infer(base, None);
                let resolved = self.resolve(&bt);
                match resolved {
                    Ty::Rec(fs) => {
                        for (fname, fval) in fields {
                            match fs.iter().find(|(n, _)| n == fname) {
                                Some((_, ft)) => {
                                    let t = self.infer(fval, Some(&ft.clone()));
                                    self.forbid_cap_value(&t, fval.span, "a record");
                                }
                                None => {
                                    self.diags.push(Diag::new(
                                        "W11",
                                        format!("`..` base record has no field `{}`", fname),
                                        span,
                                    ));
                                    self.infer(fval, None);
                                }
                            }
                        }
                        Ty::Rec(fs)
                    }
                    Ty::Named(tyname) if self.nominals.contains_key(&tyname) => {
                        self.diags.push(
                            Diag::new(
                                "W42",
                                format!("`{}` is nominal; copy it with `{}{{..base, ...}}`", tyname, tyname),
                                span,
                            )
                            .hint("nominal types keep their invariant checked at every copy"),
                        );
                        Ty::Err
                    }
                    Ty::Err => Ty::Err,
                    other => {
                        self.diags.push(
                            Diag::new("W11", "`..` requires a record", span).actual(self.show(&other)),
                        );
                        Ty::Err
                    }
                }
            }
            None => {
                // Use the expected record type to type fields when available
                let exp_fields: Option<Vec<(String, Ty)>> = match expected.map(|t| self.resolve(t)) {
                    Some(Ty::Rec(fs)) => Some(fs),
                    _ => None,
                };
                let mut out = Vec::new();
                let mut seen = HashSet::new();
                for (fname, fval) in fields {
                    if !seen.insert(fname.clone()) {
                        self.diags.push(Diag::new("W11", format!("duplicate field `{}`", fname), span));
                        continue;
                    }
                    let exp_ft = exp_fields
                        .as_ref()
                        .and_then(|fs| fs.iter().find(|(n, _)| n == fname).map(|(_, t)| t.clone()));
                    let t = match exp_ft {
                        Some(ft) => self.infer(fval, Some(&ft)),
                        None => self.infer(fval, None),
                    };
                    self.forbid_cap_value(&t, fval.span, "a record");
                    out.push((fname.clone(), t));
                }
                out.sort_by(|a, b| a.0.cmp(&b.0));
                Ty::Rec(out)
            }
        }
    }

    fn infer_call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> Ty {
        let ct = self.infer(callee, None);
        match self.shallow(&ct) {
            Ty::Fn(params, ret) => {
                if params.len() != args.len() {
                    self.diags.push(
                        Diag::new(
                            "W19",
                            format!("call expects {} argument(s), got {}", params.len(), args.len()),
                            span,
                        )
                        .expected(self.show(&ct)),
                    );
                    return *ret;
                }
                for (arg, pt) in args.iter().zip(params.iter()) {
                    self.infer(arg, Some(&pt.clone()));
                }
                *ret
            }
            Ty::Err => Ty::Err,
            other => {
                self.diags.push(
                    Diag::new("W19", "this expression is not callable", callee.span)
                        .actual(self.show(&other)),
                );
                for arg in args {
                    self.infer(arg, None);
                }
                Ty::Err
            }
        }
    }

    fn infer_bin(&mut self, op: BinOp, l: &Expr, r: &Expr, span: Span) -> Ty {
        use BinOp::*;
        match op {
            And | Or => {
                self.infer(l, Some(&Ty::Bool));
                self.infer(r, Some(&Ty::Bool));
                Ty::Bool
            }
            Add | Sub | Mul | Div | Rem => {
                let lt = self.infer(l, None);
                self.infer(r, Some(&lt.clone()));
                match self.shallow(&lt) {
                    Ty::Int => Ty::Int,
                    Ty::Float => Ty::Float,
                    Ty::Err => Ty::Err,
                    Ty::Var(_) => {
                        // both sides underdetermined; default to Int
                        self.unify(&Ty::Int, &lt, span, "W25", "in arithmetic");
                        Ty::Int
                    }
                    other => {
                        self.diags.push(
                            Diag::new("W25", "arithmetic needs Int with Int or Float with Float", span)
                                .actual(self.show(&other))
                                .hint("there are no implicit conversions [W15]; use int_to_float / float_to_int"),
                        );
                        Ty::Err
                    }
                }
            }
            Concat => {
                let lt = self.infer(l, None);
                self.infer(r, Some(&lt.clone()));
                match self.shallow(&lt) {
                    Ty::Text => Ty::Text,
                    Ty::List(x) => Ty::List(x),
                    Ty::Err => Ty::Err,
                    Ty::Var(_) => {
                        self.unify(&Ty::Text, &lt, span, "W25", "in `++`");
                        Ty::Text
                    }
                    other => {
                        self.diags.push(
                            Diag::new("W25", "`++` concatenates Text with Text or List with List", span)
                                .actual(self.show(&other)),
                        );
                        Ty::Err
                    }
                }
            }
            Lt | Le | Gt | Ge => {
                let lt = self.infer(l, None);
                self.infer(r, Some(&lt.clone()));
                match self.shallow(&lt) {
                    Ty::Int | Ty::Float | Ty::Err => {}
                    Ty::Var(_) => {
                        self.unify(&Ty::Int, &lt, span, "W25", "in a comparison");
                    }
                    other => {
                        self.diags.push(
                            Diag::new("W25", "ordering comparisons need Int or Float", span)
                                .actual(self.show(&other)),
                        );
                    }
                }
                Ty::Bool
            }
            Eq | Ne => {
                let lt = self.infer(l, None);
                self.infer(r, Some(&lt.clone()));
                let resolved = self.resolve(&lt);
                if contains_fn_or_cap(&resolved) {
                    self.diags.push(
                        Diag::new("W14", "equality is not defined on function or capability types", span)
                            .actual(self.show(&resolved)),
                    );
                }
                Ty::Bool
            }
        }
    }

    fn forbid_cap_value(&mut self, t: &Ty, span: Span, container: &str) {
        if let Ty::Cap(name) = self.shallow(t) {
            self.diags.push(
                Diag::new("W33", format!("capability `{}` cannot be stored in {}", name, container), span)
                    .hint("capabilities may only be passed as arguments or let-bound"),
            );
        }
    }

    // ---------- patterns ----------

    fn check_pattern(&mut self, pat: &Pattern, expected: &Ty) {
        match &pat.kind {
            PatKind::Wildcard => {}
            PatKind::Bind(name) => {
                let is_cap = matches!(self.shallow(expected), Ty::Cap(_));
                self.env.push(Binding {
                    name: name.clone(),
                    ty: expected.clone(),
                    is_cap,
                    lambda_depth: self.lambda_depth,
                });
            }
            PatKind::LitInt(_) => {
                self.unify(expected, &Ty::Int, pat.span, "W23", "in a pattern");
            }
            PatKind::LitFloat(_) => {
                self.unify(expected, &Ty::Float, pat.span, "W23", "in a pattern");
            }
            PatKind::LitBool(_) => {
                self.unify(expected, &Ty::Bool, pat.span, "W23", "in a pattern");
            }
            PatKind::LitText(_) => {
                self.unify(expected, &Ty::Text, pat.span, "W23", "in a pattern");
            }
            PatKind::Ctor(name, subs) => match name.as_str() {
                "Some" => {
                    let inner = self.fresh();
                    self.unify(expected, &Ty::Opt(Box::new(inner.clone())), pat.span, "W23", "in a pattern");
                    if subs.len() == 1 {
                        self.check_pattern(&subs[0], &inner);
                    } else {
                        self.diags.push(Diag::new("W23", "`Some` pattern takes exactly one sub-pattern", pat.span));
                    }
                }
                "None" => {
                    let inner = self.fresh();
                    self.unify(expected, &Ty::Opt(Box::new(inner)), pat.span, "W23", "in a pattern");
                    if !subs.is_empty() {
                        self.diags.push(Diag::new("W23", "`None` pattern takes no sub-patterns", pat.span));
                    }
                }
                "Ok" | "Err" => {
                    let a = self.fresh();
                    let b = self.fresh();
                    self.unify(
                        expected,
                        &Ty::Res(Box::new(a.clone()), Box::new(b.clone())),
                        pat.span,
                        "W23",
                        "in a pattern",
                    );
                    let inner = if name == "Ok" { a } else { b };
                    if subs.len() == 1 {
                        self.check_pattern(&subs[0], &inner);
                    } else {
                        self.diags.push(Diag::new(
                            "W23",
                            format!("`{}` pattern takes exactly one sub-pattern", name),
                            pat.span,
                        ));
                    }
                }
                _ => match self.ctors.get(name).cloned() {
                    Some((tyname, payload)) => {
                        self.unify(expected, &Ty::Named(tyname), pat.span, "W23", "in a pattern");
                        if subs.len() != payload.len() {
                            self.diags.push(Diag::new(
                                "W23",
                                format!("`{}` pattern takes {} sub-pattern(s), got {}", name, payload.len(), subs.len()),
                                pat.span,
                            ));
                            return;
                        }
                        for (sp, pt) in subs.iter().zip(payload.iter()) {
                            self.check_pattern(sp, pt);
                        }
                    }
                    None => {
                        self.diags.push(Diag::new("W12", format!("unknown variant `{}` in pattern", name), pat.span));
                    }
                },
            },
            PatKind::List { heads, rest } => {
                let elem = self.fresh();
                self.unify(expected, &Ty::List(Box::new(elem.clone())), pat.span, "W23", "in a pattern");
                for h in heads {
                    self.check_pattern(h, &elem);
                }
                if let Some(binder) = rest {
                    if binder != "_" {
                        self.env.push(Binding {
                            name: binder.clone(),
                            ty: Ty::List(Box::new(elem)),
                            is_cap: false,
                            lambda_depth: self.lambda_depth,
                        });
                    }
                }
            }
        }
    }

    // ---------- exhaustiveness [W24] ----------

    fn missing_cases(&self, scrut: &Ty, arms: &[(Pattern, Expr)]) -> Option<String> {
        // any irrefutable top-level pattern covers everything
        if arms.iter().any(|(p, _)| irrefutable(p)) {
            return None;
        }
        match self.resolve(scrut) {
            Ty::Named(tyname) => {
                let ctors = self.variants.get(&tyname)?;
                let covered: HashSet<&str> = arms
                    .iter()
                    .filter_map(|(p, _)| match &p.kind {
                        PatKind::Ctor(n, subs) if subs.iter().all(irrefutable) => Some(n.as_str()),
                        _ => None,
                    })
                    .collect();
                let missing: Vec<&str> = ctors
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .filter(|n| !covered.contains(n))
                    .collect();
                if missing.is_empty() {
                    None
                } else {
                    Some(missing.join(", "))
                }
            }
            Ty::Opt(_) => self.variant_like(arms, &["Some", "None"]),
            Ty::Res(_, _) => self.variant_like(arms, &["Ok", "Err"]),
            Ty::Bool => {
                let mut t = false;
                let mut f = false;
                for (p, _) in arms {
                    match p.kind {
                        PatKind::LitBool(true) => t = true,
                        PatKind::LitBool(false) => f = true,
                        _ => {}
                    }
                }
                match (t, f) {
                    (true, true) => None,
                    (true, false) => Some("false".into()),
                    (false, true) => Some("true".into()),
                    (false, false) => Some("true, false".into()),
                }
            }
            Ty::List(_) => {
                // rest arms with irrefutable heads cover all lengths >= heads.len()
                let mut min_rest: Option<usize> = None;
                let mut exact: HashSet<usize> = HashSet::new();
                for (p, _) in arms {
                    if let PatKind::List { heads, rest } = &p.kind {
                        if heads.iter().all(irrefutable) {
                            match rest {
                                Some(_) => {
                                    min_rest = Some(min_rest.map_or(heads.len(), |m: usize| m.min(heads.len())));
                                }
                                None => {
                                    exact.insert(heads.len());
                                }
                            }
                        }
                    }
                }
                match min_rest {
                    Some(k) => {
                        let missing: Vec<String> = (0..k)
                            .filter(|n| !exact.contains(n))
                            .map(|n| format!("lists of length {}", n))
                            .collect();
                        if missing.is_empty() {
                            None
                        } else {
                            Some(missing.join(", "))
                        }
                    }
                    None => Some("longer lists (add a `[x, ..rest]` or `_` arm)".into()),
                }
            }
            Ty::Err | Ty::Var(_) => None, // don't cascade
            _ => Some("add a catch-all `_` arm".into()),
        }
    }

    fn variant_like(&self, arms: &[(Pattern, Expr)], needed: &[&str]) -> Option<String> {
        let covered: HashSet<&str> = arms
            .iter()
            .filter_map(|(p, _)| match &p.kind {
                PatKind::Ctor(n, subs) if subs.iter().all(irrefutable) => Some(n.as_str()),
                _ => None,
            })
            .collect();
        let missing: Vec<&str> = needed.iter().copied().filter(|n| !covered.contains(n)).collect();
        if missing.is_empty() {
            None
        } else {
            Some(missing.join(", "))
        }
    }
}

fn irrefutable(p: &Pattern) -> bool {
    matches!(p.kind, PatKind::Wildcard | PatKind::Bind(_))
}

fn contains_hole(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Hole(_) => true,
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Text(_)
        | ExprKind::Unit
        | ExprKind::Var(_) => false,
        ExprKind::List(es) => es.iter().any(contains_hole),
        ExprKind::Ctor(_, es) => es.iter().any(contains_hole),
        ExprKind::Record { spread, fields } | ExprKind::NamedRec { spread, fields, .. } => {
            spread.as_ref().map_or(false, |b| contains_hole(b)) || fields.iter().any(|(_, v)| contains_hole(v))
        }
        ExprKind::Field(b, _) => contains_hole(b),
        ExprKind::Call(c, args) => contains_hole(c) || args.iter().any(contains_hole),
        ExprKind::Lambda { body, .. } => contains_hole(body),
        ExprKind::Block { lets, tail } => lets.iter().any(|(_, v)| contains_hole(v)) || contains_hole(tail),
        ExprKind::If { cond, then, els } => contains_hole(cond) || contains_hole(then) || contains_hole(els),
        ExprKind::Match { scrutinee, arms } => {
            contains_hole(scrutinee) || arms.iter().any(|(_, b)| contains_hole(b))
        }
        ExprKind::Bin(_, l, r) => contains_hole(l) || contains_hole(r),
        ExprKind::NotOp(i) | ExprKind::NegOp(i) | ExprKind::Propagate(i) => contains_hole(i),
    }
}

fn contains_fn_or_cap(t: &Ty) -> bool {
    match t {
        Ty::Fn(_, _) | Ty::Cap(_) => true,
        Ty::List(x) | Ty::Opt(x) => contains_fn_or_cap(x),
        Ty::Res(a, b) => contains_fn_or_cap(a) || contains_fn_or_cap(b),
        Ty::Rec(fs) => fs.iter().any(|(_, x)| contains_fn_or_cap(x)),
        _ => false,
    }
}

fn prop_type_ok(t: &Ty) -> bool {
    match t {
        Ty::Int | Ty::Float | Ty::Bool | Ty::Text => true,
        Ty::List(x) => matches!(**x, Ty::Int | Ty::Float | Ty::Bool | Ty::Text),
        _ => false,
    }
}

fn is_builtin_ctor(name: &str) -> bool {
    matches!(name, "Some" | "None" | "Ok" | "Err")
}

fn item_span(i: &Item) -> Span {
    match i {
        Item::TypeDef(t) => t.span,
        Item::Def(d) => d.span,
        Item::Test(t) => t.span,
    }
}

fn subst(t: &Ty, map: &HashMap<String, Ty>) -> Ty {
    match t {
        Ty::Rigid(name) => map.get(name).cloned().unwrap_or_else(|| t.clone()),
        Ty::List(x) => Ty::List(Box::new(subst(x, map))),
        Ty::Opt(x) => Ty::Opt(Box::new(subst(x, map))),
        Ty::Res(a, b) => Ty::Res(Box::new(subst(a, map)), Box::new(subst(b, map))),
        Ty::Rec(fs) => Ty::Rec(fs.iter().map(|(n, x)| (n.clone(), subst(x, map))).collect()),
        Ty::Fn(args, r) => Ty::Fn(args.iter().map(|x| subst(x, map)).collect(), Box::new(subst(r, map))),
        other => other.clone(),
    }
}

// ---------- standard library [§10] ----------

fn builtin_sigs() -> HashMap<String, Scheme> {
    let mut m = HashMap::new();
    let a = || Ty::Rigid("A".into());
    let b = || Ty::Rigid("B".into());
    let e = || Ty::Rigid("E".into());
    let list = |t: Ty| Ty::List(Box::new(t));
    let opt = |t: Ty| Ty::Opt(Box::new(t));
    let res = |t: Ty, u: Ty| Ty::Res(Box::new(t), Box::new(u));
    let fun = |args: Vec<Ty>, r: Ty| Ty::Fn(args, Box::new(r));
    let cap = |n: &str| Ty::Cap(n.into());

    let mut add = |name: &str, tps: &[&str], params: Vec<Ty>, ret: Ty| {
        m.insert(
            name.to_string(),
            Scheme { tparams: tps.iter().map(|s| s.to_string()).collect(), params, ret },
        );
    };

    // Text
    add("text_len", &[], vec![Ty::Text], Ty::Int);
    add("text_of_int", &[], vec![Ty::Int], Ty::Text);
    add("text_of_float", &[], vec![Ty::Float], Ty::Text);
    add("text_of_bool", &[], vec![Ty::Bool], Ty::Text);
    add("int_of_text", &[], vec![Ty::Text], opt(Ty::Int));
    add("split", &[], vec![Ty::Text, Ty::Text], list(Ty::Text));
    add("join", &[], vec![list(Ty::Text), Ty::Text], Ty::Text);
    add("contains", &[], vec![Ty::Text, Ty::Text], Ty::Bool);
    add("chars", &[], vec![Ty::Text], list(Ty::Text));
    add("to_upper", &[], vec![Ty::Text], Ty::Text);
    add("to_lower", &[], vec![Ty::Text], Ty::Text);
    add("trim", &[], vec![Ty::Text], Ty::Text);

    // List
    add("len", &["A"], vec![list(a())], Ty::Int);
    add("list_get", &["A"], vec![list(a()), Ty::Int], opt(a()));
    add("append", &["A"], vec![list(a()), a()], list(a()));
    add("map", &["A", "B"], vec![list(a()), fun(vec![a()], b())], list(b()));
    add("filter", &["A"], vec![list(a()), fun(vec![a()], Ty::Bool)], list(a()));
    add("fold", &["A", "B"], vec![list(a()), b(), fun(vec![b(), a()], b())], b());
    add("range", &[], vec![Ty::Int, Ty::Int], list(Ty::Int));
    add("reverse", &["A"], vec![list(a())], list(a()));
    add("sort_by", &["A"], vec![list(a()), fun(vec![a()], Ty::Int)], list(a()));
    add(
        "zip",
        &["A", "B"],
        vec![list(a()), list(b())],
        list(Ty::Rec(vec![("fst".into(), a()), ("snd".into(), b())])),
    );
    add("find", &["A"], vec![list(a()), fun(vec![a()], Ty::Bool)], opt(a()));
    add("index_of", &["A"], vec![list(a()), a()], opt(Ty::Int));

    // Option / Result / math
    add("unwrap_or", &["A"], vec![opt(a()), a()], a());
    add("ok_or", &["A", "E"], vec![opt(a()), e()], res(a(), e()));
    add("abs", &[], vec![Ty::Int], Ty::Int);
    add("min", &[], vec![Ty::Int, Ty::Int], Ty::Int);
    add("max", &[], vec![Ty::Int, Ty::Int], Ty::Int);
    add("int_to_float", &[], vec![Ty::Int], Ty::Float);
    add("float_to_int", &[], vec![Ty::Float], Ty::Int);

    // Effectful [W32]
    add("print", &[], vec![cap("Io"), Ty::Text], Ty::Unit);
    add("read_line", &[], vec![cap("Io")], Ty::Text);
    add("fs", &[], vec![cap("Io")], cap("Fs"));
    add("fs_read", &[], vec![cap("Fs"), Ty::Text], res(Ty::Text, Ty::Text));
    add("fs_write", &[], vec![cap("Fs"), Ty::Text, Ty::Text], res(Ty::Unit, Ty::Text));
    add("rand", &[], vec![cap("Io")], cap("Rand"));
    add("rand_int", &[], vec![cap("Rand"), Ty::Int, Ty::Int], Ty::Int);
    add("clock", &[], vec![cap("Io")], cap("Clock"));
    add("now_ms", &[], vec![cap("Clock")], Ty::Int);

    m
}
