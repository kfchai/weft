mod ast;
mod check;
mod diag;
mod eval;
mod index;
mod lexer;
mod parser;
mod splice;

use std::process::ExitCode;

fn main() -> ExitCode {
    // Deep Weft recursion multiplies into many Rust frames; the Windows main
    // thread gets 1MB, so do the real work on a thread with a roomy stack.
    let code = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(real_main)
        .expect("spawn interpreter thread")
        .join()
        .expect("interpreter thread panicked");
    ExitCode::from(code)
}

fn real_main() -> u8 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let rest: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();

    let (cmd, files) = match rest.split_first() {
        Some((c, f)) if !f.is_empty() => (c.as_str(), f),
        _ => {
            eprintln!("usage: weftc <parse|check|run|test> [--json] <file.weft>...");
            return 2;
        }
    };

    match cmd {
        "parse" => {
            let mut failed = false;
            for file in files {
                match parse_file(file, json) {
                    Ok(n) => {
                        if !json {
                            println!("{}: ok ({} top-level items)", file, n);
                        } else {
                            println!("{{\"file\":\"{}\",\"ok\":true,\"items\":{}}}", file.replace('\\', "/"), n);
                        }
                    }
                    Err(msg) => {
                        eprintln!("{}", msg);
                        failed = true;
                    }
                }
            }
            if failed { 1 } else { 0 }
        }
        "check" => {
            let mut failed = false;
            for file in files {
                if !check_file(file, json) {
                    failed = true;
                }
            }
            if failed { 1 } else { 0 }
        }
        "run" => {
            let file = files[0];
            match load_checked(file, json) {
                Some((prog, src)) => {
                    let mut interp = eval::Interp::new(&prog);
                    match interp.run_main() {
                        Ok(code) => code.clamp(0, 255) as u8,
                        Err(e) => {
                            eprintln!("{}", render(&e.diag, &src, file, json));
                            101
                        }
                    }
                }
                None => 1,
            }
        }
        "test" => {
            let mut all_passed = true;
            for file in files {
                match load_checked(file, json) {
                    Some((prog, _src)) => {
                        let mut interp = eval::Interp::new(&prog);
                        let outcomes = interp.run_tests(&prog);
                        let passed = outcomes.iter().filter(|o| o.passed).count();
                        let total = outcomes.len();
                        for o in &outcomes {
                            if !o.passed {
                                all_passed = false;
                                if json {
                                    println!(
                                        "{{\"file\":\"{}\",\"test\":\"{}\",\"passed\":false,\"detail\":\"{}\"}}",
                                        file.replace('\\', "/"),
                                        o.name,
                                        o.detail.clone().unwrap_or_default().replace('\\', "\\\\").replace('"', "\\\"")
                                    );
                                } else {
                                    println!(
                                        "  FAIL \"{}\": {}",
                                        o.name,
                                        o.detail.clone().unwrap_or_default()
                                    );
                                }
                            }
                        }
                        if json {
                            println!(
                                "{{\"file\":\"{}\",\"tests\":{},\"passed\":{}}}",
                                file.replace('\\', "/"),
                                total,
                                passed
                            );
                        } else {
                            println!("{}: {}/{} tests passed", file, passed, total);
                        }
                    }
                    None => {
                        all_passed = false;
                    }
                }
            }
            if all_passed { 0 } else { 1 }
        }
        "repair-context" => repair_context(files[0]),
        "splice" => {
            if files.len() < 2 {
                eprintln!("usage: weftc splice <base.weft> <patch.weft>... [--write]");
                return 2;
            }
            let write = args.iter().any(|a| a == "--write");
            let base_path = files[0];
            let (base_src, base_prog) = match load_ast(base_path) {
                Some(x) => x,
                None => return 1,
            };
            let mut patches = Vec::new();
            for p in &files[1..] {
                match load_ast(p) {
                    Some((src, prog)) => patches.push(splice::Patch {
                        label: std::path::Path::new(p.as_str())
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| p.to_string()),
                        src,
                        prog,
                    }),
                    None => return 1,
                }
            }
            match splice::splice(&base_src, &base_prog, &patches) {
                Ok(m) => {
                    if !m.signature_changes.is_empty() {
                        eprintln!(
                            "warning: {} replacement(s) change a definition's signature; callers outside the patch may break:",
                            m.signature_changes.len()
                        );
                        for c in &m.signature_changes {
                            eprintln!("{}", c);
                        }
                    }
                    if write {
                        if let Err(e) = std::fs::write(base_path, &m.text) {
                            eprintln!("cannot write {}: {}", base_path, e);
                            return 1;
                        }
                        println!(
                            "spliced into {}: {} replaced, {} added",
                            base_path,
                            m.replaced.len(),
                            m.added.len()
                        );
                        for k in &m.replaced {
                            println!("  ~ {}", k);
                        }
                        for k in &m.added {
                            println!("  + {}", k);
                        }
                    } else {
                        print!("{}", m.text);
                    }
                    0
                }
                Err(e) => {
                    eprintln!("{}", e);
                    1
                }
            }
        }
        "skeleton" | "graph" | "ctx" => {
            let file = files[0];
            let src = match std::fs::read_to_string(file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}: cannot read: {}", file, e);
                    return 1;
                }
            };
            let toks = match lexer::lex(&src) {
                Ok(t) => t,
                Err(d) => {
                    eprintln!("{}", render(&d, &src, file, json));
                    return 1;
                }
            };
            let mut p = parser::Parser::new(toks);
            let prog = match p.parse_program() {
                Ok(prog) => prog,
                Err(d) => {
                    eprintln!("{}", render(&d, &src, file, json));
                    return 1;
                }
            };
            match cmd {
                "skeleton" => {
                    print!("{}", index::skeleton(&prog, &src));
                    0
                }
                "graph" => {
                    print!("{}", index::graph(&prog));
                    0
                }
                _ => {
                    let targets: Vec<String> = files[1..].iter().map(|s| s.to_string()).collect();
                    if targets.is_empty() {
                        eprintln!("usage: weftc ctx <file.weft> <def-name>...");
                        return 2;
                    }
                    match index::ctx(&prog, &src, &targets) {
                        Ok(text) => {
                            print!("{}", text);
                            0
                        }
                        Err(e) => {
                            eprintln!("ctx: {}", e);
                            1
                        }
                    }
                }
            }
        }
        other => {
            eprintln!("unknown command `{}`; expected parse, check, run, test, repair-context, skeleton, graph, or ctx", other);
            2
        }
    }
}

