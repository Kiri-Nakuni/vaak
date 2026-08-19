pub mod interp;
pub mod lexer;
pub mod state;

/// 無名標準ライブラリ（D-37）。すべてのプログラムの前に読み込まれる。
/// 中核ではない——ここにあるものは REod で書かれている。
pub const PRELUDE: &str = include_str!("prelude.reod");

/// ソースを実行し、出力と、ホストへ渡す正常終了の値を返す（3.6節）。
pub fn run_exit(src: &str, input: Vec<i64>) -> Result<(Vec<String>, i64), String> {
    let prelude_lines = PRELUDE.lines().count() as u32;
    let combined = format!("{}\n{}", PRELUDE, src);
    let lx = lexer::lex(&combined)?;
    let mut it = interp::Interp::new(lx, input);
    it.set_line_offset(prelude_lines + 1);
    let code = it.run()?;
    Ok((std::mem::take(&mut it.out), code))
}

/// 出力だけが要るとき。
pub fn run(src: &str, input: Vec<i64>) -> Result<Vec<String>, String> {
    run_exit(src, input).map(|(out, _)| out)
}
