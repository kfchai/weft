// Phase 4 index: compiler-derived views of a program for LLM context assembly.
// skeleton = signatures + docs (the map); graph = dependencies (the slicer);
// ctx = full bodies for a target slice + skeletons for everything else.

use crate::ast::*;
use crate::diag::line_col;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub fn type_text(te: &TypeExpr) -> String {
    match te {
        TypeExpr::Name(n, args, _) => {
            if args.is_empty() {
                n.clone()
            } else {
                let inner: Vec<String> = args.iter().map(type_text).collect();
                format!("{}[{}]", n, inner.join(", "))
            }
        }
        TypeExpr::Record(fields, _) => {
            let inner: Vec<String> = fields.iter().map(|(n, t)| format!("{}: {}", n, type_text(t))).collect();
            format!("{{{}}}", inner.join(", "))
        }
        TypeExpr::Fn(args, ret, _) => {
            let inner: Vec<String> = args.iter().map(type_text).collect();
            format!("({}) -> {}", inner.join(", "), type_text(ret))
        }
    }
}

fn param_text(p: &Param) -> String {
    match &p.contract {
        Some(c) => format!("{}: {} where {}", p.name, type_text(&p.ty), expr_text(c)),
        None => format!("{}: {}", p.name, type_text(&p.ty)),
    }
}

pub fn def_signature(d: &Def) -> String {
    let tp = if d.tparams.is_empty() {
        String::new()
    } else {
        format!("[{}]", d.tparams.join(", "))
    };
    match &d.params {
        Some(ps) => {
            let inner: Vec<String> = ps.iter().map(param_text).collect();
            format!("def {}{}({}) -> {}", d.name, tp, inner.join(", "), type_text(&d.ty))
        }
        None => format!("def {}: {}", d.name, type_text(&d.ty)),
    }
}

pub fn typedef_text(td: &TypeDef) -> String {
    match &td.decl {
        TypeDecl::Alias(te) => format!("type {} = {}", td.name, type_text(te)),
        TypeDecl::Variants(vs) => {
            let inner: Vec<String> = vs
                .iter()
                .map(|v| {
                    if v.payload.is_empty() {
                        v.name.clone()
                    } else {
                        let ps: Vec<String> = v.payload.iter().map(type_text).collect();
                        format!("{}({})", v.name, ps.join(", "))
                    }
                })
                .collect();
            format!("type {} = {}", td.name, inner.join(" | "))
        }
        TypeDecl::Nominal { fields, invariant } => {
            let inner: Vec<String> = fields.iter().map(|(n, t)| format!("{}: {}", n, type_text(t))).collect();
            format!("type {} = {{{}}} where {}", td.name, inner.join(", "), expr_text(invariant))
        }
    }
}

/// First line of the contiguous `#` comment block directly above an item.
fn doc_line(src: &str, span_start: usize) -> Option<String> {
    let (line, _) = line_col(src, span_start);
    let lines: Vec<&str> = src.lines().collect();
    let mut first_doc: Option<String> = None;
    let mut n = line.saturating_sub(1); // 0-based index of the line above
    while n >= 1 {
        let t = lines[n - 1].trim();
        if let Some(stripped) = t.strip_prefix('#') {
            first_doc = Some(stripped.trim().to_string());
            n -= 1;
        } else {
            break;
        }
    }
    first_doc.filter(|s| !s.is_empty())
}

/// Slice an item's source, extended upward to include its comment block.
fn item_source<'a>(src: &'a str, span: crate::diag::Span) -> String {
    let (start_line, _) = line_col(src, span.start);
    let (end_line, _) = line_col(src, span.end);
    let lines: Vec<&str> = src.lines().collect();
    let mut first = start_line;
    while first > 1 && lines[first - 2].trim_start().starts_with('#') {
        first -= 1;
    }
    lines[first - 1..end_line.min(lines.len())].join("\n")
}