/// The loop-closer [W41]: find the first failure (parse, check, or test),
/// and emit a self-contained repair payload — diagnostic + the cited spec
/// rule's text + a source excerpt — ready to paste to a model.
fn repair_context(path: &str) -> u8 {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: cannot read: {}", path, e);
            return 1;
        }
    };
    let diag: Option<diag::Diag> = (|| {
        let toks = match lexer::lex(&src) {
            Ok(t) => t,
            Err(d) => return Some(d),
        };
        let mut p = parser::Parser::new(toks);
        let prog = match p.parse_program() {
            Ok(prog) => prog,
            Err(d) => return Some(d),
        };
        let result = check::check_program(&prog);
        if let Some(d) = result.diags.into_iter().next() {
            return Some(d);
        }
        let mut interp = eval::Interp::new(&prog);
        let outcomes = interp.run_tests(&prog);
        outcomes.into_iter().find(|o| !o.passed).map(|o| {
            let rule = if o.property { "W35" } else { "W34" };
            diag::Diag::new(
                rule,
                format!("test \"{}\" failed: {}", o.name, o.detail.unwrap_or_default()),
                o.span,
            )
        })
    })();

    let d = match diag {
        Some(d) => d,
        None => {
            println!("No failures in {} — nothing to repair.", path);
            return 0;
        }
    };

    println!("# Weft repair context for {}\n", path);
    println!("## Failure\n\n```json\n{}\n```\n", d.to_json(&src));
    match find_spec(path).and_then(|spec_path| {
        std::fs::read_to_string(&spec_path)
            .ok()
            .and_then(|spec| spec_rule_excerpt(&spec, &d.rule))
    }) {
        Some(excerpt) => println!("## The rule you broke — [{}]\n\n{}\n", d.rule, excerpt),
        None => println!("## Rule [{}]\n\n(SPEC.md not found near the program file)\n", d.rule),
    }
    println!("## Source excerpt\n\n```weft\n{}```\n", source_excerpt(&src, d.span));
    println!("## Instruction\n\nFix the program so this failure disappears, keeping all other behavior. Reply with the complete corrected single-file program only.");
    0
}

