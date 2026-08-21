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

// ===== 第二段：作用素式（`flow` / `$repeat`）=====

t!(二段抜けて値を置く, "loop { { break break 7; }; }");
t!(flowで名前を付ける, "flow $out = break break; loop { { $out 8; }; }");
t!(returnの定番,
   "flow $return = $repeat(break, getdepth());
    fn f (n : i64) { nfor (i, 0, 10) { if (i == n) $return i; fi; }; } -> i64; f(4) ?? 0 - 1");
t!(returnが見つからないとparadox,
   "flow $return = $repeat(break, getdepth());
    fn f (n : i64) { nfor (i, 0, 3) { if (i == n) $return i; fi; }; } -> i64; f(9) ?? 99");
t!(getdepthは使用位置で決まる,
   "flow $return = $repeat(break, getdepth());
    fn f () { { { $return 5; }; }; 0 } -> i64; f()");
t!(repeatの回数は式でよい, "loop { { { break $repeat(break, 1 + 1) 6; }; }; }");

// ===== 第三段：集合体 =====

t!(配列リテラル, "let xs := [ 2, 3, 5, 7 ]; xs[2]");
t!(配列の長さ, "let xs := [ 2, 3, 5, 7 ]; xs.len()");
t!(範囲外は畳まれる, "let xs := [ 1 ]; xs[9] ?? 42");
t!(負の添字も畳まれる, "let xs := [ 1 ]; xs[0 - 1] ?? 42");
t!(newで作る, "var a : i64 array := new i64 array(5, 7); a[3]");
t!(newの既定は零, "var a : i64 array := new i64 array(3, 0); a[1] + 1");
t!(要素に書く, "var a := [1,2,3]; a[1] := 9; a[1]");
t!(合計する, "var s := 0; let xs := [1,2,3,4]; nfor (i,0,xs.len()) { s += xs[i]; }; s");
t!(深く複製する, "var a := [1,2,3]; var b := a; b[0] := 9; a[0]");
t!(複製した方は変わる, "var a := [1,2,3]; var b := a; b[0] := 9; b[0]");
t!(aliasで受ければ写さない,
   "fn sum (a : i64 array alias) { var s := 0; nfor (i,0,a.len()) { s += a[i]; }; s } -> i64;
    let xs := [1,2,3,4]; sum(xs)");
t!(aliasで書き換えると元も変わる,
   "fn bump (var a : i64 array alias) { a[0] := 9; } -> i64;
    var xs := [1,2]; bump(xs); xs[0]");
t!(値で受ければ元は変わらない,
   "fn bump (var a : i64 array) { a[0] := 9; } -> i64;
    var xs := [1,2]; bump(xs); xs[0]");
t!(配列を返す, "fn mk () { [ 3, 1, 4 ] } -> i64 array; let a := mk(); a[0] + a[2]");
t!(文字列の長さ, "let s := \"abcd\"; s.len()");
t!(文字列の要素, "let s := \"abcd\"; s[1] -> i64");
t!(文字列も深く複製する, "var s := \"ab\"; var t := s; t[0] := 122; s[0] -> i64");
t!(u8の配列, "var a : u8 array := new u8 array(3, 200); (a[0] -> i64) + 1");
t!(真偽の配列, "var a : bool array := new bool array(3, true); if (a[1]) 7 else 8 fi");
t!(浮動小数の配列, "var a : f64 array := new f64 array(2, 1.5); if (a[0] + a[1] == 3.0) 5 else 6 fi");
t!(二分探索,
   "flow $return = $repeat(break, getdepth());
    fn find (a : i64 array alias, x : i64) {
        var lo := 0; var hi := a.len() - 1;
        while (lo <= hi) {
            let mid := (lo + hi) / 2;
            if (a[mid] ?? 0 == x) $return mid;
            elif (a[mid] ?? 0 < x) lo := mid + 1;
            else hi := mid - 1; fi;
        };
    } -> i64;
    let xs := [ 2, 3, 5, 7, 11, 13 ];
    (find(xs, 7) ?? 0 - 1) * 10 + (find(xs, 4) ?? 0 - 1)");

t!(場は関数の境で戻る,
   "fn mk (n : i64) { var a : i64 array := new i64 array(n, 0);
        nfor (i, 0, n) { a[i] := i; }; a } -> i64 array;
    fn total (a : i64 array alias) { var s := 0; nfor (i, 0, a.len()) { s += a[i]; }; s } -> i64;
    var acc := 0; nfor (k, 0, 2000) { let xs := mk(20); acc += total(xs); }; acc mod 251");

// ===== 第四段：`f80`（STEEL 方言の基底型、S-20）=====
//
// **木を辿る参照実装は `f80` を持たない。** Rust に対応する型が無いので、
// 差分試験ができない——代わりに **C の `long double` と突き合わせる。**

/// STEEL だけで走らせ、終了コードを返す。
fn steel_only(name: &str, src: &str) -> Option<i32> {
    let prog = vaak::parser::parse(src).expect("構文");
    let errs: Vec<_> = vaak::check::check(&prog)
        .into_iter()
        .chain(vaak::types::check_types(&prog))
        .map(|e| e.msg)
        .collect();
    assert!(errs.is_empty(), "{name}: {errs:?}");
    let ir = vaak::steel::compile(&prog).unwrap_or_else(|e| panic!("{name}: {}", e.msg));
    if Command::new("clang").arg("--version").output().is_err() {
        return None;
    }
    let dir = std::env::temp_dir().join(format!("f80-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ll = dir.join("a.ll");
    let exe = dir.join("a.out");
    std::fs::write(&ll, ir).unwrap();
    let st = Command::new("clang").arg("-O2").arg("-o").arg(&exe).arg(&ll).output().unwrap();
    assert!(st.status.success(), "{name}: clang: {}", String::from_utf8_lossy(&st.stderr));
    Some(Command::new(&exe).status().unwrap().code().unwrap())
}

#[test]
fn f80の算術() {
    let Some(c) = steel_only(
        "算術",
        "var x : f80 := 1.0; var y : f80 := 2.0; if (x + y == 3.0 -> f80) 7 else 8 fi",
    ) else {
        return;
    };
    assert_eq!(c, 7);
}

#[test]
fn f80から整数へ() {
    let Some(c) = steel_only("整数へ", "var x : f80 := 1.5; (x * 2.0 -> f80) -> i64") else {
        return;
    };
    assert_eq!(c, 3);
}

#[test]
fn f80の零で割ると畳まれる() {
    let Some(c) = steel_only(
        "零割",
        "var x : f80 := 1.0; var y : f80 := 0.0; if ((x / y) ?? 5.0 -> f80 == 5.0 -> f80) 4 else 6 fi",
    ) else {
        return;
    };
    assert_eq!(c, 4);
}

#[test]
fn f80は余分な精度を持つ() {
    // **これが `f80` の存在理由である。**
    // 2^-53 は `f64` では 1.0 に丸まるが、`f80`（仮数 64 ビット）では残る。
    // **C の `long double` と同じ答えになる**ことを確かめてある
    let Some(c) = steel_only(
        "精度",
        "var one : f80 := 1.0;
         var eps : f80 := 1.1102230246251565e-16;
         var sum : f80 := one + eps;
         var one64 := 1.0;
         var sum64 := one64 + 1.1102230246251565e-16;
         (if (sum == one) 0 else 1 fi) * 10 + (if (sum64 == one64) 0 else 1 fi)",
    ) else {
        return;
    };
    assert_eq!(c, 10, "f80 で残り、f64 で消えること");
}

#[test]
fn f80の配列() {
    let Some(c) = steel_only(
        "配列",
        "var a : f80 array := new f80 array(3, 1.5); if (a[0] + a[1] == 3.0 -> f80) 5 else 6 fi",
    ) else {
        return;
    };
    assert_eq!(c, 5);
}

#[test]
fn f80は参照実装が断る() {
    // **方言に無い型は黙って受けない**（S-20）。
    // `f80` と書いたのに `f64` で走ったら、書き手は精度を疑わない
    let prog = vaak::parser::parse("var x : f80 := 1.0; x").unwrap();
    let r = vaak::interp::Interp::new().run(&prog);
    match r {
        Err(e) => assert!(e.msg.contains("f80"), "{}", e.msg),
        Ok(v) => panic!("断っていない: {v:?}"),
    }
    let c = vaak::vm::compile(&prog);
    assert!(c.is_err(), "VM も断ること");
}
