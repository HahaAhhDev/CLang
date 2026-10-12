mod bytecode;
mod codegen;
mod compile;
mod error;
mod fmt;
mod interp;
mod parser;
mod value;

use codegen::native_build;
use compile::{compile_source, load_maybe_compiled};
use interp::Interp;

fn print_usage() {
    println!("usage:");
    println!("  clng <file.clng|file.clngb> [args...]   run a file");
    println!("  clng compile <file.clng> [-o out] [--native]  build it");
    println!("  clng fmt <file.clng>                    print canonical form");
    println!("  clng repl                               interactive loop");
    println!("  clng test [path...]                     run test_ functions");
    println!("  clng doc <file.clng>                    print doc stubs");
}

fn report(path: &str, err: &error::ClangError) {
    eprintln!("{}", err);
    if let Ok(text) = std::fs::read_to_string(path) {
        if text.starts_with("CLNGB1") {
            return;
        }
        let lines: Vec<&str> = text.lines().collect();
        if err.line >= 1 && err.line <= lines.len() {
            let body = lines[err.line - 1];
            eprintln!(" --> {}:{}", path, err.line);
            eprintln!("  | {}", body);
            eprintln!("  | {}^", " ".repeat(body.chars().count().min(60)));
        }
    }
}

fn collect_clng(dir: &str, out: &mut Vec<String>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_clng(&path.to_string_lossy(), out);
            } else if path.extension().map(|e| e == "clng").unwrap_or(false) {
                out.push(path.to_string_lossy().to_string());
            }
        }
    }
}

fn repl() {
    use std::io::{self, BufRead, Write};
    let mut ip = Interp::new();
    println!("clang repl, type exit to leave");
    let stdin = io::stdin();
    let mut pending = String::new();
    let mut depth: i32 = 0;
    print!("> ");
    let _ = io::stdout().flush();
    for line in stdin.lock().lines() {
        let line = line.unwrap_or_default();
        if line.trim() == "exit" && pending.is_empty() {
            break;
        }
        depth += line.chars().filter(|c| *c == '{').count() as i32;
        depth -= line.chars().filter(|c| *c == '}').count() as i32;
        pending.push_str(&line);
        pending.push('\n');
        if depth > 0 {
            print!(".. ");
            let _ = io::stdout().flush();
            continue;
        }
        let chunk = std::mem::take(&mut pending);
        depth = 0;
        if chunk.trim().is_empty() {
            print!("> ");
            let _ = io::stdout().flush();
            continue;
        }
        match ip.run_source(&chunk) {
            Ok(()) => {}
            Err(e) => eprintln!("{}", e),
        }
        print!("> ");
        let _ = io::stdout().flush();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(2);
    }
    if args[1] == "repl" {
        repl();
        return;
    }
    if args[1] == "test" {
        let mut targets: Vec<String> = Vec::new();
        if args.len() > 2 {
            for a in &args[2..] {
                targets.push(a.clone());
            }
        } else {
            targets.push("tests".to_string());
        }
        let mut files: Vec<String> = Vec::new();
        for target in targets {
            let meta = std::fs::metadata(&target);
            match meta {
                Ok(m) if m.is_dir() => {
                    let mut entries: Vec<String> = Vec::new();
                    collect_clng(&target, &mut entries);
                    entries.sort();
                    files.extend(entries);
                }
                Ok(_) => files.push(target),
                Err(_) => {
                    eprintln!("clang error at line 1: no such test path '{}'", target);
                    std::process::exit(1);
                }
            }
        }
        if files.is_empty() {
            println!("no test files found");
            return;
        }
        let mut pass = 0;
        let mut fail = 0;
        for f in files {
            let src = match std::fs::read_to_string(&f) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}: cannot read: {}", f, e);
                    fail += 1;
                    continue;
                }
            };
            let mut ip = Interp::new();
            match ip.run_test_fns(&src) {
                Ok(results) if results.is_empty() => {
                    println!("{}: no test_ functions", f);
                }
                Ok(results) => {
                    for (name, r) in results {
                        match r {
                            Ok(()) => {
                                println!("{} {} ok", f, name);
                                pass += 1;
                            }
                            Err(e) => {
                                println!("{} {} FAIL\n{}", f, name, e);
                                fail += 1;
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("{}: setup FAIL\n{}", f, e);
                    fail += 1;
                }
            }
        }
        println!("{} passed, {} failed", pass, fail);
        if fail > 0 {
            std::process::exit(1);
        }
        return;
    }
    if args[1] == "doc" {
        if args.len() < 3 {
            eprintln!("clang error at line 1: missing input file");
            eprintln!("help: run clng doc app.clng");
            std::process::exit(1);
        }
        let path = args[2].clone();
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("clang error at line 1: cannot read '{}': {}", path, e);
                std::process::exit(1);
            }
        };
        print!("{}", fmt::doc_stubs(&src));
        return;
    }
    if args[1] == "fmt" {
        if args.len() < 3 {
            eprintln!("clang error at line 1: missing input file");
            eprintln!("help: run clng fmt app.clng");
            std::process::exit(1);
        }
        let path = args[2].clone();
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("clang error at line 1: cannot read '{}': {}", path, e);
                std::process::exit(1);
            }
        };
        match fmt::format_src(&src) {
            Ok(out) => print!("{}", out),
            Err(e) => {
                report(&path, &e);
                std::process::exit(1);
            }
        }
        return;
    }
    if args[1] == "compile" {
        if args.len() < 3 {
            eprintln!("clang error at line 1: missing input file");
            eprintln!("help: run clng compile app.clng");
            std::process::exit(1);
        }
        let native = args.iter().any(|a| a == "--native");
        let filtered: Vec<String> = args.iter().filter(|a| *a != "--native").cloned().collect();
        let src = filtered[2].clone();
        let mut out = format!("{}.clngb", src.trim_end_matches(".clng"));
        if let Some(i) = filtered.iter().position(|a| a == "-o") {
            if i + 1 < filtered.len() {
                out = filtered[i + 1].clone();
            }
        }
        if native {
            let bin = if out.ends_with(".clngb") {
                out.trim_end_matches(".clngb").to_string()
            } else {
                out.clone()
            };
            if let Err(e) = native_build(&src, &bin) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            return;
        }
        if let Err(e) = compile_source(&src, &out) {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        return;
    }
    if args[1] == "-h" || args[1] == "--help" {
        print_usage();
        return;
    }
    let path = args[1].clone();
    let src = match load_maybe_compiled(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    let mut ip = Interp::new();
    if args.len() > 2 {
        ip.set_cli(args[2..].to_vec());
    }
    if let Err(e) = ip.run_source(&src) {
        report(&path, &e);
        std::process::exit(1);
    }
}
