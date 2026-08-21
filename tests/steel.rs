//! STEEL の差分試験。
//!
//! **木を辿る実装が参照実装である**（S-5）。
//! STEEL が吐いた実行ファイルの終了コードが、参照実装の値と一致すること。
//!
//! `clang` が無ければ**飛ばす**——翻訳までは確かめる。

use std::process::Command;

fn interp(src: &str) -> Option<i128> {
    let prog = vaak::parser::parse(src).ok()?;
    let mut it = vaak::interp::Interp::new();
    match it.run(&prog) {
        Ok(vaak::interp::Eval::Value(v)) => v.as_int(),
        Ok(_) => Some(0),
        Err(_) => None,
    }
}

fn steel_run(name: &str, src: &str) -> Option<i32> {
    let prog = vaak::parser::parse(src).expect("構文");
    let errs: Vec<_> = vaak::check::check(&prog)
        .into_iter()
        .chain(vaak::types::check_types(&prog))
        .map(|e| e.msg)
        .collect();
    assert!(errs.is_empty(), "{name}: {errs:?}");
    let ir = match vaak::steel::compile(&prog) {
        Ok(ir) => ir,
        Err(e) => panic!("{name}: STEEL: {}", e.msg),
    };
    if Command::new("clang").arg("--version").output().is_err() {
        return None;
    }
    let dir = std::env::temp_dir().join(format!("steel-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ll = dir.join("a.ll");
    let exe = dir.join("a.out");
    std::fs::write(&ll, ir).unwrap();
    let st = Command::new("clang").arg("-O2").arg("-o").arg(&exe).arg(&ll).output().unwrap();
    assert!(st.status.success(), "{name}: clang: {}", String::from_utf8_lossy(&st.stderr));
    Some(Command::new(&exe).status().unwrap().code().unwrap())
}

/// **終了コードは 8 ビット。** 参照の値をそこへ落として比べる
fn agree(name: &str, src: &str) {
    let want = interp(src).expect("参照実装が答えを出せない");
    let Some(got) = steel_run(name, src) else { return };
    let expect = (want.rem_euclid(256)) as i32;
    assert_eq!(got, expect, "{name}: 参照 {want} → {expect}、STEEL {got}\n{src}");
}

macro_rules! t {
    ($n:ident, $src:expr) => {
        #[test]
        fn $n() {
            agree(stringify!($n), $src);
        }
    };
}

t!(算術, "1 + 2 * 3 - 4");
t!(優先順位, "2 + 3 * 4");
t!(括弧, "( 2 + 3 ) * 4");
t!(除算, "100 / 7");
t!(剰余は非負, "0 - 7 mod 3");
t!(剰余は非負2, "7 mod 3");
t!(零で割ると畳まれる, "( 1 / 0 ) ?? 42");
t!(比較, "if (3 < 5) 1 else 0 fi");
t!(否定はビット反転, "! 0");
t!(u1の否定は論理, "! (1 == 2)");
t!(束縛, "var x := 10; var y := 32; x + y");
t!(代入, "var x := 1; x += 41; x");
t!(掛け算代入, "var x := 6; x *= 7; x");
t!(条件, "var x := 5; if (x > 3) 1 elif (x > 1) 2 else 3 fi");
t!(else無しは畳まれる, "if (1 == 2) 1 fi ?? 9");
t!(ブロックは段, "{ 7 }");
t!(ブロックで脱出, "{ break 8; }");
t!(二段抜ける, "{ { break break 9; }; 0 }");
t!(ループの値は回数, "var n := 0; loop { n += 1; if (n == 5) break; fi; }");
t!(ループから値を持ち出す, "loop { break 11; }");
t!(nforの値は回数, "nfor (i, 0, 12) { };");
t!(nforで探す, "nfor (i, 0, 10) { if (i == 4) break i; fi; }");
t!(nforの合計, "var s := 0; nfor (i, 1, 10) { s += i; }; s");
t!(whileの値は回数, "var i := 0; while (i < 6) { i += 1; }");
t!(switch, "switch (2) case 1 => 10 case 2 => 20 case 3 => 30");
t!(switchは外れると畳まれる, "switch (9) case 1 => 10 ?? 77");
t!(coalesce左が値, "5 ?? 9");
t!(coalesce右が緩い, "( 1 / 0 ) ?? 20 + 3");
t!(関数, "fn add (a : i64, b : i64) { a + b } -> i64; add(20, 22)");
t!(再帰, "fn f (n : i64) { if (n < 2) n else f(n - 1) + f(n - 2) fi } -> i64; f(10)");
t!(関数から脱出, "fn f (n : i64) { nfor (i, 0, n) { if (i == 3) break break i; fi; }; 0 } -> i64; f(10)");
t!(型を指定した束縛, "var x : i32 := 300; x");
t!(狭い型で折り返す, "var x : u8 := 200; x + 100");
t!(continueで飛ばす, "var s := 0; nfor (i, 0, 10) { if (i == 3) continue; fi; s += 1; }; s");

// ===== 第二段：浮動小数と `|>` =====

t!(浮動小数の加算, "var x := 1.5; var y := 2.25; if (x + y == 3.75) 1 else 0 fi");
t!(浮動小数の除算, "var x := 7.0; var y := 2.0; if (x / y == 3.5) 1 else 0 fi");
t!(零で割ると畳まれる浮動小数, "var x := 1.0; var y := 0.0; if ((x / y) ?? 42.0 == 42.0) 1 else 0 fi");
t!(非有限は畳まれる, "var x := 1.0; var y := 0.0; if ((x / y) ?? 0.0 == 0.0) 7 else 8 fi");
t!(浮動小数の比較, "var x := 1.5; if (x < 2.0) 3 else 4 fi");
t!(浮動小数の符号反転, "var x := 1.5; if (0.0 - x < 0.0) 5 else 6 fi");
t!(f32を指定する, "var x : f32 := 1.5; if (x == 1.5 -> f32) 9 else 8 fi");
t!(パイプ, "fn double (n : i64) { n * 2 } -> i64; 21 |> double()");
t!(パイプで引数を足す, "fn add (a : i64, b : i64) { a + b } -> i64; 20 |> add(22)");
t!(パイプを繋ぐ, "fn inc (n : i64) { n + 1 } -> i64; fn dbl (n : i64) { n * 2 } -> i64; 20 |> inc() |> dbl()");