/// Look for SPEC.md next to the program file, then in ancestor directories.
fn find_spec(path: &str) -> Option<std::path::PathBuf> {
    let abs = std::path::Path::new(path).canonicalize().ok()?;
    let mut dir = abs.parent()?.to_path_buf();
    for _ in 0..5 {
        let candidate = dir.join("SPEC.md");
        if candidate.exists() {
            return Some(candidate);
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

/// Extract one rule's bullet from SPEC.md: from the line containing `[W#]**`
/// until the next rule bullet or section heading.
fn spec_rule_excerpt(spec: &str, rule: &str) -> Option<String> {
    let marker = format!("[{}]**", rule);
    let mut out: Vec<&str> = Vec::new();
    let mut in_rule = false;
    for line in spec.lines() {
        if in_rule {
            let t = line.trim_start();
            if t.starts_with("- **[W") || t.starts_with("## ") || t.starts_with("# ") {
                break;
            }
            out.push(line);
        } else if line.contains(&marker) {
            in_rule = true;
            out.push(line);
        }
    }
    while matches!(out.last(), Some(l) if l.trim().is_empty()) {
        out.pop();
    }
    if out.is_empty() { None } else { Some(out.join("\n")) }
}

/// Numbered source lines around the span, error line marked with `>`.
fn source_excerpt(src: &str, span: diag::Span) -> String {
    let (err_line, _) = diag::line_col(src, span.start);
    let lines: Vec<&str> = src.lines().collect();
    let lo = err_line.saturating_sub(4).max(1);
    let hi = (err_line + 3).min(lines.len());
    let mut out = String::new();
    for n in lo..=hi {
        let marker = if n == err_line { ">" } else { " " };
        out.push_str(&format!("{} {:>4} | {}\n", marker, n, lines[n - 1]));
    }
    out
}

/// Parse only (no typecheck) — enough for source-level tooling.
fn load_ast(path: &str) -> Option<(String, ast::Program)> {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: cannot read: {}", path, e);
            return None;
        }
    };
    let toks = match lexer::lex(&src) {
        Ok(t) => t,
        Err(d) => {
            eprintln!("{}", d.render_human(&src, path));
            return None;
        }
    };
    let mut p = parser::Parser::new(toks);
    match p.parse_program() {
        Ok(prog) => Some((src, prog)),
        Err(d) => {
            eprintln!("{}", d.render_human(&src, path));
            None
        }
    }
}

/// Parse + typecheck; returns the program only if it is clean (holes allowed).
fn load_checked(path: &str, json: bool) -> Option<(ast::Program, String)> {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: cannot read: {}", path, e);
            return None;
        }
    };
    let toks = match lexer::lex(&src) {
        Ok(t) => t,
        Err(d) => {
            eprintln!("{}", render(&d, &src, path, json));
            return None;
        }
    };
    let mut p = parser::Parser::new(toks);
    let prog = match p.parse_program() {
        Ok(prog) => prog,
        Err(d) => {
            eprintln!("{}", render(&d, &src, path, json));
            return None;
        }
    };
    let result = check::check_program(&prog);
    if !result.diags.is_empty() {
        for d in &result.diags {
            eprintln!("{}", render(d, &src, path, json));
        }
        return None;
    }
    Some((prog, src))
}

fn check_file(path: &str, json: bool) -> bool {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: cannot read: {}", path, e);
            return false;
        }
    };
    let toks = match lexer::lex(&src) {
        Ok(t) => t,
        Err(d) => {
            eprintln!("{}", render(&d, &src, path, json));
            return false;
        }
    };
    let mut p = parser::Parser::new(toks);
    let prog = match p.parse_program() {
        Ok(prog) => prog,
        Err(d) => {
            eprintln!("{}", render(&d, &src, path, json));
            return false;
        }
    };
    let result = check::check_program(&prog);
    for d in &result.diags {
        eprintln!("{}", render(d, &src, path, json));
    }
    for h in &result.holes {
        let (line, col) = diag::line_col(&src, h.span.start);
        if json {
            println!(
                "{{\"file\":\"{}\",\"note\":\"hole\",\"rule\":\"W27\",\"name\":\"{}\",\"type\":\"{}\",\"line\":{},\"col\":{}}}",
                path.replace('\\', "/"),
                h.name,
                h.ty.replace('"', "\\\""),
                line,
                col
            );
        } else {
            println!("note[W27] {}:{}:{}: hole `?{}` has type {}", path, line, col, h.name, h.ty);
        }
    }
    if result.diags.is_empty() {
        if !json {
            println!(
                "{}: ok ({} defs, {} tests{})",
                path,
                result.defs,
                result.tests,
                if result.holes.is_empty() {
                    String::new()
                } else {
                    format!(", {} holes", result.holes.len())
                }
            );
        } else {
            println!(
                "{{\"file\":\"{}\",\"ok\":true,\"defs\":{},\"tests\":{},\"holes\":{}}}",
                path.replace('\\', "/"),
                result.defs,
                result.tests,
                result.holes.len()
            );
        }
        true
    } else {
        false
    }
}

fn parse_file(path: &str, json: bool) -> Result<usize, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: cannot read: {}", path, e))?;
    let toks = lexer::lex(&src).map_err(|d| render(&d, &src, path, json))?;
    let mut p = parser::Parser::new(toks);
    let prog = p.parse_program().map_err(|d| render(&d, &src, path, json))?;
    Ok(prog.items.len())
}

fn render(d: &diag::Diag, src: &str, path: &str, json: bool) -> String {
    if json {
        format!("{{\"file\":\"{}\",\"ok\":false,\"error\":{}}}", path.replace('\\', "/"), d.to_json(src))
    } else {
        d.render_human(src, path)
    }
}
