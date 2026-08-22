//! Portable vaak — WASM で走らせる。
//!
//! **WASI 版**（`wasm32-wasip1`）は標準入力からソースを読み、
//! 最上位の外界面を終了コードにする——POSIX 版と同じ約束である（C-31）。
//!
//! ```bash
//! cargo build --release --target wasm32-wasip1 --bin portable
//! wasmtime target/wasm32-wasip1/release/portable.wasm < prog.vaak
//! ```
//!
//! **ホストが値を解釈する**（C-95 契約 4）ので、
//! 値なら下位 8 ビット、中身が空なら 0 とする。

use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut src = String::new();
    if std::io::stdin().read_to_string(&mut src).is_err() {
        eprintln!("標準入力が読めない");
        return ExitCode::from(2);
    }
    let prog = match vaak::parser::parse(&src) {
        Ok(p) => p,
        Err(e) => {
            let (l, c) = vaak::span::line_col(&src, e.span.start);
            eprintln!("{l}:{c}: 構文: {}", e.msg);
            return ExitCode::FAILURE;
        }
    };
    let mut errs = vaak::check::check(&prog);
    errs.extend(vaak::types::check_types(&prog));
    if !errs.is_empty() {
        for e in &errs {
            let (l, c) = vaak::span::line_col(&src, e.span.start);
            eprintln!("{l}:{c}: {}", e.msg);
        }
        return ExitCode::FAILURE;
    }
    match vaak::vm::run_program(&vaak::vm::compile(&prog).unwrap()) {
        Ok(vaak::interp::Eval::Value(v)) => {
            println!("{}", v.show());
            ExitCode::from(v.as_int().map(|n| n.rem_euclid(256) as u8).unwrap_or(0))
        }
        // **中身が空で終わればホストに委ねる**（C-31）。エラーではない
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            let (l, c) = vaak::span::line_col(&src, e.span.start);
            eprintln!("{l}:{c}: {}", e.msg);
            ExitCode::FAILURE
        }
    }
}
