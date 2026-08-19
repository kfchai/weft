// Tree-walking evaluator and test runner for Weft.
// Strict, left-to-right [W37]; contracts checked on every call [W28];
// runtime halts carry a structured error citing a rule [W38].

use crate::ast::*;
use crate::diag::{Diag, Span};
use std::collections::HashMap;
use std::io::Write as _;

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
    Unit,
    List(Vec<Value>),
    /// fields kept sorted by name
    Rec(Vec<(String, Value)>),
    /// Some/None/Ok/Err and user variants
    Ctor(String, Vec<Value>),
    Closure {
        params: Vec<String>,
        body: Box<Expr>,
        env: Vec<(String, Value)>,
    },
    NamedFn(String),
    Builtin(String),
    Cap(String),
}

pub struct RunErr {
    pub diag: Diag,
}

enum Flow {
    Halt(RunErr),
    /// early return raised by `?` [W26]
    Ret(Value),
}

type EResult = Result<Value, Flow>;

fn halt(rule: &str, msg: impl Into<String>, span: Span) -> Flow {
    Flow::Halt(RunErr { diag: Diag::new(rule, msg, span) })
}

pub struct Interp {
    defs: HashMap<String, Def>,
    consts: HashMap<String, Def>,
    const_cache: HashMap<String, Value>,
    /// nominal type name -> invariant expression [W42]
    invariants: HashMap<String, Expr>,
    /// all type declarations, for model-reply validation [W43]
    typedefs: HashMap<String, TypeDef>,
    prng: u64,
}

pub struct TestOutcome {
    pub name: String,
    pub passed: bool,
    pub detail: Option<String>, // failure message / counterexample
    pub cases: usize,
    pub span: Span,
    pub property: bool,
}

impl Interp {
    pub fn new(prog: &Program) -> Interp {
        let mut defs = HashMap::new();
        let mut consts = HashMap::new();
        let mut invariants = HashMap::new();
        let mut typedefs = HashMap::new();
        for item in &prog.items {
            match item {
                Item::Def(d) => {
                    if d.params.is_some() {
                        defs.insert(d.name.clone(), d.clone());
                    } else {
                        consts.insert(d.name.clone(), d.clone());
                    }
                }
                Item::TypeDef(td) => {
                    if let TypeDecl::Nominal { invariant, .. } = &td.decl {
                        invariants.insert(td.name.clone(), invariant.clone());
                    }
                    typedefs.insert(td.name.clone(), td.clone());
                }
                Item::Test(_) => {}
            }
        }
        Interp { defs, consts, const_cache: HashMap::new(), invariants, typedefs, prng: 0x9E37_79B9_7F4A_7C15 }
    }

    // ---------- entry points ----------

    pub fn run_main(&mut self) -> Result<i64, RunErr> {
        let main = match self.defs.get("main") {
            Some(d) => d.clone(),
            None => {
                return Err(RunErr {
                    diag: Diag::new("W8", "program has no `main` to run", Span::new(0, 0)),
                })
            }
        };
        match self.call_def(&main, vec![Value::Cap("Io".into())], main.span) {
            Ok(Value::Int(code)) => Ok(code),
            Ok(other) => Err(RunErr {
                diag: Diag::new("W8", format!("main returned a non-Int value: {}", show(&other)), main.span),
            }),
            Err(Flow::Halt(e)) => Err(e),
            Err(Flow::Ret(_)) => unreachable!("`?` cannot escape a def call"),
        }
    }

    pub fn run_tests(&mut self, prog: &Program) -> Vec<TestOutcome> {
        let mut out = Vec::new();
        for item in &prog.items {
            let t = match item {
                Item::Test(t) => t,
                _ => continue,
            };
            if t.params.is_empty() {
                out.push(self.run_unit_test(t));
            } else {
                out.push(self.run_property_test(t));
            }
        }
        out
    }

    fn run_unit_test(&mut self, t: &Test) -> TestOutcome {
        let outcome = |passed: bool, detail: Option<String>| TestOutcome {
            name: t.name.clone(),
            passed,
            detail,
            cases: 1,
            span: t.span,
            property: false,
        };
        let mut env = Vec::new();
        match self.eval(&t.body, &mut env) {
            Ok(Value::Bool(true)) => outcome(true, None),
            Ok(Value::Bool(false)) => outcome(false, Some("returned false".into())),
            Ok(other) => outcome(false, Some(format!("returned a non-Bool value: {}", show(&other)))),
            Err(Flow::Halt(e)) => {
                outcome(false, Some(format!("halted: [{}] {}", e.diag.rule, e.diag.message)))
            }
            Err(Flow::Ret(_)) => unreachable!(),
        }
    }

