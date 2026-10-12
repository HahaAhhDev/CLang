mod compile;
mod error;
mod interp;
mod parser;
mod value;

use compile::{compile_source, load_maybe_compiled};
use interp::Interp;

fn print_usage() {
    println!("usage:");
    println!("  clng <file.clng|file.clngb>        run a file without keeping a build");
    println!("  clng compile <file.clng> [-o out]  build an obfuscated file plus launcher");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(2);
    }
    if args[1] == "compile" {
        if args.len() < 3 {
            eprintln!("clang error at line 1: missing input file");
            eprintln!("help: run clng compile app.clng");
            std::process::exit(1);
        }
        let src = args[2].clone();
        let mut out = format!("{}.clngb", src.trim_end_matches(".clng"));
        if let Some(i) = args.iter().position(|a| a == "-o") {
            if i + 1 < args.len() {
                out = args[i + 1].clone();
            }
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
    if let Err(e) = ip.run_source(&src) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
