use std::process::ExitCode;

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
    match vaak::lexer::lex(&src) {
        Ok(toks) => {
            println!("{} トークン", toks.len());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{path}:{}", e);
            ExitCode::FAILURE
        }
    }
}
