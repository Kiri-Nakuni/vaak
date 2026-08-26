//! **決定 C-1 〜 C-92 と実装の照合。**
//!
//! 各決定について、実装がそのとおりかを一つずつ確かめる。
//! 「設計文書だけの決定」（却下理由の記録など）は対象外。
//! **落ちたら、実装か決定のどちらかが誤っている。**

use vaak::check::check;
use vaak::interp::{run, Eval};
use vaak::parser::parse;
use vaak::types::check_types;

fn statics(src: &str) -> Vec<String> {
    let Ok(p) = parse(src) else {
        return vec!["構文エラー".into()];
    };
    let mut e: Vec<String> = check(&p).into_iter().map(|x| x.msg).collect();
    e.extend(check_types(&p).into_iter().map(|x| x.msg));
    e
}

#[track_caller]
fn v(src: &str, expect: &str) {
    match run(src) {
        Ok(Eval::Value(x)) => assert_eq!(x.show(), expect, "{src:?}"),
        other => panic!("{src:?} は値 {expect} のはずだが {other:?}"),
    }
}
#[track_caller]
fn paradox(src: &str) {
    assert!(
        matches!(run(src), Ok(Eval::Paradox(_))),
        "{src:?} は paradox のはず"
    );
}
#[track_caller]
fn runtime_err(src: &str) {
    assert!(run(src).is_err(), "{src:?} は実行時エラーのはず");
    assert!(
        statics(src).is_empty(),
        "{src:?} は静的には通るはず: {:?}",
        statics(src)
    );
}
#[track_caller]
fn static_err(src: &str) {
    assert!(!statics(src).is_empty(), "{src:?} は静的エラーのはず");
}
#[track_caller]
fn ok(src: &str) {
    let e = statics(src);
    assert!(e.is_empty(), "{src:?} は静的に通るはずだが: {e:?}");
    assert!(
        run(src).is_ok(),
        "{src:?} は走るはずだが: {:?}",
        run(src).err()
    );
}
#[track_caller]
fn syntax_err(src: &str) {
    assert!(parse(src).is_err(), "{src:?} は構文エラーのはず");
}

// ---- C-1 跳躍を持たない ----
#[test]
fn c1_跳躍を持たない() {
    // `goto` は鍵語ですらない。ただの知らない名前として落ちる
    static_err("goto x;");
    ok("var goto := 1; goto;");
}

// ---- C-3 ループは反復回数を産む ----
#[test]
fn c3_ループは反復回数を産む() {
    v("var m := 100; while (m) { m /= 10; }", "3");
    v("nfor (i, 0, 4) { }", "4");
    paradox("nfor (i, 0, 0) { }");
}

// ---- C-4 switch を持つ ----
#[test]
fn c4_switch() {
    v("switch (2) case 1 => 10 case 2 => 20", "20");
}

// ---- C-6 / C-33 複製は深い ----
#[test]
fn c6_c33_複製は深い() {
    v("var a := [1, 2]; var b := a; b[0] := 9; a[0]", "1");
    v(
        "struct P { var x : i64 := 0; };
       var p := new P ( x := 1 ); var q := p; q.x := 9; p.x",
        "1",
    );
}

