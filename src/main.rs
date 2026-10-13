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
    println!("  clng -e 'emit(1)' [args...]         run inline code");
    println!("  clng - [args...]                     run stdin, heredocs work");
    println!("  clng compile <file.clng> [-o out] [--native]  build it");
    println!("  clng fmt <file.clng>                    print canonical form");
    println!("  clng repl                               interactive loop");
    println!("  clng test [path...]                     run test_ functions");
    println!("  clng bench [path...]                    time VM versus walk");
    println!("  flags: --trace                          step log to stderr");
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
    let raw: Vec<String> = std::env::args().collect();
    let trace = raw.iter().any(|a| a == "--trace");
    let args: Vec<String> = raw.into_iter().filter(|a| a != "--trace").collect();
    if trace {
        std::env::set_var("CLNG_TRACE", "1");
    }
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
            if std::env::var("CLNG_TRACE").unwrap_or_default() == "1" {
                ip.trace = true;
            }
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
                                if ip.trace {
                                    let tail: Vec<&String> = ip.trace_log.iter().rev().take(15).collect();
                                    eprintln!("trace tail (latest last):");
                                    for line in tail.iter().rev() {
                                        eprintln!("  {}", line);
                                    }
                                }
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
    if args[1] == "bench" {
        let mut targets: Vec<String> = Vec::new();
        if args.len() > 2 {
            for a in &args[2..] {
                targets.push(a.clone());
            }
        } else {
            targets.push("examples/hello.clng".to_string());
            targets.push("examples/calculator.clng".to_string());
        }
        let mut files: Vec<String> = Vec::new();
        for target in targets {
            match std::fs::metadata(&target) {
                Ok(m) if m.is_dir() => {
                    let mut entries: Vec<String> = Vec::new();
                    collect_clng(&target, &mut entries);
                    entries.sort();
                    files.extend(entries);
                }
                Ok(_) => files.push(target),
                Err(_) => {
                    eprintln!("clang error at line 1: no such bench path '{}'", target);
                    std::process::exit(1);
                }
            }
        }
        println!("{:24} {:>10} {:>10} {:>8}", "file", "vm ms", "walk ms", "faster");
        for f in files {
            let src = match std::fs::read_to_string(&f) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}: cannot read: {}", f, e);
                    continue;
                }
            };
            if src.contains("http_serve") {
                println!("{:24} skipped (serves network)", f);
                continue;
            }
            let mut warm = Interp::new();
            if warm.run_source(&src).is_err() {
                println!("{:24} skipped (needs input)", f);
                continue;
            }
            let n = 20;
            let mut vm_ms = 0.0;
            let mut walk_ms = 0.0;
            for _ in 0..n {
                let mut ip = Interp::new();
                ip.quiet = true;
                let start = std::time::Instant::now();
                let _ = ip.run_source(&src);
                vm_ms += start.elapsed().as_secs_f64() * 1000.0;
            }
            std::env::set_var("CLNG_MODE", "walk");
            for _ in 0..n {
                let mut ip = Interp::new();
                ip.quiet = true;
                let start = std::time::Instant::now();
                let _ = ip.run_source(&src);
                walk_ms += start.elapsed().as_secs_f64() * 1000.0;
            }
            std::env::remove_var("CLNG_MODE");
            let vm_avg = vm_ms / n as f64;
            let walk_avg = walk_ms / n as f64;
            let faster = if vm_avg < walk_avg {
                format!("{:.1}x vm", walk_avg / vm_avg.max(0.001))
            } else {
                format!("{:.1}x walk", vm_avg / walk_avg.max(0.001))
            };
            println!("{:24} {:>10.2} {:>10.2} {:>8}", f, vm_avg, walk_avg, faster);
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
    if args[1] == "-e" || args[1] == "-c" {
        if args.len() < 3 {
            eprintln!("clang error at line 1: missing code after -e");
            eprintln!("help: run clng -e 'emit(1)'");
            std::process::exit(1);
        }
        let mut ip = Interp::new();
        if std::env::var("CLNG_TRACE").unwrap_or_default() == "1" {
            ip.trace = true;
        }
        if args.len() > 3 {
            ip.set_cli(args[3..].to_vec());
        }
        if let Err(e) = ip.run_source(&args[2]) {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        return;
    }
    if args[1] == "-" {
        use std::io::Read;
        let mut src = String::new();
        if std::io::stdin().read_to_string(&mut src).is_err() {
            eprintln!("clang error at line 1: could not read stdin");
            std::process::exit(1);
        }
        if src.trim().is_empty() {
            eprintln!("clang error at line 1: stdin held no code");
            eprintln!("help: pipe code in or use clng -e 'emit(1)'");
            std::process::exit(1);
        }
        let mut ip = Interp::new();
        if std::env::var("CLNG_TRACE").unwrap_or_default() == "1" {
            ip.trace = true;
        }
        if args.len() > 2 {
            ip.set_cli(args[2..].to_vec());
        }
        if let Err(e) = ip.run_source(&src) {
            eprintln!("{}", e);
            std::process::exit(1);
        }
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
    if std::env::var("CLNG_TRACE").unwrap_or_default() == "1" {
        ip.trace = true;
    }
    if args.len() > 2 {
        ip.set_cli(args[2..].to_vec());
    }
    if let Err(e) = ip.run_source(&src) {
        report(&path, &e);
        std::process::exit(1);
    }
}
