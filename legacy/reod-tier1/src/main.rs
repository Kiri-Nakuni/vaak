use std::io::Read;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("使い方: reod <file.reod>   （標準入力の整数列が read() に供給される）");
        std::process::exit(2);
    }

    let src = match std::fs::read_to_string(&args[1]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{} を読めません: {}", args[1], e);
            std::process::exit(2);
        }
    };

    let mut stdin = String::new();
    let _ = std::io::stdin().read_to_string(&mut stdin);
    let input: Vec<i64> = stdin
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();

    // 無名標準ライブラリを含めて実行する。`run_exit` が唯一の入口。
    // 正常終了の値の解釈はホストに委ねられる（3.6節）。ここでは exit code にする。
    match reod::run_exit(&src, input) {
        Ok((_, code)) => std::process::exit((code & 0xff) as i32),
        Err(e) => {
            eprintln!("実行時エラー: {}", e);
            std::process::exit(1);
        }
    }
}
