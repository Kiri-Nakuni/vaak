//! STEEL vaak — Vaak を LLVM IR に翻訳し、実行ファイルにする。
//!
//! ```text
//! steel prog.vaak            → prog.ll と prog（clang があれば）
//! steel prog.vaak --emit-ir  → 標準出力に IR
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.iter().find(|a| !a.starts_with('-')) else {
        eprintln!("使い方: steel <ファイル> [--emit-ir] [-o 出力]");
        return ExitCode::from(2);
    };
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::from(2);
        }
    };
    let prog = match vaak::parser::parse(&src) {
        Ok(p) => p,
        Err(e) => {
            let (l, c) = vaak::span::line_col(&src, e.span.start);
            eprintln!("{path}:{l}:{c}: 構文: {}", e.msg);
            return ExitCode::FAILURE;
        }
    };
    let mut errs = vaak::check::check(&prog);
    errs.extend(vaak::types::check_types(&prog));
    if !errs.is_empty() {
        for e in &errs {
            let (l, c) = vaak::span::line_col(&src, e.span.start);
            eprintln!("{path}:{l}:{c}: {}", e.msg);
        }
        return ExitCode::FAILURE;
    }

    let ir = match vaak::steel::compile(&prog) {
        Ok(ir) => ir,
        Err(e) => {
            let (l, c) = vaak::span::line_col(&src, e.span.start);
            eprintln!("{path}:{l}:{c}: STEEL: {}", e.msg);
            return ExitCode::FAILURE;
        }
    };

    if args.iter().any(|a| a == "--emit-ir") {
        print!("{ir}");
        return ExitCode::SUCCESS;
    }

    let stem = path.trim_end_matches(".vaak");
    let ll = format!("{stem}.ll");
    if let Err(e) = std::fs::write(&ll, &ir) {
        eprintln!("{ll}: {e}");
        return ExitCode::FAILURE;
    }
    let out = args
        .windows(2)
        .find(|w| w[0] == "-o")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| stem.to_string());
    match std::process::Command::new("clang").args(["-O2", "-o", &out, &ll]).status() {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("clang が呼べない（IR は {ll} にある）: {e}");
            ExitCode::FAILURE
        }
    }
}