/// Names a body references, filtered to program-defined names.
fn refs_of_expr(e: &Expr, known: &HashSet<String>, out: &mut BTreeSet<String>) {
    match &e.kind {
        ExprKind::Var(n) => {
            if known.contains(n) {
                out.insert(n.clone());
            }
        }
        ExprKind::Ctor(n, args) => {
            if known.contains(n) {
                out.insert(n.clone());
            }
            for a in args {
                refs_of_expr(a, known, out);
            }
        }
        ExprKind::NamedRec { name, spread, fields } => {
            if known.contains(name) {
                out.insert(name.clone());
            }
            if let Some(b) = spread {
                refs_of_expr(b, known, out);
            }
            for (_, v) in fields {
                refs_of_expr(v, known, out);
            }
        }
        ExprKind::Record { spread, fields } => {
            if let Some(b) = spread {
                refs_of_expr(b, known, out);
            }
            for (_, v) in fields {
                refs_of_expr(v, known, out);
            }
        }
        ExprKind::List(es) => {
            for x in es {
                refs_of_expr(x, known, out);
            }
        }
        ExprKind::Field(b, _) => refs_of_expr(b, known, out),
        ExprKind::Call(c, args) => {
            refs_of_expr(c, known, out);
            for a in args {
                refs_of_expr(a, known, out);
            }
        }
        ExprKind::Lambda { body, .. } => refs_of_expr(body, known, out),
        ExprKind::Block { lets, tail } => {
            for (_, v) in lets {
                refs_of_expr(v, known, out);
            }
            refs_of_expr(tail, known, out);
        }
        ExprKind::If { cond, then, els } => {
            refs_of_expr(cond, known, out);
            refs_of_expr(then, known, out);
            refs_of_expr(els, known, out);
        }
        ExprKind::Match { scrutinee, arms } => {
            refs_of_expr(scrutinee, known, out);
            for (pat, body) in arms {
                refs_of_pattern(pat, known, out);
                refs_of_expr(body, known, out);
            }
        }
        ExprKind::Bin(_, l, r) => {
            refs_of_expr(l, known, out);
            refs_of_expr(r, known, out);
        }
        ExprKind::NotOp(i) | ExprKind::NegOp(i) | ExprKind::Propagate(i) => refs_of_expr(i, known, out),
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Text(_)
        | ExprKind::Unit
        | ExprKind::Hole(_) => {}
    }
}

fn refs_of_pattern(p: &Pattern, known: &HashSet<String>, out: &mut BTreeSet<String>) {
    match &p.kind {
        PatKind::Ctor(n, subs) => {
            if known.contains(n) {
                out.insert(n.clone());
            }
            for s in subs {
                refs_of_pattern(s, known, out);
            }
        }
        PatKind::List { heads, .. } => {
            for h in heads {
                refs_of_pattern(h, known, out);
            }
        }
        _ => {}
    }
}

fn refs_of_type(te: &TypeExpr, known: &HashSet<String>, out: &mut BTreeSet<String>) {
    match te {
        TypeExpr::Name(n, args, _) => {
            if known.contains(n) {
                out.insert(n.clone());
            }
            for a in args {
                refs_of_type(a, known, out);
            }
        }
        TypeExpr::Record(fields, _) => {
            for (_, t) in fields {
                refs_of_type(t, known, out);
            }
        }
        TypeExpr::Fn(args, ret, _) => {
            for a in args {
                refs_of_type(a, known, out);
            }
            refs_of_type(ret, known, out);
        }
    }
}

pub struct Index {
    /// name -> everything it references (defs, types via ctors/annotations)
    pub deps: BTreeMap<String, BTreeSet<String>>,
    /// ctor name -> owning type name
    pub ctor_owner: HashMap<String, String>,
    pub known: HashSet<String>,
}

pub fn build_index(prog: &Program) -> Index {
    let mut known: HashSet<String> = HashSet::new();
    let mut ctor_owner: HashMap<String, String> = HashMap::new();
    for item in &prog.items {
        match item {
            Item::Def(d) => {
                known.insert(d.name.clone());
            }
            Item::TypeDef(td) => {
                known.insert(td.name.clone());
                if let TypeDecl::Variants(vs) = &td.decl {
                    for v in vs {
                        ctor_owner.insert(v.name.clone(), td.name.clone());
                    }
                }
            }
            Item::Test(_) => {}
        }
    }
    // ctors are "known" so references resolve, then rewritten to their type
    let mut known_plus: HashSet<String> = known.clone();
    for c in ctor_owner.keys() {
        known_plus.insert(c.clone());
    }

    let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for item in &prog.items {
        if let Item::Def(d) = item {
            let mut out = BTreeSet::new();
            if let Some(ps) = &d.params {
                for p in ps {
                    refs_of_type(&p.ty, &known, &mut out);
                    if let Some(c) = &p.contract {
                        refs_of_expr(c, &known_plus, &mut out);
                    }
                }
            }
            refs_of_type(&d.ty, &known, &mut out);
            refs_of_expr(&d.body, &known_plus, &mut out);
            // rewrite ctor refs to their owning type; drop self-references
            let rewritten: BTreeSet<String> = out
                .into_iter()
                .map(|n| ctor_owner.get(&n).cloned().unwrap_or(n))
                .filter(|n| n != &d.name)
                .collect();
            deps.insert(d.name.clone(), rewritten);
        }
    }
    Index { deps, ctor_owner, known }
}

pub fn skeleton(prog: &Program, src: &str) -> String {
    let mut out = String::new();
    let total_lines = src.lines().count();
    let defs = prog.items.iter().filter(|i| matches!(i, Item::Def(_))).count();
    let tests = prog.items.iter().filter(|i| matches!(i, Item::Test(_))).count();
    out.push_str(&format!("# skeleton: {} lines, {} defs, {} tests\n\n", total_lines, defs, tests));
    for item in &prog.items {
        match item {
            Item::TypeDef(td) => {
                let (line, _) = line_col(src, td.span.start);
                out.push_str(&format!("L{:<4} {}\n", line, typedef_text(td)));
            }
            Item::Def(d) => {
                let (line, _) = line_col(src, d.span.start);
                let doc = doc_line(src, d.span.start).map(|s| format!("   # {}", s)).unwrap_or_default();
                out.push_str(&format!("L{:<4} {}{}\n", line, def_signature(d), doc));
            }
            Item::Test(t) => {
                let (line, _) = line_col(src, t.span.start);
                let kind = if t.params.is_empty() { "test" } else { "property test" };
                out.push_str(&format!("L{:<4} {} \"{}\"\n", line, kind, t.name));
            }
        }
    }
    out
}

