//! プローブの「この通りに動きます」をそのままテストにする。
//! **各行が仕様である。期待値が書いてあるので、そのまま移せる。**

use vaak::interp::{run, Eval};

/// 値になることを確かめる。
#[track_caller]
fn v(src: &str, expect: &str) {
    match run(src) {
        Ok(Eval::Value(x)) => assert_eq!(x.show(), expect, "{src:?}"),
        other => panic!("{src:?} は値 {expect} のはずだが {other:?}"),
    }
}

/// 外界面が paradox になることを確かめる。
#[track_caller]
fn paradox(src: &str) {
    match run(src) {
        Ok(Eval::Paradox(_)) => {}
        other => panic!("{src:?} は paradox のはずだが {other:?}"),
    }
}

/// エラーになることを確かめる。
#[track_caller]
fn err(src: &str) {
    match run(src) {
        Err(_) => {}
        other => panic!("{src:?} はエラーのはずだが {other:?}"),
    }
}

/// 通ることだけ確かめる（値は問わない）。
#[track_caller]
fn ok(src: &str) {
    if let Err(e) = run(src) {
        panic!("{src:?} は通るはずだが: {e}");
    }
}

// ================= 領域 =================

#[test]
fn 領域は値を一つしか持てない() {
    v("{ 1; 2; 3 }", "3");
    paradox("{ 1; 2; 3; }");
    err("{ 1 2 }");
    paradox("{ }");
}

#[test]
fn 括弧は領域を作る() {
    v("( 1; 2 )", "2");
    paradox("( )");
}

#[test]
fn セミコロンは領域を要求し値は要求しない() {
    ok("1;;");
    paradox("{ ; }");
    paradox("( ; )");
    v("{ -1 }", "-1");
    v("{ 1 -2 }", "-1"); // 並べたつもりでも減算になる
}

// ================= paradox =================

#[test]
fn ゼロ除算は_paradox() {
    paradox("1 / 0");
    v("1 / 0 ?? 42", "42");
    paradox("1 / 0 ?? 2 / 0");
    v("1 / 0 ?? 2 / 0 ?? 42", "42");
    v("( 1 / 0 ) ?? 42", "42");
}

#[test]
fn 通常の演算子は_paradox_を受け付けない() {
    err("( 1 / 0 ) + 2 ?? 42");
}

#[test]
fn セミコロンは_paradox_を潰す() {
    paradox("1 / 0;");
}

#[test]
fn セルは_paradox_を保持しない() {
    err("let x := 1 / 0; x;");
    ok("let x := 1 / 0 ?? 0; x;");
}

#[test]
fn 引数の領域が_paradox_で終結したらエラー() {
    err("fn f (a : i64) { a } -> i64; f(1 / 0);");
}

// ================= 制御 =================

#[test]
fn if_は演算子() {
    v("if (1 == 1) 1 else 2 fi", "1");
    v("if (1 == 2) 1 else 2 fi", "2");
    paradox("if (1 == 2) 1 fi");
    v("if (1 == 2) 1 fi ?? 0", "0");
    err("if (1 == 1) 1 fi 2");
}

#[test]
fn if_の分岐は_e0_で_セミコロンを吸う() {
    paradox("if (1 == 1) 3; fi");
    ok("if (1 == 1) 3; fi;");
    ok("if (1 == 1) 3 fi;");
}

#[test]
fn ループの値は反復回数() {
    v("while (0) { } ?? 7", "7");
    v("var m := 100; while (m) { m /= 10; }", "3");
    paradox("nfor (i, 0, 0) { }");
}

#[test]
fn ループ本体に値が残ってはいけない() {
    err("loop { 1 }");
    ok("loop { 1; break; };");
}

#[test]
fn 脱出で終わったループの値は脱出が置いた値() {
    v("loop { break 5; }", "5");
    paradox("loop { break; }");
}

#[test]
fn ブロックが段を作るのは裸のときだけ() {
    // 本体の { } は loop の段。一段で抜ける
    v("loop { break 5; }", "5");
    // 裸のブロックは段が二つ。内側だけ抜ける → ループは続く
    v("var n := 0; loop { { break; }; n += 1; if (n == 3) break n; fi; }", "3");
}