    fn run_property_test(&mut self, t: &Test) -> TestOutcome {
        const CASES: usize = 100;
        const GEN_TRIES: usize = 200;
        let mut ran = 0usize;
        for _ in 0..CASES {
            // generate arguments param-by-param; contracts constrain values [W35]
            let mut env: Vec<(String, Value)> = Vec::new();
            let mut ok = true;
            for p in &t.params {
                let mut accepted = None;
                for _ in 0..GEN_TRIES {
                    let v = self.gen_value(&p.ty);
                    env.push((p.name.clone(), v.clone()));
                    let holds = match &p.contract {
                        None => true,
                        Some(c) => match self.eval(c, &mut env) {
                            Ok(Value::Bool(b)) => b,
                            _ => false,
                        },
                    };
                    if holds {
                        accepted = Some(v);
                        break;
                    }
                    env.pop();
                }
                match accepted {
                    Some(_) => {}
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                continue; // could not satisfy contracts for this case
            }
            ran += 1;
            let args_desc = || {
                env.iter()
                    .map(|(n, v)| format!("{} = {}", n, show(v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            match self.eval(&t.body, &mut env.clone()) {
                Ok(Value::Bool(true)) => {}
                Ok(Value::Bool(false)) => {
                    return TestOutcome { name: t.name.clone(), passed: false, detail: Some(format!("counterexample: ({})", args_desc())), cases: ran, span: t.span, property: true };
                }
                Ok(other) => {
                    return TestOutcome { name: t.name.clone(), passed: false, detail: Some(format!("non-Bool result {} for ({})", show(&other), args_desc())), cases: ran, span: t.span, property: true };
                }
                Err(Flow::Halt(e)) => {
                    return TestOutcome { name: t.name.clone(), passed: false, detail: Some(format!("halted [{}] {} for ({})", e.diag.rule, e.diag.message, args_desc())), cases: ran, span: t.span, property: true };
                }
                Err(Flow::Ret(_)) => unreachable!(),
            }
        }
        if ran == 0 {
            return TestOutcome {
                name: t.name.clone(),
                passed: false,
                detail: Some("could not generate any inputs satisfying the contracts".into()),
                cases: 0,
                span: t.span,
                property: true,
            };
        }
        TestOutcome { name: t.name.clone(), passed: true, detail: None, cases: ran, span: t.span, property: true }
    }

    // ---------- deterministic value generation (runner-side randomness [W35]) ----------

    fn next_u64(&mut self) -> u64 {
        // splitmix64
        self.prng = self.prng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.prng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next_range(&mut self, lo: i64, hi: i64) -> i64 {
        let span = (hi - lo + 1) as u64;
        lo + (self.next_u64() % span) as i64
    }

    fn gen_value(&mut self, te: &TypeExpr) -> Value {
        match te {
            TypeExpr::Name(name, args, _) => match (name.as_str(), args.len()) {
                ("Int", 0) => {
                    // mix interesting edge values with small randoms
                    match self.next_u64() % 8 {
                        0 => Value::Int(0),
                        1 => Value::Int(1),
                        2 => Value::Int(-1),
                        _ => Value::Int(self.next_range(-100, 100)),
                    }
                }
                ("Float", 0) => match self.next_u64() % 8 {
                    0 => Value::Float(0.0),
                    1 => Value::Float(1.0),
                    2 => Value::Float(-1.0),
                    _ => Value::Float(self.next_range(-1000, 1000) as f64 / 8.0),
                },
                ("Bool", 0) => Value::Bool(self.next_u64() % 2 == 0),
                ("Text", 0) => {
                    let len = (self.next_u64() % 7) as usize;
                    let alphabet = ['a', 'b', 'c', 'x', ' ', 'z', '1'];
                    let mut s = String::new();
                    for _ in 0..len {
                        s.push(alphabet[(self.next_u64() as usize) % alphabet.len()]);
                    }
                    Value::Text(s)
                }
                ("List", 1) => {
                    let len = (self.next_u64() % 7) as usize;
                    let mut items = Vec::new();
                    for _ in 0..len {
                        items.push(self.gen_value(&args[0]));
                    }
                    Value::List(items)
                }
                _ => Value::Unit, // rejected by the checker beforehand [W35]
            },
            _ => Value::Unit,
        }
    }

    // ---------- evaluation ----------

    fn call_def(&mut self, d: &Def, args: Vec<Value>, call_span: Span) -> EResult {
        let params = d.params.clone().expect("call_def on a constant");
        debug_assert_eq!(params.len(), args.len());
        let mut env: Vec<(String, Value)> = Vec::new();
        for (p, arg) in params.iter().zip(args.into_iter()) {
            env.push((p.name.clone(), arg));
            if let Some(contract) = &p.contract {
                match self.eval(contract, &mut env)? {
                    Value::Bool(true) => {}
                    Value::Bool(false) => {
                        let shown: Vec<String> =
                            env.iter().map(|(n, v)| format!("{} = {}", n, show(v))).collect();
                        let diag = Diag::new(
                            "W28",
                            format!("contract violated calling `{}`", d.name),
                            call_span,
                        )
                        .expected(expr_text(contract))
                        .actual(shown.join(", "))
                        .hint("guard the call site so the contract holds, or adjust the contract");
                        return Err(Flow::Halt(RunErr { diag }));
                    }
                    _ => return Err(halt("W29", "contract did not evaluate to a Bool", p.span)),
                }
            }
        }
        if d.is_infer {
            // [W43]: the body is the prompt; the call goes to the model
            let prompt = match self.eval(&d.body, &mut env) {
                Ok(Value::Text(t)) => t,
                Ok(other) => {
                    return Err(halt("W43", format!("infer body produced a non-Text: {}", show(&other)), d.span))
                }
                Err(Flow::Ret(v)) => return Ok(v),
                Err(h) => return Err(h),
            };
            return Ok(self.model_call(d, &prompt));
        }
        match self.eval(&d.body, &mut env) {
            Ok(v) => Ok(v),
            Err(Flow::Ret(v)) => Ok(v), // `?` early return lands here [W26]
            Err(h) => Err(h),
        }
    }

    // ---------- model calls [W43] ----------

    fn model_call(&mut self, d: &Def, prompt: &str) -> Value {
        let err_val = |m: String| Value::Ctor("Err".into(), vec![Value::Text(m)]);
        let ok_ty = match &d.ty {
            TypeExpr::Name(n, args, _) if n == "Result" && args.len() == 2 => args[0].clone(),
            _ => return err_val("infer return type is not Result".into()),
        };
        let full = self.build_model_prompt(prompt, &ok_ty);
        let mut feedback: Option<String> = None;
        for _ in 0..2 {
            let p = match &feedback {
                Some(f) => format!(
                    "{}\n\nYour previous reply was invalid: {}\nReply again with ONLY the literal.",
                    full, f
                ),
                None => full.clone(),
            };
            let reply = match invoke_model(&p) {
                Ok(r) => r,
                Err(e) => return err_val(format!("model: {}", e)),
            };
            match self.parse_reply(&reply, &ok_ty) {
                Ok(v) => return Value::Ctor("Ok".into(), vec![v]),
                Err(e) => feedback = Some(e),
            }
        }
        err_val(format!("model: invalid reply: {}", feedback.unwrap_or_default()))
    }

    /// Prompt + the expected type + the definitions of every user type it uses.
    fn build_model_prompt(&self, prompt: &str, ok_ty: &TypeExpr) -> String {
        let mut reachable: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        self.collect_reachable(ok_ty, &mut seen, &mut reachable);
        let mut out = String::new();
        out.push_str(prompt);
        out.push_str("\n\nReply with ONLY a Weft literal value of this type — no prose, no code fences:\n  ");
        out.push_str(&crate::index::type_text(ok_ty));
        if !reachable.is_empty() {
            out.push_str("\n\nType definitions:\n");
            for name in &reachable {
                if let Some(td) = self.typedefs.get(name) {
                    out.push_str("  ");
                    out.push_str(&crate::index::typedef_text(td));
                    out.push('\n');
                }
            }
        }
        out.push_str("\nLiteral syntax: records {field: value}, named records Name{field: value}, variants Name(args) or bare Name, lists [a, b], text \"quoted\", Some(x)/None, Ok(x)/Err(e), negative numbers -n.");
        out
    }

    fn collect_reachable(&self, te: &TypeExpr, seen: &mut std::collections::HashSet<String>, out: &mut Vec<String>) {
        match te {
            TypeExpr::Name(n, args, _) => {
                for a in args {
                    self.collect_reachable(a, seen, out);
                }
                if let Some(td) = self.typedefs.get(n) {
                    if seen.insert(n.clone()) {
                        out.push(n.clone());
                        match &td.decl {
                            TypeDecl::Alias(inner) => self.collect_reachable(inner, seen, out),
                            TypeDecl::Variants(vs) => {
                                for v in vs {
                                    for p in &v.payload {
                                        self.collect_reachable(p, seen, out);
                                    }
                                }
                            }
                            TypeDecl::Nominal { fields, .. } => {
                                for (_, t) in fields {
                                    self.collect_reachable(t, seen, out);
                                }
                            }
                        }
                    }
                }
            }
            TypeExpr::Record(fields, _) => {
                for (_, t) in fields {
                    self.collect_reachable(t, seen, out);
                }
            }
            TypeExpr::Fn(args, ret, _) => {
                for a in args {
                    self.collect_reachable(a, seen, out);
                }
                self.collect_reachable(ret, seen, out);
            }
        }
    }

    /// Parse a model reply as a literal of the expected type: lex, parse,
    /// literal-only check, evaluate (invariants run here [W42]), fit check.
    fn parse_reply(&mut self, reply: &str, ok_ty: &TypeExpr) -> Result<Value, String> {
        let mut text = reply.trim();
        // tolerate a fenced reply despite instructions
        if text.starts_with("```") {
            text = text.trim_start_matches("```").trim_start_matches("weft").trim();
            if let Some(end) = text.rfind("```") {
                text = text[..end].trim();
            }
        }
        let toks = crate::lexer::lex(text).map_err(|d| format!("lex error: {}", d.message))?;
        let mut p = crate::parser::Parser::new(toks);
        let expr = p.parse_expr().map_err(|d| format!("parse error: {}", d.message))?;
        if !p.at_eof() {
            return Err("trailing content after the literal".into());
        }
        if !literal_only(&expr) {
            return Err("reply must be a pure literal (no names, calls, or operators)".into());
        }
        let mut env = Vec::new();
        let v = match self.eval(&expr, &mut env) {
            Ok(v) => v,
            Err(Flow::Halt(e)) => return Err(format!("[{}] {}", e.diag.rule, e.diag.message)),
            Err(Flow::Ret(_)) => return Err("invalid literal".into()),
        };
        self.fits(&v, ok_ty)?;
        Ok(v)
    }

    /// Structural check: does the value have the declared type?
    fn fits(&self, v: &Value, te: &TypeExpr) -> Result<(), String> {
        let fail = |exp: &str, v: &Value| Err(format!("expected {}, got {}", exp, show(v)));
        match te {
            TypeExpr::Record(fields, _) => match v {
                Value::Rec(fs) => {
                    if fs.len() != fields.len() {
                        return fail(&crate::index::type_text(te), v);
                    }
                    let mut sorted: Vec<&(String, TypeExpr)> = fields.iter().collect();
                    sorted.sort_by(|a, b| a.0.cmp(&b.0));
                    for ((fname, fty), (vname, vv)) in sorted.iter().zip(fs.iter()) {
                        if fname != vname {
                            return fail(&crate::index::type_text(te), v);
                        }
                        self.fits(vv, fty)?;
                    }
                    Ok(())
                }
                _ => fail(&crate::index::type_text(te), v),
            },
            TypeExpr::Fn(_, _, _) => Err("a model cannot produce a function value".into()),
            TypeExpr::Name(n, args, _) => match (n.as_str(), args.len()) {
                ("Int", 0) => matches!(v, Value::Int(_)).then_some(()).ok_or_else(|| format!("expected Int, got {}", show(v))),
                ("Float", 0) => matches!(v, Value::Float(_)).then_some(()).ok_or_else(|| format!("expected Float, got {}", show(v))),
                ("Bool", 0) => matches!(v, Value::Bool(_)).then_some(()).ok_or_else(|| format!("expected Bool, got {}", show(v))),
                ("Text", 0) => matches!(v, Value::Text(_)).then_some(()).ok_or_else(|| format!("expected Text, got {}", show(v))),
                ("Unit", 0) => matches!(v, Value::Unit).then_some(()).ok_or_else(|| format!("expected Unit, got {}", show(v))),
                ("List", 1) => match v {
                    Value::List(items) => {
                        for it in items {
                            self.fits(it, &args[0])?;
                        }
                        Ok(())
                    }
                    _ => fail("a List", v),
                },
                ("Option", 1) => match v {
                    Value::Ctor(c, payload) if c == "Some" && payload.len() == 1 => self.fits(&payload[0], &args[0]),
                    Value::Ctor(c, payload) if c == "None" && payload.is_empty() => Ok(()),
                    _ => fail("an Option", v),
                },
                ("Result", 2) => match v {
                    Value::Ctor(c, payload) if c == "Ok" && payload.len() == 1 => self.fits(&payload[0], &args[0]),
                    Value::Ctor(c, payload) if c == "Err" && payload.len() == 1 => self.fits(&payload[0], &args[1]),
                    _ => fail("a Result", v),
                },
                _ => match self.typedefs.get(n) {
                    Some(td) => match &td.decl {
                        TypeDecl::Alias(inner) => self.fits(v, inner),
                        TypeDecl::Variants(vs) => match v {
                            Value::Ctor(cname, payload) => match vs.iter().find(|vd| &vd.name == cname) {
                                Some(vd) => {
                                    if payload.len() != vd.payload.len() {
                                        return fail(n, v);
                                    }
                                    for (pv, pt) in payload.iter().zip(vd.payload.iter()) {
                                        self.fits(pv, pt)?;
                                    }
                                    Ok(())
                                }
                                None => fail(n, v),
                            },
                            _ => fail(n, v),
                        },
                        TypeDecl::Nominal { fields, .. } => match v {
                            Value::Rec(fs) => {
                                if fs.len() != fields.len() {
                                    return fail(n, v);
                                }
                                let mut sorted: Vec<&(String, TypeExpr)> = fields.iter().collect();
                                sorted.sort_by(|a, b| a.0.cmp(&b.0));
                                for ((fname, fty), (vname, vv)) in sorted.iter().zip(fs.iter()) {
                                    if fname != vname {
                                        return fail(n, v);
                                    }
                                    self.fits(vv, fty)?;
                                }
                                Ok(())
                            }
                            _ => fail(n, v),
                        },
                    },
                    None => Err(format!("unknown type `{}` in infer return", n)),
                },
            },
        }
    }

    fn call_value(&mut self, f: &Value, args: Vec<Value>, span: Span) -> EResult {
        match f {
            Value::NamedFn(name) => {
                let d = self.defs.get(name).cloned().expect("checked def");
                self.call_def(&d, args, span)
            }
            Value::Builtin(name) => self.builtin(name.clone().as_str(), args, span),
            Value::Closure { params, body, env } => {
                let mut env2 = env.clone();
                for (p, a) in params.iter().zip(args.into_iter()) {
                    env2.push((p.clone(), a));
                }
                self.eval(body, &mut env2)
            }
            other => Err(halt("W19", format!("value is not callable: {}", show(other)), span)),
        }
    }

    fn lookup(&mut self, name: &str, env: &[(String, Value)], span: Span) -> EResult {
        if let Some((_, v)) = env.iter().rev().find(|(n, _)| n == name) {
            return Ok(v.clone());
        }
        if let Some(v) = self.const_cache.get(name) {
            return Ok(v.clone());
        }
        if let Some(c) = self.consts.get(name).cloned() {
            let mut e = Vec::new();
            let v = self.eval(&c.body, &mut e)?;
            self.const_cache.insert(name.to_string(), v.clone());
            return Ok(v);
        }
        if self.defs.contains_key(name) {
            return Ok(Value::NamedFn(name.to_string()));
        }
        if is_builtin(name) {
            return Ok(Value::Builtin(name.to_string()));
        }
        Err(halt("W3", format!("unknown name `{}` at runtime", name), span))
    }

    fn eval(&mut self, e: &Expr, env: &mut Vec<(String, Value)>) -> EResult {
        match &e.kind {
            ExprKind::Int(v) => Ok(Value::Int(*v)),
            ExprKind::Float(f) => Ok(Value::Float(*f)),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Text(s) => Ok(Value::Text(s.clone())),
            ExprKind::Unit => Ok(Value::Unit),
            ExprKind::Hole(name) => Err(halt(
                "W27",
                format!("evaluated hole `?{}`; fill it in before running", name),
                e.span,
            )),
            ExprKind::List(items) => {
                let mut out = Vec::new();
                for it in items {
                    out.push(self.eval(it, env)?);
                }
                Ok(Value::List(out))
            }
            ExprKind::Var(name) => self.lookup(name, env, e.span),
            ExprKind::Ctor(name, args) => {
                let mut vals = Vec::new();
                for a in args {
                    vals.push(self.eval(a, env)?);
                }
                Ok(Value::Ctor(name.clone(), vals))
            }
            ExprKind::Record { spread, fields } => {
                match spread {
                    Some(base) => {
                        let bv = self.eval(base, env)?;
                        let mut fs = match bv {
                            Value::Rec(fs) => fs,
                            other => {
                                return Err(halt("W11", format!("`..` on a non-record: {}", show(&other)), e.span))
                            }
                        };
                        for (name, val) in fields {
                            let v = self.eval(val, env)?;
                            match fs.iter_mut().find(|(n, _)| n == name) {
                                Some(slot) => slot.1 = v,
                                None => return Err(halt("W11", format!("no field `{}`", name), e.span)),
                            }
                        }
                        Ok(Value::Rec(fs))
                    }
                    None => {
                        let mut fs = Vec::new();
                        for (name, val) in fields {
                            fs.push((name.clone(), self.eval(val, env)?));
                        }
                        fs.sort_by(|a, b| a.0.cmp(&b.0));
                        Ok(Value::Rec(fs))
                    }
                }
            }
            ExprKind::NamedRec { name, spread, fields } => {
                let mut fs: Vec<(String, Value)> = match spread {
                    Some(base) => match self.eval(base, env)? {
                        Value::Rec(fs) => fs,
                        other => {
                            return Err(halt("W42", format!("`..` on a non-record: {}", show(&other)), e.span))
                        }
                    },
                    None => Vec::new(),
                };
                for (fname, fval) in fields {
                    let v = self.eval(fval, env)?;
                    match fs.iter_mut().find(|(n, _)| n == fname) {
                        Some(slot) => slot.1 = v,
                        None => fs.push((fname.clone(), v)),
                    }
                }
                fs.sort_by(|a, b| a.0.cmp(&b.0));
                // check the invariant with the fields in scope [W42]
                if let Some(inv) = self.invariants.get(name).cloned() {
                    let mut inv_env: Vec<(String, Value)> = fs.clone();
                    match self.eval(&inv, &mut inv_env)? {
                        Value::Bool(true) => {}
                        Value::Bool(false) => {
                            let shown: Vec<String> =
                                fs.iter().map(|(n, v)| format!("{} = {}", n, show(v))).collect();
                            let diag = Diag::new(
                                "W42",
                                format!("invariant of `{}` violated at construction", name),
                                e.span,
                            )
                            .expected(expr_text(&inv))
                            .actual(shown.join(", "))
                            .hint("construct only values that satisfy the type's invariant");
                            return Err(Flow::Halt(RunErr { diag }));
                        }
                        other => {
                            return Err(halt("W42", format!("invariant evaluated to a non-Bool: {}", show(&other)), e.span))
                        }
                    }
                }
                Ok(Value::Rec(fs))
            }
            ExprKind::Field(base, name) => {
                let bv = self.eval(base, env)?;
                match bv {
                    Value::Rec(fs) => fs
                        .iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, v)| Ok(v.clone()))
                        .unwrap_or_else(|| Err(halt("W11", format!("no field `{}`", name), e.span))),
                    other => Err(halt("W11", format!("`.{}` on a non-record: {}", name, show(&other)), e.span)),
                }
            }
            ExprKind::Call(callee, args) => {
                let f = self.eval(callee, env)?;
                let mut vals = Vec::new();
                for a in args {
                    vals.push(self.eval(a, env)?);
                }
                self.call_value(&f, vals, e.span)
            }
            ExprKind::Lambda { params, body } => Ok(Value::Closure {
                params: params.iter().map(|(n, _)| n.clone()).collect(),
                body: body.clone(),
                env: env.clone(),
            }),
            ExprKind::Block { lets, tail } => {
                let mark = env.len();
                for (name, val) in lets {
                    let v = self.eval(val, env)?;
                    env.push((name.clone(), v));
                }
                let out = self.eval(tail, env);
                env.truncate(mark);
                out
            }
            ExprKind::If { cond, then, els } => match self.eval(cond, env)? {
                Value::Bool(true) => self.eval(then, env),
                Value::Bool(false) => self.eval(els, env),
                other => Err(halt("W22", format!("if condition was not a Bool: {}", show(&other)), e.span)),
            },
            ExprKind::Match { scrutinee, arms } => {
                let sv = self.eval(scrutinee, env)?;
                for (pat, body) in arms {
                    let mark = env.len();
                    if try_match(pat, &sv, env) {
                        let out = self.eval(body, env);
                        env.truncate(mark);
                        return out;
                    }
                    env.truncate(mark);
                }
                Err(halt(
                    "W24",
                    format!("no match arm matched value {}", show(&sv)),
                    e.span,
                ))
            }
            ExprKind::Bin(op, l, r) => self.eval_bin(*op, l, r, env, e.span),
            ExprKind::NotOp(inner) => match self.eval(inner, env)? {
                Value::Bool(b) => Ok(Value::Bool(!b)),
                other => Err(halt("W25", format!("`not` on a non-Bool: {}", show(&other)), e.span)),
            },
            ExprKind::NegOp(inner) => match self.eval(inner, env)? {
                Value::Int(n) => match n.checked_neg() {
                    Some(v) => Ok(Value::Int(v)),
                    None => Err(halt("W38", "Int overflow in unary `-`", e.span)),
                },
                Value::Float(f) => Ok(Value::Float(-f)),
                other => Err(halt("W25", format!("unary `-` on a non-number: {}", show(&other)), e.span)),
            },
            ExprKind::Propagate(inner) => match self.eval(inner, env)? {
                Value::Ctor(name, mut args) if name == "Ok" && args.len() == 1 => Ok(args.remove(0)),
                v @ Value::Ctor(_, _) => {
                    if let Value::Ctor(name, _) = &v {
                        if name == "Err" {
                            return Err(Flow::Ret(v));
                        }
                    }
                    Err(halt("W26", format!("`?` on a non-Result value: {}", show(&v)), e.span))
                }
                other => Err(halt("W26", format!("`?` on a non-Result value: {}", show(&other)), e.span)),
            },
        }
    }

    fn eval_bin(&mut self, op: BinOp, l: &Expr, r: &Expr, env: &mut Vec<(String, Value)>, span: Span) -> EResult {
        use BinOp::*;
        // short-circuit forms first [W25]
        match op {
            And => {
                return match self.eval(l, env)? {
                    Value::Bool(false) => Ok(Value::Bool(false)),
                    Value::Bool(true) => self.eval(r, env),
                    other => Err(halt("W25", format!("`and` on a non-Bool: {}", show(&other)), span)),
                }
            }
            Or => {
                return match self.eval(l, env)? {
                    Value::Bool(true) => Ok(Value::Bool(true)),
                    Value::Bool(false) => self.eval(r, env),
                    other => Err(halt("W25", format!("`or` on a non-Bool: {}", show(&other)), span)),
                }
            }
            _ => {}
        }
        let lv = self.eval(l, env)?;
        let rv = self.eval(r, env)?;
        match op {
            Add | Sub | Mul | Div | Rem => match (&lv, &rv) {
                (Value::Int(a), Value::Int(b)) => {
                    let res = match op {
                        Add => a.checked_add(*b),
                        Sub => a.checked_sub(*b),
                        Mul => a.checked_mul(*b),
                        Div => {
                            if *b == 0 {
                                return Err(halt("W38", "Int division by zero", span));
                            }
                            a.checked_div(*b)
                        }
                        Rem => {
                            if *b == 0 {
                                return Err(halt("W38", "Int modulo by zero", span));
                            }
                            a.checked_rem(*b)
                        }
                        _ => unreachable!(),
                    };
                    match res {
                        Some(v) => Ok(Value::Int(v)),
                        None => Err(halt("W38", "Int overflow", span)),
                    }
                }
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(match op {
                    Add => a + b,
                    Sub => a - b,
                    Mul => a * b,
                    Div => a / b,
                    Rem => a % b,
                    _ => unreachable!(),
                })),
                _ => Err(halt("W25", "arithmetic on mismatched operand types", span)),
            },
            Concat => match (lv, rv) {
                (Value::Text(a), Value::Text(b)) => Ok(Value::Text(a + &b)),
                (Value::List(mut a), Value::List(b)) => {
                    a.extend(b);
                    Ok(Value::List(a))
                }
                _ => Err(halt("W25", "`++` needs Text with Text or List with List", span)),
            },
            Lt | Le | Gt | Ge => {
                let ord = match (&lv, &rv) {
                    (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
                    (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
                    _ => None,
                };
                match ord {
                    Some(o) => Ok(Value::Bool(match op {
                        Lt => o.is_lt(),
                        Le => o.is_le(),
                        Gt => o.is_gt(),
                        Ge => o.is_ge(),
                        _ => unreachable!(),
                    })),
                    None => Err(halt("W25", "ordering comparison on non-numeric operands", span)),
                }
            }
            Eq => Ok(Value::Bool(values_eq(&lv, &rv))),
            Ne => Ok(Value::Bool(!values_eq(&lv, &rv))),
            And | Or => unreachable!(),
        }
    }

    // ---------- standard library ----------

    fn builtin(&mut self, name: &str, mut args: Vec<Value>, span: Span) -> EResult {
        macro_rules! arg {
            ($i:expr) => {
                std::mem::replace(&mut args[$i], Value::Unit)
            };
        }
        let v = match (name, args.len()) {
            // Text
            ("text_len", 1) => match arg!(0) {
                Value::Text(s) => Value::Int(s.chars().count() as i64),
                _ => return type_halt(name, span),
            },
            ("text_of_int", 1) => match arg!(0) {
                Value::Int(n) => Value::Text(n.to_string()),
                _ => return type_halt(name, span),
            },
            ("text_of_float", 1) => match arg!(0) {
                Value::Float(f) => Value::Text(show_float(f)),
                _ => return type_halt(name, span),
            },
            ("text_of_bool", 1) => match arg!(0) {
                Value::Bool(b) => Value::Text(if b { "true".into() } else { "false".into() }),
                _ => return type_halt(name, span),
            },
            ("int_of_text", 1) => match arg!(0) {
                Value::Text(s) => match s.parse::<i64>() {
                    Ok(n) => Value::Ctor("Some".into(), vec![Value::Int(n)]),
                    Err(_) => Value::Ctor("None".into(), vec![]),
                },
                _ => return type_halt(name, span),
            },
            ("split", 2) => match (arg!(0), arg!(1)) {
                (Value::Text(s), Value::Text(sep)) => {
                    let parts: Vec<Value> = if sep.is_empty() {
                        s.chars().map(|c| Value::Text(c.to_string())).collect()
                    } else {
                        s.split(&sep).map(|p| Value::Text(p.to_string())).collect()
                    };
                    Value::List(parts)
                }
                _ => return type_halt(name, span),
            },
            ("join", 2) => match (arg!(0), arg!(1)) {
                (Value::List(items), Value::Text(sep)) => {
                    let mut parts = Vec::new();
                    for it in items {
                        match it {
                            Value::Text(t) => parts.push(t),
                            _ => return type_halt(name, span),
                        }
                    }
                    Value::Text(parts.join(&sep))
                }
                _ => return type_halt(name, span),
            },
            ("contains", 2) => match (arg!(0), arg!(1)) {
                (Value::Text(s), Value::Text(sub)) => Value::Bool(s.contains(&sub)),
                _ => return type_halt(name, span),
            },
            ("chars", 1) => match arg!(0) {
                Value::Text(s) => Value::List(s.chars().map(|c| Value::Text(c.to_string())).collect()),
                _ => return type_halt(name, span),
            },
            ("to_upper", 1) => match arg!(0) {
                Value::Text(s) => Value::Text(s.to_uppercase()),
                _ => return type_halt(name, span),
            },
            ("to_lower", 1) => match arg!(0) {
                Value::Text(s) => Value::Text(s.to_lowercase()),
                _ => return type_halt(name, span),
            },
            ("trim", 1) => match arg!(0) {
                Value::Text(s) => Value::Text(s.trim().to_string()),
                _ => return type_halt(name, span),
            },
            // List
            ("len", 1) => match arg!(0) {
                Value::List(xs) => Value::Int(xs.len() as i64),
                _ => return type_halt(name, span),
            },
            ("list_get", 2) => match (arg!(0), arg!(1)) {
                (Value::List(xs), Value::Int(i)) => {
                    if i >= 0 && (i as usize) < xs.len() {
                        Value::Ctor("Some".into(), vec![xs[i as usize].clone()])
                    } else {
                        Value::Ctor("None".into(), vec![])
                    }
                }
                _ => return type_halt(name, span),
            },
            ("append", 2) => match (arg!(0), arg!(1)) {
                (Value::List(mut xs), x) => {
                    xs.push(x);
                    Value::List(xs)
                }
                _ => return type_halt(name, span),
            },
            ("map", 2) => match (arg!(0), arg!(1)) {
                (Value::List(xs), f) => {
                    let mut out = Vec::new();
                    for x in xs {
                        out.push(self.call_value(&f, vec![x], span)?);
                    }
                    Value::List(out)
                }
                _ => return type_halt(name, span),
            },
            ("filter", 2) => match (arg!(0), arg!(1)) {
                (Value::List(xs), f) => {
                    let mut out = Vec::new();
                    for x in xs {
                        match self.call_value(&f, vec![x.clone()], span)? {
                            Value::Bool(true) => out.push(x),
                            Value::Bool(false) => {}
                            _ => return type_halt(name, span),
                        }
                    }
                    Value::List(out)
                }
                _ => return type_halt(name, span),
            },
            ("fold", 3) => match (arg!(0), arg!(1), arg!(2)) {
                (Value::List(xs), init, f) => {
                    let mut acc = init;
                    for x in xs {
                        acc = self.call_value(&f, vec![acc, x], span)?;
                    }
                    acc
                }
                _ => return type_halt(name, span),
            },
            ("range", 2) => match (arg!(0), arg!(1)) {
                (Value::Int(lo), Value::Int(hi)) => {
                    let mut out = Vec::new();
                    let mut i = lo;
                    while i < hi {
                        out.push(Value::Int(i));
                        i += 1;
                    }
                    Value::List(out)
                }
                _ => return type_halt(name, span),
            },
            ("reverse", 1) => match arg!(0) {
                Value::List(mut xs) => {
                    xs.reverse();
                    Value::List(xs)
                }
                _ => return type_halt(name, span),
            },
            ("sort_by", 2) => match (arg!(0), arg!(1)) {
                (Value::List(xs), f) => {
                    let mut keyed = Vec::new();
                    for x in xs {
                        let k = match self.call_value(&f, vec![x.clone()], span)? {
                            Value::Int(k) => k,
                            _ => return type_halt(name, span),
                        };
                        keyed.push((k, x));
                    }
                    keyed.sort_by_key(|(k, _)| *k); // stable, ascending [Â§10]
                    Value::List(keyed.into_iter().map(|(_, x)| x).collect())
                }
                _ => return type_halt(name, span),
            },
            ("find", 2) => match (arg!(0), arg!(1)) {
                (Value::List(xs), f) => {
                    let mut found = Value::Ctor("None".into(), vec![]);
                    for x in xs {
                        match self.call_value(&f, vec![x.clone()], span)? {
                            Value::Bool(true) => {
                                found = Value::Ctor("Some".into(), vec![x]);
                                break;
                            }
                            Value::Bool(false) => {}
                            _ => return type_halt(name, span),
                        }
                    }
                    found
                }
                _ => return type_halt(name, span),
            },
            ("index_of", 2) => match (arg!(0), arg!(1)) {
                (Value::List(xs), x) => xs
                    .iter()
                    .position(|item| values_eq(item, &x))
                    .map(|i| Value::Ctor("Some".into(), vec![Value::Int(i as i64)]))
                    .unwrap_or(Value::Ctor("None".into(), vec![])),
                _ => return type_halt(name, span),
            },
            ("zip", 2) => match (arg!(0), arg!(1)) {
                (Value::List(xs), Value::List(ys)) => Value::List(
                    xs.into_iter()
                        .zip(ys.into_iter())
                        .map(|(a, b)| Value::Rec(vec![("fst".into(), a), ("snd".into(), b)]))
                        .collect(),
                ),
                _ => return type_halt(name, span),
            },
            // Option / Result / math
            ("unwrap_or", 2) => match (arg!(0), arg!(1)) {
                (Value::Ctor(c, mut payload), d) => {
                    if c == "Some" && payload.len() == 1 {
                        payload.remove(0)
                    } else {
                        d
                    }
                }
                _ => return type_halt(name, span),
            },
            ("ok_or", 2) => match (arg!(0), arg!(1)) {
                (Value::Ctor(c, mut payload), e) => {
                    if c == "Some" && payload.len() == 1 {
                        Value::Ctor("Ok".into(), vec![payload.remove(0)])
                    } else {
                        Value::Ctor("Err".into(), vec![e])
                    }
                }
                _ => return type_halt(name, span),
            },
            ("abs", 1) => match arg!(0) {
                Value::Int(n) => Value::Int(n.abs()),
                _ => return type_halt(name, span),
            },
            ("min", 2) => match (arg!(0), arg!(1)) {
                (Value::Int(a), Value::Int(b)) => Value::Int(a.min(b)),
                _ => return type_halt(name, span),
            },
            ("max", 2) => match (arg!(0), arg!(1)) {
                (Value::Int(a), Value::Int(b)) => Value::Int(a.max(b)),
                _ => return type_halt(name, span),
            },
            ("int_to_float", 1) => match arg!(0) {
                Value::Int(n) => Value::Float(n as f64),
                _ => return type_halt(name, span),
            },
            ("float_to_int", 1) => match arg!(0) {
                Value::Float(f) => Value::Int(f.trunc() as i64),
                _ => return type_halt(name, span),
            },
            // Effectful [W32]
            ("print", 2) => match (arg!(0), arg!(1)) {
                (Value::Cap(_), Value::Text(s)) => {
                    println!("{}", s);
                    let _ = std::io::stdout().flush();
                    Value::Unit
                }
                _ => return type_halt(name, span),
            },
            ("read_line", 1) => match arg!(0) {
                Value::Cap(_) => {
                    let mut line = String::new();
                    let _ = std::io::stdin().read_line(&mut line);
                    Value::Text(line.trim_end_matches(['\n', '\r']).to_string())
                }
                _ => return type_halt(name, span),
            },
            ("fs", 1) => Value::Cap("Fs".into()),
            ("rand", 1) => Value::Cap("Rand".into()),
            ("clock", 1) => Value::Cap("Clock".into()),
            ("model", 1) => Value::Cap("Model".into()),
            ("fs_read", 2) => match (arg!(0), arg!(1)) {
                (Value::Cap(_), Value::Text(path)) => match std::fs::read_to_string(&path) {
                    Ok(content) => Value::Ctor("Ok".into(), vec![Value::Text(content)]),
                    Err(err) => Value::Ctor("Err".into(), vec![Value::Text(err.to_string())]),
                },
                _ => return type_halt(name, span),
            },
            ("fs_write", 3) => match (arg!(0), arg!(1), arg!(2)) {
                (Value::Cap(_), Value::Text(path), Value::Text(content)) => {
                    match std::fs::write(&path, content) {
                        Ok(()) => Value::Ctor("Ok".into(), vec![Value::Unit]),
                        Err(err) => Value::Ctor("Err".into(), vec![Value::Text(err.to_string())]),
                    }
                }
                _ => return type_halt(name, span),
            },
            ("rand_int", 3) => match (arg!(0), arg!(1), arg!(2)) {
                (Value::Cap(_), Value::Int(lo), Value::Int(hi)) => {
                    if hi < lo {
                        return Err(halt("W38", "rand_int with hi < lo", span));
                    }
                    Value::Int(self.next_range(lo, hi))
                }
                _ => return type_halt(name, span),
            },
            ("now_ms", 1) => match arg!(0) {
                Value::Cap(_) => {
                    let ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    Value::Int(ms)
                }
                _ => return type_halt(name, span),
            },
            _ => return Err(halt("W3", format!("unknown builtin `{}`", name), span)),
        };
        Ok(v)
    }
}

fn type_halt(name: &str, span: Span) -> EResult {
    Err(halt("W32", format!("`{}` called with wrong argument types at runtime", name), span))
}

fn try_match(pat: &Pattern, v: &Value, env: &mut Vec<(String, Value)>) -> bool {
    match (&pat.kind, v) {
        (PatKind::Wildcard, _) => true,
        (PatKind::Bind(name), _) => {
            env.push((name.clone(), v.clone()));
            true
        }
        (PatKind::LitInt(a), Value::Int(b)) => a == b,
        (PatKind::LitFloat(a), Value::Float(b)) => a == b,
        (PatKind::LitBool(a), Value::Bool(b)) => a == b,
        (PatKind::LitText(a), Value::Text(b)) => a == b,
        (PatKind::Ctor(name, subs), Value::Ctor(vname, payload)) => {
            name == vname
                && subs.len() == payload.len()
                && subs.iter().zip(payload.iter()).all(|(p, x)| try_match(p, x, env))
        }
        (PatKind::List { heads, rest }, Value::List(items)) => {
            match rest {
                None => {
                    if heads.len() != items.len() {
                        return false;
                    }
                }
                Some(_) => {
                    if items.len() < heads.len() {
                        return false;
                    }
                }
            }
            for (p, x) in heads.iter().zip(items.iter()) {
                if !try_match(p, x, env) {
                    return false;
                }
            }
            if let Some(binder) = rest {
                if binder != "_" {
                    env.push((binder.clone(), Value::List(items[heads.len()..].to_vec())));
                }
            }
            true
        }
        _ => false,
    }
}

fn values_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Text(x), Value::Text(y)) => x == y,
        (Value::Unit, Value::Unit) => true,
        (Value::List(xs), Value::List(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys.iter()).all(|(x, y)| values_eq(x, y))
        }
        (Value::Rec(xs), Value::Rec(ys)) => {
            xs.len() == ys.len()
                && xs
                    .iter()
                    .zip(ys.iter())
                    .all(|((n1, x), (n2, y))| n1 == n2 && values_eq(x, y))
        }
        (Value::Ctor(n1, xs), Value::Ctor(n2, ys)) => {
            n1 == n2 && xs.len() == ys.len() && xs.iter().zip(ys.iter()).all(|(x, y)| values_eq(x, y))
        }
        _ => false,
    }
}

/// A model reply may only be a literal tree: no names, calls, or control flow.
fn literal_only(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Text(_) | ExprKind::Unit => true,
        ExprKind::NegOp(inner) => literal_only(inner),
        ExprKind::List(es) => es.iter().all(literal_only),
        ExprKind::Ctor(_, args) => args.iter().all(literal_only),
        ExprKind::Record { spread: None, fields } => fields.iter().all(|(_, v)| literal_only(v)),
        ExprKind::NamedRec { spread: None, fields, .. } => fields.iter().all(|(_, v)| literal_only(v)),
        _ => false,
    }
}

/// Run the configured model command (WEFT_MODEL_CMD), prompt on stdin,
/// reply on stdout.
fn invoke_model(prompt: &str) -> Result<String, String> {
    let cmd = std::env::var("WEFT_MODEL_CMD")
        .map_err(|_| "no model configured (set WEFT_MODEL_CMD to a command reading a prompt on stdin)".to_string())?;
    let mut child = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", &cmd])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
    } else {
        std::process::Command::new("sh")
            .args(["-c", &cmd])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
    }
    .map_err(|e| format!("cannot start `{}`: {}", cmd, e))?;
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(prompt.as_bytes());
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().map_err(|e| format!("model command failed: {}", e))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("model command exited nonzero: {}", err.chars().take(200).collect::<String>()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn show_float(f: f64) -> String {
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{}.0", s)
    }
}

pub fn show(v: &Value) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Float(f) => show_float(*f),
        Value::Bool(b) => b.to_string(),
        Value::Text(s) => format!("{:?}", s),
        Value::Unit => "unit".into(),
        Value::List(xs) => {
            let inner: Vec<String> = xs.iter().map(show).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Rec(fs) => {
            let inner: Vec<String> = fs.iter().map(|(n, x)| format!("{}: {}", n, show(x))).collect();
            format!("{{{}}}", inner.join(", "))
        }
        Value::Ctor(name, payload) => {
            if payload.is_empty() {
                name.clone()
            } else {
                let inner: Vec<String> = payload.iter().map(show).collect();
                format!("{}({})", name, inner.join(", "))
            }
        }
        Value::Closure { .. } | Value::NamedFn(_) | Value::Builtin(_) => "<function>".into(),
        Value::Cap(c) => format!("<capability {}>", c),
    }
}

fn is_builtin(name: &str) -> bool {
    const NAMES: [&str; 41] = [
        "text_len", "text_of_int", "text_of_float", "text_of_bool", "int_of_text", "split", "join",
        "contains", "chars", "to_upper", "to_lower", "trim", "len", "list_get", "append", "map",
        "filter", "fold", "range", "reverse", "sort_by", "zip", "find", "index_of", "unwrap_or",
        "ok_or", "abs", "min", "max", "int_to_float", "float_to_int", "print", "read_line", "fs",
        "fs_read", "fs_write", "rand", "rand_int", "clock", "now_ms", "model",
    ];
    NAMES.contains(&name)
}