// ---- C-7 / C-77 str は u8 array をラップした型 ----
#[test]
fn c7_c77_str_はラップ型() {
    // 添字はバイトを返す。長さはバイト数
    v(r#"var s := "abc"; s.len()"#, "3");
    v(r#"var s := "abc"; s[0]"#, "97");
    // u8 array とは別の型（暗黙には行き来しない）
    static_err(r#"var s : str := "a"; var b : u8 array := s;"#);
    // 包むのも剥がすのも new
    ok(r#"var b := new u8 array ( 3, 97 ); var s := new str ( b ); s.len();"#);
    v(
        r#"var s := "abc"; var b := new u8 array ( s ); b[1] -> i64"#,
        "98",
    );
}

// ---- C-10 if は演算子。分岐は被演算子位置 ----
#[test]
fn c10_if_は演算子() {
    v("if (1 < 2) 1 else 2 fi", "1");
    // 分岐はスコープを作らない → 宣言を置けない
    static_err("if (1 < 2) var x := 1 fi;");
}

// ---- C-12 複合代入は糖衣 ----
#[test]
fn c12_複合代入は糖衣() {
    v("var a := 10; a += 5; a", "15");
    v("var a := 10; a mod= 3; a", "1");
}

// ---- C-14 値・虚無・paradox ----
#[test]
fn c14_虚無と_paradox_は別() {
    // 内面が空 → 外界面は paradox
    paradox("{ }");
    paradox("{ 1; }");
    // paradox を潰すのは `;`、消すのは `??`
    paradox("1 / 0;");
    v("1 / 0 ?? 7", "7");
}

// ---- C-15 |> ----
#[test]
fn c15_feed() {
    v(
        "fn add (a : i64, b : i64) { a + b } -> i64; 1 |> add(2)",
        "3",
    );
}

// ---- C-19 / C-48 別名は値ではない。束縛の形態 ----
#[test]
fn c19_c48_別名は束縛の形態() {
    // alias は型注釈の最も外側にのみ
    syntax_err("var y : i64 alias array &= x;");
    // alias の束縛は &= に限る
    static_err("var a := 1; var b : i64 alias := a;");
    // 別名は値の中に入らない → 再帰的データ構造は作れない
    static_err("struct Node { var next : Node; };");
}

// ---- C-20 束縛が複製する。アクセスは複製しない ----
#[test]
fn c20_アクセスは複製しない() {
    // メンバ関数のレシーバは複製されない（破壊的）
    v("var a := [1]; a.push(2); a.len()", "2");
}

// ---- C-21 数値の意味論 ----
#[test]
fn c21_数値() {
    // u1 が真偽値。比較は u1 を返す
    v("1 < 2", "1");
    // if の条件は u1 のみ（真偽値らしさが無い）
    static_err("var n : i64 := 1; if (n) 1 fi;");
    // while は任意の整数型
    ok("var n : i64 := 2; while (n) { n -= 1; };");
    // 0 除算は paradox
    paradox("1 / 0");
    // 暗黙変換は禁止
    static_err("var a : i64 := 1; var b : u8 := 1; a + b;");
}

// ---- C-23 / C-34 / C-70 脱出の細部 ----
#[test]
fn c23_c34_脱出はフレームで止まる() {
    // 上限まで抜けるのは許される
    v("fn f () { break 42; } -> i64; f()", "42");
    // 越えるのはエラー
    static_err("fn f () { break break; };");
    // outward を書けば越えられる。**書いた break の段送りに掛かる**
    ok("fn f () { break outward break 1; }; loop { f(); };");
    static_err("fn f () { { break outward break 1; }; };"); // 空振り
}

// ---- C-26 / C-39 switch の腕は一つの値。閉じる語を持たない ----
#[test]
fn c26_c39_switch() {
    // 腕は E@6。`;` を吸わない
    syntax_err("switch (1) case 1 => foo; case 2 => bar");
    // 最上位の ?? は switch 全体に掛かる
    v("switch (3) case 1 => 10 ?? 99", "99");
}

// ---- C-29 / C-66 paradox は型としては直和。-> は外界面の型 ----
#[test]
fn c29_c66_直和と外界面の型() {
    // -> が無ければ外界面は paradox のみ
    static_err("fn f () { 1 };");
    ok("fn f () { 1 } -> i64;");
    // セルは paradox を保持しない
    runtime_err("let x := 1 / 0; x;");
}

// ---- C-30 `:` は識別子に、`->` は領域に ----
#[test]
fn c30_注釈の位置() {
    ok("fn f (a : i64) { a } -> i64; f(1);");
    syntax_err("fn f (a : i64) -> i64 { a };");
}

// ---- C-31 最上位の外界面は言語の意味論ではない ----
#[test]
fn c31_最上位() {
    // 中身が空で終わってもエラーにならない
    ok("var x := 1;");
}

// ---- C-35 var / let / const ----
#[test]
fn c35_束縛種() {
    ok("var a := 1; a := 2;");
    static_err("let a := 1; a := 2;");
    static_err("const a := 1; a := 2;");
}

// ---- C-36 前方宣言を持たない ----
#[test]
fn c36_宣言はスコープ全体で見える() {
    v("fn a () { b() } -> i64; fn b () { 7 } -> i64; a()", "7");
    // 変数は宣言位置から先
    static_err("x; var x := 1;");
}

// ---- C-37 / C-78 const の凍結はスコープで解ける ----
#[test]
fn c37_const_の凍結() {
    runtime_err("var a := 1; { const b : i64 alias &= a; a := 2; };");
    ok("var a := 1; { const b : i64 alias &= a; }; a := 2;");
}

// ---- C-38 nfor の第三被演算子は回数 ----
#[test]
fn c38_nfor_は回数() {
    v("var s := 0; nfor (i, 10, 3) { s += i; }; s", "33"); // 10+11+12
}

// ---- C-40 宣言は paradox を産む ----
#[test]
fn c40_宣言は_paradox_を産む() {
    paradox("{ var x := 5 }");
    static_err("{ var x := 5 var y := 6 }");
}

// ---- C-42 コメント ----
#[test]
fn c42_コメント() {
    v("1 % 行コメント\n+ 2", "3");
    v("1 %{ 入れ子 %{ の }% コメント }% + 2", "3");
}

// ---- C-43 脱出は領域を閉じてから発生する ----
#[test]
fn c43_脱出は領域を閉じてから() {
    // break の値は paradox。`;` が要る
    ok("loop { if (1 < 2) break; fi; };");
}

// ---- C-44 ループ本体の内界面には何も置かれていない ----
#[test]
fn c44_ループ本体に値は残らない() {
    static_err("loop { 1 }");
    ok("loop { 1; break; };");
}

// ---- C-45 ?? の優先順位は非対称 ----
#[test]
fn c45_coalesce_は非対称() {
    // 左は強く、右は緩く
    v(
        "var m : str i64 map := ( \"a\" => 1 ); m[\"z\"] ?? 0 - 1",
        "-1",
    );
    v("1 + 1 / 0 ?? 5", "6"); // 1 + ((1/0) ?? 5)
}

// ---- C-46 / C-72 エラーの種類 ----
#[test]
fn c46_c72_エラーの種類() {
    // 静的
    static_err("{ 1 2 }");
    // 実行時（値）：消費されなかった paradox
    runtime_err("fn f (a : i64) { a } -> i64; f(1 / 0);");
    // 実行時（制御）：再開できるものが無い
    static_err("loop { { continue; }; };");
}

// ---- C-49 型は逆ポーランド ----
#[test]
fn c49_逆ポーランド() {
    ok("var m : str i64 map := ( \"a\" => 1 );");
    syntax_err("var m : i64 array map := x;"); // map は 2 つ取る
}

// ---- C-52 構築は new 型 ( 引数 ) ----
#[test]
fn c52_構築() {
    v("var a := new i64 array ( 3, 7 ); a[1]", "7");
    v(
        "struct P { var x : i64 := 5; }; var p := new P ( ); p.x",
        "5",
    );
}

// ---- C-53 別名は名前だけを指す ----
#[test]
fn c53_別名は名前だけ() {
    syntax_err("var b &= a[0];");
    static_err("fn f (a : i64 alias) { a } -> i64; var xs := [1]; f(xs[0]);");
}

// ---- C-54 配列リテラル ----
#[test]
fn c54_配列リテラル() {
    v("var xs := [ 3, 1, 4 ]; xs[2]", "4");
}

// ---- C-55 / C-71 outward は脱出を要求する ----
#[test]
fn c55_c71_被演算子の種類() {
    syntax_err("break outward;");
    syntax_err("break outward 42;");
    syntax_err("continue 1;");
    ok("nfor (i, 0, 3) { continue continue; };");
}

// ---- C-59 実行と評価を区別する ----
#[test]
fn c59_評価と実行() {
    // 値は書かれた位置で読まれる（break）
    v("{{ let x := 2; break break x }}", "2");
}

// ---- C-60 作用素式の名前は $ で始まる ----
#[test]
fn c60_flowname() {
    syntax_err("flow return = break;");
    ok("flow $r = break;");
}

// ---- C-62 代入は演算子。値を置かない ----
#[test]
fn c62_代入は演算子() {
    paradox("var x := 1; { x := 5 }");
    static_err("var a := 1; var b := 1; var c := 1; a := b := c;");
}

// ---- C-64 レシーバは経路でよい ----
#[test]
fn c64_レシーバは経路でよい() {
    v("var arr := [ [ 1 ] ]; arr[0].push(3); arr[0].len()", "2");
}

// ---- C-68 分岐は E@0。腕は E@6 ----
#[test]
fn c68_分岐と腕() {
    paradox("if (1 < 2) 3; fi"); // 分岐は ; を吸う
                                 // 腕は `;` を吸わないので、`;` は switch 全体に効く。次の腕が来れば構文エラー
    ok("switch (1) case 1 => 3;");
    syntax_err("switch (1) case 1 => 3; case 2 => 4");
}

// ---- C-69 + と - は左被演算子を要求しない ----
#[test]
fn c69_プラスマイナスは左を要求しない() {
    v("{ -1 }", "-1");
    v("{ 1 -2 }", "-1"); // 並べたつもりでも減算
    v("{ 1; -2 }", "-2"); // `;` の後は符号
}

// ---- C-73 break は即時、continue は遅延 ----
#[test]
fn c73_即時と遅延() {
    v("nfor (i, 0, 10) { continue break i; }", "1");
    static_err("nfor (i, 0, 10) { let t := i; continue break t; }");
}

// ---- C-75 / C-84 浮動小数 ----
#[test]
fn c75_c84_浮動小数() {
    paradox("1.0 / 0.0");
    paradox("0.0 / 0.0");
    paradox("1e308 * 1e308");
    // NaN が無いので比較は全順序
    v("1.0 == 1.0", "1");
}

// ---- C-76 除算はユークリッド ----
#[test]
fn c76_ユークリッド除算() {
    v("-7 / 3", "-3");
    v("-7 mod 3", "2");
    v("7 / -3", "-2");
    v("7 mod -3", "1");
    v("-7 / -3", "3");
    v("-7 mod -3", "2");
}

// ---- C-79 整数は 2^N を法とする ----
#[test]
fn c79_整数は法をとる() {
    v("var a : u8 := 255; a + 1", "0");
    v("var a : i32 := 2147483647; a + 1", "-2147483648");
}

// ---- C-80 `{ ; }` は通る ----
#[test]
fn c80_空の左辺でも_セミコロンは働く() {
    paradox("{ ; }");
    ok("1;;");
}

// ---- C-86 宣言はスコープを作る領域にしか置けない ----
#[test]
fn c86_宣言の位置() {
    static_err("if (1 < 2) var x := 1 fi;");
    static_err("( var x := 1; x );");
    static_err("switch (1) case 1 => var x := 1");
    ok("{ var x := 1; x }");
}

// ---- C-87 同じセルに別名が二つ届かない。レシーバも数える ----
#[test]
fn c87_別名の同一性() {
    static_err("fn f (a : i64 alias, b : i64 alias) { a } -> i64; var p := 1; f(p, p);");
}

// ---- C-90 領域ごとのアリーナ ----
#[test]
fn c90_領域を抜ければ捨てられる() {
    // 周回ごとに解放されるので、いくら回しても増えない
    ok("nfor (i, 0, 200) { var a := new i64 array ( 50, 0 ); a.len(); };");
}

// ---- C-92 continue continue は周回を一つ余分に飛ばす ----
#[test]
fn c92_continue_continue() {
    v(
        "var n := 0; nfor (i, 0, 4) { if (i == 0) continue continue; fi; n += 1; }; n",
        "2",
    );
}

// ---- C-94 注釈は集合体の中まで届く ----
#[test]
fn c94_注釈は集合体の中まで届く() {
    // 要素が i32 なら、溢れは 2^32 を法として折り返す
    v("var a : i32 array := [2147483647]; a[0] + 1", "-2147483648");
    v(
        "var a : i32 array := new i32 array ( 2, 0 ); a[0] := 2147483647; a[0] + 1",
        "-2147483648",
    );
    // 写像の値も
    v(
        r#"var m : str i32 map := ( "a" => 2147483647 ); m["a"] + 1"#,
        "-2147483648",
    );
}
