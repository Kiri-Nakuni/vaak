use std::process::ExitCode;
use vaak::interp::{Interp, Eval};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("使い方: vaak <ファイル>");
        return ExitCode::from(2);
    };
    let src = match std::fs::read_to_string(&path) {
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
    let mut it = Interp::new();
    match it.run(&prog) {
        // 最上位の領域の**外界面は言語の意味論ではない**。ホストに委ねる
        Ok(Eval::Value(v)) => {
            println!("{}", v.show());
            ExitCode::SUCCESS
        }
        Ok(Eval::Paradox(_)) | Ok(Eval::Akasha) => ExitCode::SUCCESS,
        Ok(Eval::Escape(_)) => {
            eprintln!("{path}: フレームを越える脱出");
            ExitCode::FAILURE
        }
        Err(e) => {
            let (l, c) = vaak::span::line_col(&src, e.span.start);
            eprintln!("{path}:{l}:{c}: {}", e.msg);
            ExitCode::FAILURE
        }
    }
}
