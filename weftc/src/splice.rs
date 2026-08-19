// Definition-granularity merge. A patch is a Weft file containing whole
// definitions; splicing replaces base items with the same name and appends
// the rest. Two patches touching the same name is a conflict — detected by
// name, not by file position, so edits to different definitions never clash.

use crate::ast::*;
use crate::diag::line_col;
use std::collections::HashMap;

/// A top-level item located in its source, with any comment block above it.
pub struct Located {
    pub key: String,
    pub start_line: usize,
    pub end_line: usize,
    /// signature of a def, for detecting type-changing replacements
    pub sig: Option<String>,
}

fn key_of(item: &Item) -> String {
    match item {
        Item::Def(d) => format!("def {}", d.name),
        Item::TypeDef(td) => format!("type {}", td.name),
        Item::Test(t) => format!("test {}", t.name),
    }
}

fn span_of(item: &Item) -> crate::diag::Span {
    match item {
        Item::Def(d) => d.span,
        Item::TypeDef(td) => td.span,
        Item::Test(t) => t.span,
    }
}

/// Locate every top-level item, extending each upward over its comments.
pub fn locate(prog: &Program, src: &str) -> Vec<Located> {
    let lines: Vec<&str> = src.lines().collect();
    prog.items
        .iter()
        .map(|item| {
            let span = span_of(item);
            let (start, _) = line_col(src, span.start);
            let (end, _) = line_col(src, span.end);
            let mut first = start;
            while first > 1 && lines[first - 2].trim_start().starts_with('#') {
                first -= 1;
            }
            let sig = match item {
                Item::Def(d) => Some(crate::index::def_signature(d)),
                _ => None,
            };
            Located { key: key_of(item), start_line: first, end_line: end.min(lines.len()), sig }
        })
        .collect()
}

fn item_text(src: &str, loc: &Located) -> String {
    let lines: Vec<&str> = src.lines().collect();
    lines[loc.start_line - 1..loc.end_line].join("\n")
}

pub struct Patch {
    pub label: String,
    pub src: String,
    pub prog: Program,
}

pub struct Merge {
    pub text: String,
    pub replaced: Vec<String>,
    pub added: Vec<String>,
    /// replacements that changed a definition's signature — callers may break
    pub signature_changes: Vec<String>,
}

/// Merge patches into a base program by definition name.
pub fn splice(base_src: &str, base_prog: &Program, patches: &[Patch]) -> Result<Merge, String> {
    // conflict detection: the same name touched by two patches
    let mut owner: HashMap<String, String> = HashMap::new();
    let mut conflicts: Vec<String> = Vec::new();
    for p in patches {
        for loc in locate(&p.prog, &p.src) {
            match owner.get(&loc.key) {
                Some(first) if first != &p.label => {
                    conflicts.push(format!("  {} — touched by {} and {}", loc.key, first, p.label));
                }
                _ => {
                    owner.insert(loc.key.clone(), p.label.clone());
                }
            }
        }
    }
    if !conflicts.is_empty() {
        conflicts.sort();
        conflicts.dedup();
        return Err(format!(
            "conflict: {} definition(s) edited by more than one patch\n{}",
            conflicts.len(),
            conflicts.join("\n")
        ));
    }

    let base_items = locate(base_prog, base_src);
    let base_by_key: HashMap<&str, &Located> =
        base_items.iter().map(|l| (l.key.as_str(), l)).collect();

    // (start_line, end_line, replacement text) for items the base already has
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut appends: Vec<String> = Vec::new();
    let mut replaced: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    let mut signature_changes: Vec<String> = Vec::new();

    for p in patches {
        for loc in locate(&p.prog, &p.src) {
            let text = item_text(&p.src, &loc);
            match base_by_key.get(loc.key.as_str()) {
                Some(target) => {
                    // Replacing by name is only safe if the type is unchanged;
                    // otherwise callers outside the patch may silently break.
                    if let (Some(new_sig), Some(old_sig)) = (&loc.sig, &target.sig) {
                        if new_sig != old_sig {
                            signature_changes.push(format!(
                                "  {} ({})\n      was: {}\n      now: {}",
                                loc.key, p.label, old_sig, new_sig
                            ));
                        }
                    }
                    edits.push((target.start_line, target.end_line, text));
                    replaced.push(loc.key.clone());
                }
                None => {
                    appends.push(text);
                    added.push(loc.key.clone());
                }
            }
        }
    }

    // apply replacements bottom-up so earlier line numbers stay valid
    edits.sort_by(|a, b| b.0.cmp(&a.0));
    let mut lines: Vec<String> = base_src.lines().map(|s| s.to_string()).collect();
    for (start, end, text) in &edits {
        let replacement: Vec<String> = text.lines().map(|s| s.to_string()).collect();
        lines.splice(start - 1..*end, replacement);
    }

    let mut out = lines.join("\n");
    if !appends.is_empty() {
        out.push_str("\n\n# ------------------------------------------------------------\n");
        out.push_str("# Spliced in by `weftc splice`\n");
        out.push_str("# ------------------------------------------------------------\n\n");
        out.push_str(&appends.join("\n\n"));
        out.push('\n');
    } else if !out.ends_with('\n') {
        out.push('\n');
    }

    replaced.sort();
    added.sort();
    signature_changes.sort();
    Ok(Merge { text: out, replaced, added, signature_changes })
}
