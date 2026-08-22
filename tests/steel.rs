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
    // **`-lm` が要る。** `exp` `log` `pow` `sin` などは libm の関数へ落ちる
    let st = Command::new("clang")
        .arg("-O2")
        .arg("-o")
        .arg(&exe)
        .arg(&ll)
        .arg("-lm")
        .output()
        .unwrap();
    assert!(st.status.success(), "{name}: clang: {}", String::from_utf8_lossy(&st.stderr));
    Some(Command::new(&exe).status().unwrap().code().unwrap())
}

/// Unix の終了コードは 8 ビットだが、Windows は `i32` のまま返す。
/// **どちらでも観測できる下位 8 ビット**へ揃えて比べる。
fn agree(name: &str, src: &str) {
    let want = interp(src).expect("参照実装が答えを出せない");
    let Some(got) = steel_run(name, src) else { return };
    let expect = (want.rem_euclid(256)) as i32;
    let got8 = got.rem_euclid(256);
    assert_eq!(got8, expect, "{name}: 参照 {want} → {expect}、STEEL {got} → {got8}\n{src}");
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
t!(狭い型への代入,
   "var x : u8 := 0; x := 300; x");
t!(狭い型の欄への代入,
   "struct P { var x : u8 := 0; }; var p := new P (); p.x := 300; p.x");
t!(狭い型の値引数,
   "fn f (x : u8) { x } -> u8; f(300)");
t!(狭い型の返り値,
   "fn f () { 300 } -> u8; f()");
t!(continueで飛ばす, "var s := 0; nfor (i, 0, 10) { if (i == 3) continue; fi; s += 1; }; s");

// ===== 第二段：浮動小数と `|>` =====

t!(浮動小数の加算, "var x := 1.5; var y := 2.25; if (x + y == 3.75) 1 else 0 fi");
t!(浮動小数の除算, "var x := 7.0; var y := 2.0; if (x / y == 3.5) 1 else 0 fi");
t!(零で割ると畳まれる浮動小数, "var x := 1.0; var y := 0.0; if ((x / y) ?? 42.0 == 42.0) 1 else 0 fi");
t!(非有限は畳まれる, "var x := 1.0; var y := 0.0; if ((x / y) ?? 0.0 == 0.0) 7 else 8 fi");
t!(浮動小数の比較, "var x := 1.5; if (x < 2.0) 3 else 4 fi");
t!(浮動小数の符号反転, "var x := 1.5; if (0.0 - x < 0.0) 5 else 6 fi");
t!(浮動小数はブロックを出ても小数部を保つ,
   "if ({ 1.5 } == 1.5) 1 else 0 fi");
t!(浮動小数は分岐の合流でも小数部を保つ,
   "if ((if (true) 1.5 else 2.5 fi) == 1.5) 1 else 0 fi");
t!(浮動小数はcoalesceの合流でも小数部を保つ,
   "if ((1.5 ?? 2.5) == 1.5) 1 else 0 fi");
t!(浮動小数はswitchの合流でも小数部を保つ,
   "if ((switch (1) case 1 => 1.5) == 1.5) 1 else 0 fi");
t!(浮動小数の返り値は小数部を保つ,
   "fn f () { 1.5 } -> f64; if (f() == 1.5) 1 else 0 fi");
t!(f32を指定する, "var x : f32 := 1.5; if (x == 1.5 -> f32) 9 else 8 fi");
t!(f32演算で溢れると畳まれる,
   "var x : f32 := 3.0e38;
    if (((x + x) ?? (7.0 -> f32)) == (7.0 -> f32)) 1 else 0 fi");
t!(f32へ狭めて溢れると畳まれる,
   "var x : f64 := 3.5e38;
    if (((x -> f32) ?? (7.0 -> f32)) == (7.0 -> f32)) 1 else 0 fi");
t!(f32の返り値へ狭めて溢れると畳まれる,
   "fn f () { 3.5e38 } -> f32;
    if (((f()) ?? (7.0 -> f32)) == (7.0 -> f32)) 1 else 0 fi");
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
t!(alias引数の数値代入が呼び出し元へ届く,
   "fn set (var x : i64 alias) { x := 42; };
    var x := 0; set(x); x");
t!(alias引数から伸ばした配列の根が共有される,
   "fn push_one (var xs : i64 array alias) { xs.push(42); };
    var xs : i64 array := new i64 array(0, 0); push_one(xs);
    if (xs.len() == 1) (xs[0] ?? 0) else 0 fi");
t!(alias引数へ逃げた確保は関数解放を越えて生きる,
   "fn push_one (var xs : i64 array alias) { xs.push(42); };
    var xs : i64 array := new i64 array(0, 0); push_one(xs);
    let trash := new i64 array(4, 7);
    (xs[0] ?? 0) + trash[0] - 7");
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
    let st = Command::new("clang")
        .arg("-O2")
        .arg("-o")
        .arg(&exe)
        .arg(&ll)
        .arg("-lm")
        .output()
        .unwrap();
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
fn f80はブロックを出ても小数部を保つ() {
    let Some(c) = steel_only(
        "ブロック",
        "if ({ 1.5 -> f80 } == 1.5 -> f80) 7 else 8 fi",
    ) else {
        return;
    };
    assert_eq!(c, 7);
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

// ===== 第四段：入れ子の集合体 =====

t!(入れ子の添字, "let m := [ [1,2], [3,4] ]; m[1][0]");
t!(入れ子も深く複製する,
   "var m := [ [1,2], [3,4] ]; var n := m; n[0][0] := 9; m[0][0] * 10 + n[0][0]");
t!(入れ子の長さ, "let m := [ [1,2,3] ]; m[0].len() * 10 + m.len()");
t!(入れ子をnewで作る,
   "var g : i64 array array := new i64 array array(2, [0,0]); g[0][1] := 7; g[1][1] * 10 + g[0][1]");
t!(入れ子を返す,
   "fn mk () { [ [1,2], [3,4] ] } -> i64 array array; let a := mk(); a[1][0] * 10 + a[0][1]");
t!(入れ子を返して場を使う,
   "fn mk () { [ [ 7, 8 ], [ 9, 10 ] ] } -> i64 array array;
    fn churn (n : i64) { var a : i64 array := new i64 array(n, 5); a[0] } -> i64;
    let g := mk(); var z := 0; nfor (k, 0, 50) { z += churn(200); };
    g[0][0] * 10 + g[1][1]");
t!(入れ子を何度も作る,
   "fn mk (n : i64) { var g : i64 array array := new i64 array array(n, [0,0,0]);
        nfor (i,0,n) { g[i][0] := i; }; g } -> i64 array array;
    var s := 0; nfor (k,0,500) { let g := mk(4); s += g[3][0]; }; s mod 251");
t!(三重の入れ子,
   "let t := [ [ [1,2] ], [ [3,4] ] ]; t[1][0][1]");
t!(埋める値は一つずつ写す,
   "var g : i64 array array := new i64 array array(3, [0]); g[0][0] := 5; g[1][0] * 10 + g[0][0]");
t!(文字列の配列, "let xs := [ \"ab\", \"cde\" ]; xs[1].len() * 10 + xs[0].len()");

// ===== 第四段：構造体と包み型 =====

t!(構造体を作って読む,
   "struct P { let x : i64; let y : i64; };
    let p := new P ( x := 3, y := 4 ); p.x * 10 + p.y");
t!(欄の既定値,
   "struct P { let x : i64; var y : i64 := 7; };
    let p := new P ( x := 1 ); p.y");
t!(欄に書く,
   "struct P { let x : i64; var y : i64 := 0; };
    var p := new P ( x := 1 ); p.y := 9; p.y");
t!(構造体も深く複製する,
   "struct P { let x : i64; var y : i64 := 0; };
    var a := new P ( x := 1 ); var b := a; b.y := 9; a.y * 10 + b.y");
t!(構造体をaliasで受ける,
   "struct P { let x : i64; let y : i64; };
    fn d2 (p : P alias) { p.x * p.x + p.y * p.y } -> i64;
    let p := new P ( x := 3, y := 4 ); d2(p)");
t!(構造体を返す,
   "struct P { let x : i64; let y : i64; };
    fn mk (a : i64) { new P ( x := a, y := a + 1 ) } -> P;
    let p := mk(4); p.x * 10 + p.y");
t!(構造体の除去子は脱出側で型を失わない,
   "flow $return = $repeat(break, getdepth());
    struct P { let x : i64 := 42; };
    fn maybe (yes : u1) { if (yes) new P ( ) fi } -> P;
    fn use (yes : u1) {
        let p := maybe(yes) ?? $return;
        p.x
    } -> i64;
    use(true) ?? 0");
t!(構造体が集合体を持つ,
   "struct Bag { var xs : i64 array; };
    var a := new Bag ( xs := [1,2,3] );
    var b := a; b.xs[0] := 9;
    a.xs[0] * 10 + b.xs[0]");
t!(構造体の配列,
   "struct P { let x : i64; };
    let ps := [ new P ( x := 1 ), new P ( x := 2 ) ]; ps[1].x");
t!(包み型は基底型のまま,
   "wrap Meters = i64; let m := new Meters ( 5 ); new i64 ( m ) * 2");
t!(包み型の集合体,
   "wrap Bytes = u8 array; var b : Bytes := new Bytes ( new u8 array(3, 97) ); 3");
t!(strをu8配列へ剥がす,
   "let text := \"ABC\"; var bytes := new u8 array(text);
    bytes[0] := 90; (text[0] -> i64) + (bytes[0] -> i64)");
t!(u8配列をstrへ包む,
   "var bytes : u8 array := [65, 66, 67]; var text := new str(bytes);
    text[0] := 90; (bytes[0] -> i64) + (text[0] -> i64)");
t!(strの包みを往復する,
   "var bytes := new u8 array(3, 97); var text := new str(bytes);
    var roundtrip := new u8 array(text); roundtrip[1] -> i64");
t!(u8配列の一引数構築は長さを失わず零で埋める,
   "var bytes := new u8 array(3);
    bytes.len() * 1000
      + (bytes[0] -> i64) * 100
      + (bytes[1] -> i64) * 10
      + (bytes[2] -> i64)");

// ── 複合代入は場所を一度だけ数える ─────────────────────────

t!(欄への複合代入,
   "struct P { var x : i64 := 10; var y : i64 := 3; };
    var p := new P (); p.x += 5; p.y *= 4; p.x + p.y");
t!(枡への複合代入,
   "var a := new i64 array(3, 7); a[0] += 1; a[1] -= 2; a[2] *= 3; a[0] + a[1] + a[2]");
t!(欄への割り算の複合代入,
   "struct P { var x : i64 := 100; }; var p := new P (); p.x /= 7; p.x");
t!(入れ子の欄へ足す,
   "struct P { var a : i64 array; };
    var p := new P ( a := new i64 array(2, 5) ); p.a[0] += 6; p.a[0] + p.a[1]");
t!(添字は一度だけ数える,
   "var n := 0; var a := new i64 array(2, 0); a[n] += 9; a[0] + a[1] + n");
t!(浮動小数の複合代入,
   "struct P { var x : f64 := 1.5; }; var p := new P (); p.x += 2.25;
    if (p.x == 3.75) 1 else 0 fi");
t!(注釈は集合体にも付く, "var a := new i64 array(2, 4); (a -> i64 array)[1]");

// ── 別名は枠を指す。値は動かない ─────────────────────────

t!(別名から読む, "var x := 7; var y : i64 alias &= x; y");
t!(別名へ書くと元も変わる, "var x := 7; var y : i64 alias &= x; y := 9; x");
t!(元へ書くと別名も変わる, "var x := 7; var y : i64 alias &= x; x := 4; y");
t!(別名を指し直す,
   "var a := 1; var b := 2; var r : i64 alias &= a; r &= b; r := 8; a * 10 + b");
t!(別名の別名, "var x := 5; var y : i64 alias &= x; var z : i64 alias &= y; z := 3; x");
t!(別名へ複合代入, "var x := 10; var y : i64 alias &= x; y += 5; x");
t!(集合体の別名, "var a := new i64 array(2, 3); var b : i64 array alias &= a; b[0] := 9; a[0] + a[1]");
t!(別名は枠を新しく作らない,
   "var x := 1; { var y : i64 alias &= x; y := 6; }; x");

// ── 押す・引く・空にする ─────────────────────────

t!(押して伸ばす, "var a := new i64 array(0, 0); a.push(3); a.push(4); a.push(5); a.len()");
t!(押した値が読める, "var a := new i64 array(0, 0); a.push(7); a.push(9); a[0] * 10 + a[1]");
t!(何度も押す,
   "var a := new i64 array(0, 0); var i := 0; while (i < 40) { a.push(i); i += 1; }; a[39] + a.len()");
t!(引くと減る, "var a := new i64 array(3, 5); a.pop(); a.len()");
t!(引いた値が出る, "var a := new i64 array(0, 0); a.push(6); a.push(8); a.pop()");
t!(空から引くと虚無, "var a := new i64 array(0, 0); a.pop() ?? 42");
t!(空にする, "var a := new i64 array(9, 1); a.clear(); a.len()");
t!(空にしてから押す, "var a := new i64 array(9, 1); a.clear(); a.push(3); a.len() * 10 + a[0]");
t!(押しても他は変わらない,
   "var a := new i64 array(2, 1); var b := a; b.push(9); a.len() * 10 + b.len()");
t!(欄の集合体を押す,
   "struct P { var a : i64 array; }; var p := new P ( a := new i64 array(0, 0) );
    p.a.push(4); p.a.push(6); p.a[1] + p.a.len()");
t!(浮動小数を押す,
   "var a := new f64 array(0, 0.0); a.push(1.5); a.push(2.5);
    if (a[0] + a[1] == 4.0) 1 else 0 fi");

// ── 挿す・抜く ─────────────────────────

t!(先頭へ挿す, "var a := new i64 array(2, 5); a.insert(0, 9); a[0] * 100 + a[1] * 10 + a[2]");
t!(途中へ挿す, "var a := new i64 array(0, 0); a.push(1); a.push(3); a.insert(1, 2); a[0]*100+a[1]*10+a[2]");
t!(末尾へ挿す, "var a := new i64 array(0, 0); a.push(1); a.insert(1, 7); a[1] * 10 + a.len()");
t!(枠の外へ挿すと何も起きない, "var a := new i64 array(2, 4); a.insert(5, 9); a.len() * 10 + a[0]");
t!(負の添字へ挿すと何も起きない, "var a := new i64 array(2, 4); a.insert(0 - 1, 9); a.len()");
t!(抜くと詰まる, "var a := new i64 array(0, 0); a.push(1); a.push(2); a.push(3); a.remove(1); a[0]*10+a[1]");
t!(抜いた値が出る, "var a := new i64 array(0, 0); a.push(7); a.push(8); a.remove(0)");
t!(抜くと減る, "var a := new i64 array(4, 1); a.remove(2); a.len()");
t!(枠の外を抜くと虚無, "var a := new i64 array(2, 1); a.remove(9) ?? 42");
t!(枠の外を抜いても減らない, "var a := new i64 array(2, 1); a.remove(9) ?? 0; a.len()");
t!(末尾を抜く, "var a := new i64 array(0, 0); a.push(4); a.push(6); a.remove(1) * 10 + a.len()");
t!(挿してから抜く, "var a := new i64 array(0, 0); a.push(1); a.insert(0, 9); a.remove(1) + a[0]");
t!(浮動小数を挿す,
   "var a := new f64 array(0, 0.0); a.push(2.0); a.insert(0, 1.5);
    if (a[0] + a[1] == 3.5) 1 else 0 fi");
t!(欄の集合体へ挿す,
   "struct P { var a : i64 array; }; var p := new P ( a := new i64 array(1, 5) );
    p.a.insert(0, 3); p.a[0] * 10 + p.a[1]");

// ── 写像 ─────────────────────────

t!(空の写像, "var m := new i64 i64 map ( ); m.len()");
t!(写像リテラル, "var m : i64 i64 map := ( 1 => 10, 2 => 20 ); m.len()");
t!(鍵で引く, "var m : i64 i64 map := ( 1 => 10, 2 => 20 ); m[2]");
t!(無い鍵は虚無, "var m : i64 i64 map := ( 1 => 10 ); m[9] ?? 42");
t!(鍵を足す, "var m := new i64 i64 map ( ); m[5] := 7; m[5] * 10 + m.len()");
t!(同じ鍵は上書き, "var m := new i64 i64 map ( ); m[5] := 7; m[5] := 9; m[5] * 10 + m.len()");
t!(順に並ぶ, "var m := new i64 i64 map ( ); m[3] := 1; m[1] := 2; m[2] := 3; m.keys()[0] * 100 + m.keys()[1] * 10 + m.keys()[2]");
t!(たくさん入れる,
   "var m := new i64 i64 map ( ); var i := 0;
    while (i < 30) { m[29 - i] := i; i += 1; }; m.len() * 100 + m[14]");
t!(順に並べ直す,
   "var m := new i64 i64 map ( ); var i := 0;
    while (i < 8) { m[7 - i] := i; i += 1; };
    m.keys()[0] * 10 + m.keys()[7]");
t!(鍵があるか, "var m : i64 i64 map := ( 1 => 10, 5 => 50 ); if (m.has(5)) 1 else 0 fi");
t!(無い鍵は無い, "var m : i64 i64 map := ( 1 => 10 ); if (m.has(9)) 1 else 0 fi");
t!(写像から抜く, "var m : i64 i64 map := ( 1 => 10, 2 => 20 ); m.remove(1) + m.len()");
t!(無い鍵を抜くと虚無, "var m : i64 i64 map := ( 1 => 10 ); (m.remove(9) ?? 5) + m.len()");
t!(抜いた後は引けない, "var m : i64 i64 map := ( 1 => 10, 2 => 20 ); m.remove(2) ?? 0; m[2] ?? 7");
t!(写像も深く複製する,
   "var a : i64 i64 map := ( 1 => 10 ); var b := a; b[1] := 99; a[1] * 100 + b[1]");
t!(写像を空にする, "var m : i64 i64 map := ( 1 => 1, 2 => 2 ); m.clear(); m.len()");
t!(文字列の鍵,
   "var m : str i64 map := ( \"bb\" => 2, \"a\" => 1 ); m[\"bb\"] * 10 + m.len()");
t!(文字列の鍵は短い順, "var m : str i64 map := ( \"bb\" => 2, \"a\" => 1 ); m.keys()[0].len()");
t!(写像の値が集合体,
   "var m := new i64 i64 array map ( ); m[3] := new i64 array(2, 8); m[3][1] ?? 0");

// ── hash：鍵の値で飛ぶ（C-98） ─────────────────────────

t!(空のhash, "var h := new i64 i64 hash ( ); h.len()");
t!(hashリテラル, "var h : i64 i64 hash := ( 1 => 10, 2 => 20 ); h.len()");
t!(hashを引く, "var h : i64 i64 hash := ( 1 => 10, 2 => 20 ); h[2]");
t!(hashの無い鍵は虚無, "var h : i64 i64 hash := ( 1 => 10 ); h[9] ?? 42");
t!(hashへ足す, "var h := new i64 i64 hash ( ); h[5] := 7; h[5] * 10 + h.len()");
t!(hashは上書きする, "var h := new i64 i64 hash ( ); h[5] := 7; h[5] := 9; h[5] * 10 + h.len()");
t!(hashは入れた順,
   "var h := new i64 i64 hash ( ); h[9] := 1; h[3] := 2; h[7] := 3;
    var k := h.keys(); k[0] * 100 + k[1] * 10 + k[2]");
t!(hashで在るか, "var h : i64 i64 hash := ( 1 => 1, 5 => 5 ); if (h.has(5)) 1 else 0 fi");
t!(hashで無いものは無い, "var h : i64 i64 hash := ( 1 => 1 ); if (h.has(9)) 1 else 0 fi");
t!(hashから抜く, "var h : i64 i64 hash := ( 1 => 10, 2 => 20 ); h.remove(1) + h.len()");
t!(hashの無い鍵を抜くと虚無, "var h : i64 i64 hash := ( 1 => 10 ); (h.remove(9) ?? 5) + h.len()");
t!(hashは抜いた後引けない, "var h : i64 i64 hash := ( 1 => 1, 2 => 2 ); h.remove(2) ?? 0; h[2] ?? 7");
t!(hashを空にする, "var h : i64 i64 hash := ( 1 => 1, 2 => 2 ); h.clear(); h.len()");
t!(hashも深く複製する,
   "var a : i64 i64 hash := ( 1 => 10 ); var b := a; b[1] := 99; a[1] * 100 + b[1]");
t!(hashにたくさん入れる,
   "var h := new i64 i64 hash ( ); var i := 0;
    while (i < 200) { h[i * 37] := i; i += 1; }; h.len() + h[37 * 99]");
t!(hashは抜いても引ける,
   "var h := new i64 i64 hash ( ); var i := 0;
    while (i < 50) { h[i] := i; i += 1; };
    var j := 0;
    while (j < 25) { h.remove(j * 2) ?? 0; j += 1; };
    h.len() * 100 + (h[7] ?? 0)");
t!(文字列を鍵にするhash,
   "var h : str i64 hash := ( \"alpha\" => 1, \"beta\" => 2 ); h[\"beta\"] * 10 + h.len()");
t!(hashの値が集合体,
   "var h := new i64 i64 array hash ( ); h[3] := new i64 array(2, 8); h[3][1] ?? 0");

// ── 浮動小数の鍵（C-99） ─────────────────────────

t!(浮動小数の鍵は昇順,
   "var m : f64 i64 map := ( 1.0 => 1, 0.0 - 2.0 => 2, 3.0 => 3 );
    var k := m.keys(); if (k[0] < k[1]) 1 else 0 fi");
t!(浮動小数の鍵は負が先,
   "var m : f64 i64 map := ( 1.0 => 1, 0.0 - 2.0 => 2 );
    var k := m.keys(); if (k[0] == 0.0 - 2.0) 1 else 0 fi");
t!(負の零は零と同じ鍵,
   "var m := new f64 i64 map ( ); m[0.0] := 1; m[(0.0 - 1.0) * 0.0] := 2;
    m.len() * 10 + m[0.0]");
t!(hashでも負の零は零,
   "var h := new f64 i64 hash ( ); h[0.0] := 1; h[(0.0 - 1.0) * 0.0] := 2;
    h.len() * 10 + h[0.0]");
t!(浮動小数の鍵を引く, "var m : f64 i64 map := ( 1.5 => 7, 2.5 => 9 ); m[2.5]");
t!(hashで浮動小数の鍵を引く, "var h : f64 i64 hash := ( 1.5 => 7, 2.5 => 9 ); h[2.5]");
t!(浮動小数の鍵で無いもの, "var m : f64 i64 map := ( 1.5 => 7 ); m[9.5] ?? 42");
t!(hashで浮動小数の無い鍵, "var h : f64 i64 hash := ( 1.5 => 7 ); h[9.5] ?? 42");
t!(f32の鍵, "var m : f32 i64 map := ( 1.5 => 7, 0.0 - 2.5 => 9 ); var k := m.keys(); m[k[0]]");
t!(hashでf32の鍵, "var h : f32 i64 hash := ( 1.5 => 7, 0.0 - 2.5 => 9 ); h[1.5]");
t!(浮動小数の鍵を抜く, "var m : f64 i64 map := ( 1.5 => 7, 2.5 => 9 ); m.remove(1.5) + m.len()");
t!(hashで浮動小数の鍵を抜く, "var h : f64 i64 hash := ( 1.5 => 7, 2.5 => 9 ); h.remove(1.5) + h.len()");

// ── 文脈の型は演算の中まで届く（C-100） ─────────────────────────

t!(u8の場では折り返してから割る, "var x : u8 := 100 * 3 / 2; x");
t!(i64の場では折り返さない, "var x : i64 := 100 * 3 / 2; x");
t!(枠を越えるリテラル, "var x : u8 := 256 / 2; x");
t!(代入でも届く, "var x : u8 := 0; x := 100 * 3 / 2; x");
t!(領域を通しても届く, "var x : u8 := { 100 * 3 / 2 }; x");
t!(ifの枝へ届く, "var x : u8 := if (1 == 1) 100 * 3 / 2 else 0 fi; x");
t!(注釈から届く, "(100 * 3 / 2) -> u8");
t!(引数へ届く, "fn f (a : u8) { a } -> u8; f(100 * 3 / 2)");
t!(返り値から届く, "fn f () { 100 * 3 / 2 } -> u8; f()");
t!(欄へ届く, "struct P { var x : u8 := 0; }; var p := new P ( x := 100 * 3 / 2 ); p.x");
t!(欄の既定へも届く, "struct P { var x : u8 := 100 * 3 / 2; }; var p := new P (); p.x");
t!(配列の要素へ届く, "var a : u8 array := [ 100 * 3 / 2 ]; a[0]");
t!(名前で書いても同じ,
   "var a : u8 := 100; var b : u8 := 3; var c : u8 := 2; var x : u8 := 0; x := a * b / c; x");
t!(i32でも折り返す, "var x : i32 := 100000 * 100000 / 1000; x");
t!(桁数は別の型でよい, "var x : u8 := 1 << 9; x");
t!(幅以上の桁送りはこぼれる, "var x : u8 := 1 << 9; x");
t!(右へ幅以上送る, "var x : u8 := 255 >> 9; x");
t!(符号つきを右へ幅以上送ると符号で埋まる, "var x : i32 := 0 - 1; x := x >> 40; x");
t!(i64を幅以上送る, "var x : i64 := 1 << 64; x");

// ── 数のメンバ関数（S-23） ─────────────────────────

t!(絶対値, "var x : i64 := 0 - 5; x.abs()");
t!(絶対値は折り返す, "var x : i32 := 0 - 2147483648; x.abs()");
t!(最小と最大, "var a : i64 := 5; a.min(3) * 10 + a.max(3)");
t!(符号なしの最小, "var a : u8 := 5; a.min(200)");
t!(立っているビットを数える, "var x : u8 := 0b1011; x.count_ones()");
t!(前の零を数える, "var x : u8 := 0b00010000; x.leading_zeros()");
t!(零の前の零, "var x : u8 := 0; x.leading_zeros()");
t!(後ろの零を数える, "var x : u8 := 0b00010000; x.trailing_zeros()");
t!(零の後ろの零, "var x : u8 := 0; x.trailing_zeros()");
t!(ビットを逆にする, "var x : u8 := 0b11010000; x.reverse_bits()");
t!(バイトを入れ替える, "var x : u16 := 0x1234; x.swap_bytes()");
t!(一バイトは入れ替えても同じ, "var x : u8 := 0x12; x.swap_bytes()");
t!(左へ回す, "var x : u8 := 1; x.rotate_left(1)");
t!(幅だけ回すと戻る, "var x : u8 := 0b10110001; x.rotate_left(8)");
t!(幅を越えて回す, "var x : u8 := 1; x.rotate_left(9)");
t!(右へ回す, "var x : u8 := 1; x.rotate_right(1)");
t!(端をまたいで回す, "var x : u8 := 0b10000000; x.rotate_left(1)");
t!(足して止まる, "var x : u8 := 250; x.saturating_add(10)");
t!(引いて止まる, "var x : u8 := 5; x.saturating_sub(10)");
t!(掛けて止まる, "var x : i32 := 100000; x.saturating_mul(100000)");
t!(負へ掛けて止まる, "var x : i32 := 0 - 100000; x.saturating_mul(100000)");
t!(止まらない掛け算, "var x : u8 := 20; x.saturating_mul(20)");
t!(平方根, "var x : f64 := 4.0; if (x.sqrt() == 2.0) 1 else 0 fi");
t!(負の平方根は虚無, "var x : f64 := 0.0 - 1.0; if ((x.sqrt() ?? 7.0) == 7.0) 1 else 0 fi");
t!(浮動小数の絶対値, "var x : f64 := 0.0 - 2.5; if (x.abs() == 2.5) 1 else 0 fi");
t!(床と天井,
   "var x : f64 := 2.5; if (x.floor() == 2.0) 1 else 0 fi");
t!(切り捨て, "var x : f64 := 0.0 - 2.5; if (x.trunc() == 0.0 - 2.0) 1 else 0 fi");
t!(四捨五入, "var x : f64 := 2.5; if (x.round() == 3.0) 1 else 0 fi");
t!(符号を写す, "var x : f64 := 3.0; if (x.copysign(0.0 - 1.0) == 0.0 - 3.0) 1 else 0 fi");
t!(掛けて足す, "var x : f64 := 2.0; if (x.mul_add(3.0, 1.0) == 7.0) 1 else 0 fi");
t!(冪, "var x : f64 := 2.0; if (x.pow(10.0) == 1024.0) 1 else 0 fi");
t!(零の対数は虚無, "var x : f64 := 0.0; if ((x.ln() ?? 5.0) == 5.0) 1 else 0 fi");
t!(二の対数, "var x : f64 := 8.0; if (x.log2() == 3.0) 1 else 0 fi");
t!(f32の平方根, "var x : f32 := 4.0; if (x.sqrt() == 2.0) 1 else 0 fi");

// ── 十進以外の表記 ─────────────────────────

t!(二進, "0b1011");
t!(十六進, "0xff");
t!(八進, "0o777");
t!(区切りつき, "1_000");
t!(区切りつき十六進, "0xFF_FF");

// ── 包み型は関数の返りでも剥がれる ─────────────────────────

t!(包み型を返す関数,
   "wrap NodeId = i64; fn mk (n : i64) { new NodeId(n) } -> NodeId;
    (mk(7) -> i64) * 10 + 1");
t!(包み型を返して包み型で受ける,
   "wrap NodeId = i64; fn mk (n : i64) { new NodeId(n) } -> NodeId;
    fn use2 (h : NodeId) { (h -> i64) * 2 } -> i64;
    use2(mk(21))");
t!(包み型の配列を返す,
   "wrap Bytes = u8 array; fn mk () { new Bytes(new u8 array(2, 5)) } -> Bytes;
    var b := mk(); (b -> u8 array)[1] -> i64");

// ── 動く段数の `$repeat` ─────────────────────────

t!(動く段数で零段, "var n := 0; {{ $repeat(break, n) 5 }}");
t!(動く段数で負, "var n := 0 - 3; {{ $repeat(break, n) 5 }}");
t!(動く段数で一段, "var n := 1; {{ $repeat(break, n) 5 }}");
t!(動く段数で二段, "var n := 2; {{ { $repeat(break, n) 7 } ; 3 }}");
t!(動く段数で一段だけ抜ける, "var n := 1; {{ { $repeat(break, n) 7 } ; 3 }}");
t!(動く段数で三段,
   "var n := 3; {{ { { $repeat(break, n) 9 } ; 2 } ; 3 }}");
t!(動く段数の作用素が二段,
   "var n := 2; {{ { $repeat(break break, n) 7 } ; 3 }}");
t!(動く段数で再開,
   "var s := 0; var n := 1; loop { s += 1; if (s > 3) break fi; $repeat(continue, n) }; s");
t!(動く段数は式から来てもよい,
   "var a := 1; var b := 1; {{ { $repeat(break, a + b) 7 } ; 3 }}");
t!(動く段数の被演算子は一度だけ走る,
   "var c := 0; var n := 1; fn bump (var k : i64 alias) { k += 1; 5 } -> i64;
    {{ $repeat(break, n) bump(c) }} + c");

// ── 被演算子つきの `continue`（C-73） ─────────────────────────

t!(遅延した脱出は次の周回で走る, "nfor (i, 0, 10) { continue break i; }");
t!(条件つきで遅延する, "nfor (i, 0, 10) { if (i < 3) continue break i fi; }");
t!(遅延した再開, "nfor (i, 0, 10) { continue continue; }");
t!(ループでも遅延する,
   "var s := 0; loop { s += 1; if (s > 2) break s fi; continue break s }");
t!(whileでも遅延する,
   "var s := 0; var i := 0; while (i < 9) { i += 1; if (i > 2) break i fi; continue break i }");
t!(遅延しない周回は普通に回る,
   "var s := 0; nfor (i, 0, 5) { if (i == 2) continue break i fi; s += 1; }");
t!(裸のブロックを抜けてから再開する,
   "nfor (i, 0, 10) { { break continue break i; }; }");
t!(ブロックを抜けてから再開だけする,
   "var s := 0; nfor (i, 0, 3) { { break continue; }; s += 1; }; s");
t!(入れ子のループはそれぞれ持つ,
   "var s := 0; nfor (i, 0, 3) { nfor (j, 0, 3) { continue break j; }; }; s");
t!(遅延が二つあっても混ざらない,
   "var s := 0; nfor (i, 0, 6) { if (i == 1) continue break 100 fi;
    if (i == 3) continue break 200 fi; s += 1; }");

// ── `outward`：フレームを越える脱出（C-34 / C-70） ─────────────────────────

t!(越えて一段抜ける, "fn f () { break outward break 1; } -> i64; {{ f() }}");
t!(越えた先で二段抜ける,
   "fn f () { break outward break break 3; } -> i64; {{ { f() }; 9 }}");
t!(越えても外の領域は残る,
   "fn f () { break outward break 5; } -> i64; {{ { f() }; 9 }}");
t!(深いところから越える,
   "fn f () { { break break outward break 7; }; 0 } -> i64; {{ f() }}");
t!(条件つきで越える,
   "fn f (a : i64) { if (a > 2) break outward break a fi; a } -> i64;
    {{ f(1) + f(5) }}");
t!(越えない呼び出しは普通に返る, "fn f (a : i64) { a * 2 } -> i64; f(21)");
t!(越える関数でも越えない道は普通,
   "fn f (a : i64) { if (a > 100) break outward break a fi; a * 2 } -> i64;
    {{ f(21) }}");
t!(越えた値を使う,
   "fn f () { break outward break 6; } -> i64; {{ f() }} * 7");