pub fn graph(prog: &Program) -> String {
    let idx = build_index(prog);
    let mut out = String::new();
    out.push_str("# dependency graph (name -> references)\n\n");
    for (name, deps) in &idx.deps {
        if deps.is_empty() {
            out.push_str(&format!("{} ->\n", name));
        } else {
            let list: Vec<&str> = deps.iter().map(|s| s.as_str()).collect();
            out.push_str(&format!("{} -> {}\n", name, list.join(", ")));
        }
    }
    out
}

/// Assemble a context slice for modifying `targets`:
/// map of everything + full source for targets, their direct dependencies,
/// referenced types, caller signatures, and tests that mention a target.
pub fn ctx(prog: &Program, src: &str, targets: &[String]) -> Result<String, String> {
    let idx = build_index(prog);
    for t in targets {
        if !idx.known.contains(t) {
            return Err(format!("unknown definition `{}`", t));
        }
    }
    let target_set: BTreeSet<String> = targets.iter().cloned().collect();

    // slice = targets + one hop of dependencies (defs and types)
    let mut slice: BTreeSet<String> = target_set.clone();
    for t in &target_set {
        if let Some(ds) = idx.deps.get(t) {
            for d in ds {
                slice.insert(d.clone());
            }
        }
    }
    // callers (signature only)
    let mut callers: BTreeSet<String> = BTreeSet::new();
    for (name, deps) in &idx.deps {
        if target_set.iter().any(|t| deps.contains(t)) && !slice.contains(name) {
            callers.insert(name.clone());
        }
    }

    let mut out = String::new();
    out.push_str(&format!("# Context slice for: {}\n", targets.join(", ")));
    out.push_str("# Everything needed to modify these definitions. Signatures below are\n# complete and honest [W2]; trust them without reading further.\n\n");

    out.push_str("## Program map (all signatures)\n\n");
    out.push_str(&skeleton(prog, src));

    out.push_str("\n## Full definitions (targets + direct dependencies)\n\n```weft\n");
    let mut emitted_lines = 0usize;
    for item in &prog.items {
        let (name, span) = match item {
            Item::Def(d) => (d.name.clone(), d.span),
            Item::TypeDef(td) => (td.name.clone(), td.span),
            Item::Test(_) => continue,
        };
        if slice.contains(&name) {
            let s = item_source(src, span);
            emitted_lines += s.lines().count() + 1;
            out.push_str(&s);
            out.push_str("\n\n");
        }
    }
    out.push_str("```\n");

    if !callers.is_empty() {
        out.push_str("\n## Callers of the targets (signatures only — do not break these)\n\n");
        for item in &prog.items {
            if let Item::Def(d) = item {
                if callers.contains(&d.name) {
                    out.push_str(&format!("{}\n", def_signature(d)));
                }
            }
        }
    }

    // tests that mention a target OR a caller of a target — a change to a
    // target is observable through its callers, so those tests are in scope
    let mut watch: BTreeSet<String> = target_set.clone();
    for c in &callers {
        watch.insert(c.clone());
    }
    let mut test_srcs: Vec<String> = Vec::new();
    for item in &prog.items {
        if let Item::Test(t) = item {
            let mut refs = BTreeSet::new();
            let known_plus: HashSet<String> = idx.known.iter().cloned().chain(idx.ctor_owner.keys().cloned()).collect();
            refs_of_expr(&t.body, &known_plus, &mut refs);
            let refs: BTreeSet<String> = refs
                .into_iter()
                .map(|n| idx.ctor_owner.get(&n).cloned().unwrap_or(n))
                .collect();
            if watch.iter().any(|w| refs.contains(w)) {
                test_srcs.push(item_source(src, t.span));
            }
        }
    }
    if !test_srcs.is_empty() {
        out.push_str("\n## Existing tests touching the targets or their callers (must keep passing)\n\n```weft\n");
        for s in &test_srcs {
            emitted_lines += s.lines().count() + 1;
            out.push_str(s);
            out.push_str("\n\n");
        }
        out.push_str("```\n");
    }

    let total = src.lines().count().max(1);
    let ctx_lines = out.lines().count();
    out.push_str(&format!(
        "\n## Stats\nfull program: {} lines · full bodies emitted: {} lines · whole context: {} lines ({}% of a full-file read)\n",
        total,
        emitted_lines,
        ctx_lines,
        ctx_lines * 100 / total
    ));
    Ok(out)
}