#[test]
fn 段送りは書いた順に起きる() {
    v("{{ let x := 2; break break x }}", "2");
    v("loop { { break break 9; }; }", "9");
}

#[test]
fn switch_は上から順に照合する() {
    v("switch (2) case 1 => 10 case 2 => 20", "20");
    paradox("switch (3) case 1 => 10 case 2 => 20");
    v("switch (3) case 1 => 10 case 2 => 20 ?? 99", "99");
}

// ================= 脱出 =================

#[test]
fn 脱出の被演算子は_parse8() {
    v("loop { break 5 ?? 42; }", "5");
    v("loop { break 2 + 3; }", "5");
}

#[test]
fn continue_の被演算子は遅延する() {
    // 次の周回の先頭で break i が走る → i は 1
    v("nfor (i, 0, 10) { continue break i; }", "1");
}

#[test]
fn continue_continue_は周回を一つ余分に飛ばす() {
    // 0 周目で continue → 1 周目の先頭で継続 → 2 周目から普通に回る
    v("var n := 0; nfor (i, 0, 4) { if (i == 0) continue continue; fi; n += 1; }", "4");
    ok("var n := 0; nfor (i, 0, 4) { if (i == 0) continue continue; fi; n += 1; }; n;");
}

#[test]
fn 抜けた先がループでなければ制御のエラー() {
    err("loop { { continue; }; };");
}

#[test]
fn フレームの上限まで抜けるのは許される() {
    v("fn f () { break 42; } -> i64; f()", "42");
    v("fn f () { if (1 == 1) break 42 fi; } -> i64; f()", "42");
}

#[test]
fn 値の無い脱出は何も置かない() {
    paradox("fn f () { break; } -> i64; f()");
}

#[test]
fn repeat_は段を重ねる() {
    v("loop { { $repeat(break, 2) 7; }; }", "7");
    // n が 0 以下なら作用素を重ねない
    v("$repeat(break, 0) 5", "5");
}

#[test]
fn return_は標準ライブラリの作用素式() {
    v(
        "fn find (a : i64 array alias, x : i64) { nfor (i, 0, a.len()) { if (a[i] == x) $return i; fi; }; } -> i64;
         var xs := [ 3, 1, 4 ];
         find(xs, 4) ?? -1",
        "2",
    );
    v(
        "fn find (a : i64 array alias, x : i64) { nfor (i, 0, a.len()) { if (a[i] == x) $return i; fi; }; } -> i64;
         var xs := [ 3, 1, 4 ];
         find(xs, 9) ?? -1",
        "-1",
    );
}

// ================= 束縛 =================

#[test]
fn 宣言は値を置かない() {
    ok("var x := 5; var y := 6;");
    err("var x := 5 var y := 6");
    paradox("{ var x := 5 }");
}

#[test]
fn 代入は値を置かない() {
    paradox("var x := 1; { x := 5 }");
    err("var x := 1; (x := 5) + 3");
}

#[test]
fn 束縛種() {
    ok("var x := 1; x := 2;");
    err("let x := 1; x := 2;");
    err("const x := 1; x := 2;");
}

#[test]
fn 権限は増やせない() {
    ok("var a := 1; var b &= a;");
    err("let a := 1; var b &= a;");
    ok("let a := 1; const b &= a;"); // 凍結は能力を奪うだけ
}

#[test]
fn const_の別名は元の名前からの書き込みも禁じる() {
    err("var a := 1; { const b &= a; a := 2; };");
    ok("var a := 1; { const b &= a; }; a := 2;");
}

#[test]
fn 複製は深い() {
    v("var a := [ 1, 2 ]; var b := a; b[0] := 9; a[0]", "1");
}

#[test]
fn 別名は同じセルを指す() {
    v("var a := 1; var b &= a; b := 9; a", "9");
}

#[test]
fn 別名は指し直せる() {
    v("var a := 1; var c := 2; var b &= a; b &= c; b", "2");
}

#[test]
fn alias_引数に渡せるのは名前だけ() {
    ok("fn f (a : i64 array alias) { a.len() } -> i64; var xs := [1]; f(xs);");
    err("fn f (a : i64 array alias) { a.len() } -> i64; var xs := [[1]]; f(xs[0]);");
}

#[test]
fn 同じセルに別名が二つ届かない() {
    err("fn f (a : i64 alias, b : i64 alias) { a } -> i64; var p := 1; f(p, p);");
}

#[test]
fn 関数から局所変数は見えない() {
    err("{ var x := 1; fn f () { x } -> i64; f(); };");
}

#[test]
fn 前方宣言なしで相互再帰() {
    v(
        "fn is_odd (n : i64) { if (n == 0) 0 else is_even(n - 1) fi } -> u1;
         fn is_even (n : i64) { if (n == 0) 1 else is_odd(n - 1) fi } -> u1;
         is_odd(3)",
        "1",
    );
}

#[test]
fn 破壊的メンバ関数はレシーバに_var_を要求する() {
    ok("var a := [ 1 ]; a.push(2);");
    err("let a := [ 1 ]; a.push(2);");
}

#[test]
fn レシーバは経路でよい() {
    v("var arr := [ [ 1 ] ]; arr[0].push(3); arr[0].len()", "2");
}

// ================= 数値 =================

#[test]
fn 除算はユークリッド() {
    v("-7 / 3", "-3");
    v("-7 mod 3", "2");
    v("7 / -3", "-2");
    v("7 mod -3", "1");
    v("var xs := [ 1, 2, 3 ]; xs[(-1) mod 3]", "3");
}

#[test]
fn 浮動小数の非有限は_paradox() {
    paradox("1.0 / 0.0");
    paradox("1e308 * 1e308");
    v("1.0 / 0.0 ?? 0.0", "0.0");
}

#[test]
fn 短絡する() {
    v("0 && (1 / 0)", "0");
    v("1 || (1 / 0)", "1");
}

#[test]
fn 比較の結果は_u1() {
    v("1 < 2", "1");
    v("2 < 1", "0");
}

// ================= 集合体 =================

#[test]
fn 配列と写像() {
    v("var xs := [ 3, 1, 4 ]; xs.len()", "3");
    v("var xs := [ 3, 1, 4 ]; xs[1]", "1");
    paradox("var xs := [ 3 ]; xs[9]");
    v(r#"var m := ( "a" => 1 ); m["a"]"#, "1");
    paradox(r#"var m := ( "a" => 1 ); m["z"]"#);
}

#[test]
fn 構造体() {
    v(
        "struct Point { var x : i64 := 0; var y : i64 := 0; };
         var p := new Point ( x := 1, y := 2 );
         p.x + p.y",
        "3",
    );
    v(
        "struct Point { var x : i64 := 0; var y : i64 := 0; };
         var p := new Point ( x := 1 );
         p.y",
        "0",
    );
}

#[test]
fn const_は値ごと凍る() {
    err(
        "struct Point { var x : i64 := 0; };
         const p := new Point ( x := 1 );
         p.x := 2;",
    );
}

#[test]
fn 構築() {
    v("var a := new i64 array ( 3, 7 ); a[2]", "7");
    v("var m := new str i64 map ( ); m.len()", "0");
}

// ================= 実例 =================

#[test]
fn gcd() {
    v("fn gcd (a : i64, b : i64) { if (b == 0) a else gcd(b, a mod b) fi } -> i64; gcd(48, 18)", "6");
}

#[test]
fn 距離() {
    v(
        "struct Point { var x : i64 := 0; var y : i64 := 0; };
         fn dist2 (a : Point alias, b : Point alias) {
             let dx := a.x - b.x, dy := a.y - b.y;
             dx * dx + dy * dy
         } -> i64;
         var p := new Point ( x := 0, y := 0 );
         var q := new Point ( x := 3, y := 4 );
         dist2(p, q)",
        "25",
    );
}

#[test]
fn 静的検査を省いても_flow_の再帰は有限の誤りになる() {
    let src = "flow $a = $a; $a";
    assert!(vaak::interp::run(src).is_err());
    assert!(vaak::vm::run(src).is_err());
}
