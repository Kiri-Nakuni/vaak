//! 静的検査。**走らせる前に分かるもの**だけを見る。
//! paradox の消費は実行時なので、ここでは検査しない。

use vaak::check::check;
use vaak::parser::parse;

#[track_caller]
fn errs(src: &str) -> Vec<String> {
    let p = parse(src).unwrap_or_else(|e| panic!("{src:?} の解析に失敗: {e}"));
    check(&p).into_iter().map(|e| e.msg).collect()
}

#[track_caller]
fn ok(src: &str) {
    let e = errs(src);
    assert!(e.is_empty(), "{src:?} は通るはずだが: {e:?}");
}

#[track_caller]
fn bad(src: &str, needle: &str) {
    let e = errs(src);
    assert!(
        e.iter().any(|m| m.contains(needle)),
        "{src:?} は「{needle}」で落ちるはずだが: {e:?}"
    );
}

#[test]
fn 領域に値が二つ() {
    bad("{ 1 2 }", "値が二つ");
    bad("if (1) 1 fi 2", "値が二つ");
    ok("{ 1; 2 }");
}

#[test]
fn ループ本体に値が残ってはいけない() {
    bad("loop { 1 }", "値が残っている");
    ok("loop { 1; }");
    ok("loop { break; }");
}

#[test]
fn 脱出する経路は値を置かない() {
    ok("loop { if (1) break else 2; fi; }");
    bad("loop { if (1) break else 2 fi }", "値が残っている");
    ok("loop { if (1) break else 2 fi; }");
}

#[test]
fn 宣言はスコープを作る領域にしか置けない() {
    bad("if (1) var x := 1 fi;", "宣言を置けない");
    ok("if (1) { var x := 1; } fi;");
    bad("( var x := 1; x + 1 );", "宣言を置けない");
    ok("{ var x := 1; x + 1 }");
    bad("var a := 1; a && (var x := 1);", "宣言を置けない");
    bad("switch (1) case 1 => var x := 1", "宣言を置けない");
}

#[test]
fn 名前解決() {
    bad("xx := 5;", "知らない名前");
    ok("var xx := 1; xx := 5;");
    bad("f();", "知らない関数");
}

#[test]
fn 関数から局所変数は見えない() {
    bad("{ var x := 1; fn f () { x } -> i64; };", "知らない名前");
    ok("{ fn g () { 1 } -> i64; fn f () { g() } -> i64; };");
}

#[test]
fn 権限は増やせない() {
    ok("var a := 1; var b : i64 alias &= a;");
    bad("let a := 1; var b : i64 alias &= a;", "権限は増やせない");
    ok("let a := 1; const b : i64 alias &= a;");
    bad("let a := 1; a := 2;", "書けない");
}

#[test]
fn 破壊的メンバ関数はレシーバに_var_を要求する() {
    ok("var a := [1]; a.push(2);");
    bad("let a := [1]; a.push(2);", "`var` を要求する");
}

#[test]
fn alias_引数に渡せるのは名前だけ() {
    ok("fn f (a : i64 array alias) { a.len() } -> i64; var xs := [1]; f(xs);");
    bad(
        "fn f (a : i64 array alias) { a.len() } -> i64; var xs := [[1]]; f(xs[0]);",
        "名前だけ",
    );
}

#[test]
fn 同じセルに別名が二つ届かない() {
    bad(
        "fn f (a : i64 alias, b : i64 alias) { a } -> i64; var p := 1; f(p, p);",
        "別名が二つ",
    );
}

#[test]
fn 比較と代入は連鎖できない() {
    bad("1 < 2 < 3;", "連鎖");
    bad("var a := 1; var b := 1; var c := 1; a := b := c;", "連鎖");
    ok("(1 < 2) == 1;");
}

#[test]
fn 型依存グラフは_dag() {
    bad("struct Node { var next : Node; };", "循環");
    bad(
        "struct A { var b : B; }; struct B { var a : A; };",
        "循環",
    );
    ok("struct P { var x : i64; }; struct Q { var p : P; };");
}

#[test]
fn 段数はフレームを越えられない() {
    bad("fn f () { break break break; };", "フレームを越える");
    ok("fn f () { loop { break break; }; };");
    ok("fn f () { break; };");
}

#[test]
fn 再開できるものが無い() {
    bad("loop { { continue; }; };", "再開できるもの");
    ok("loop { continue; };");
}

#[test]
fn 矢印の無い関数は値を置けない() {
    bad("fn f () { 1 };", "paradox しか置けない");
    ok("fn f () { 1 } -> i64;");
    ok("fn f () { 1; };");
}

#[test]
fn alias_の束縛は_alias_バインドに限る() {
    bad("var a := 1; var b : i64 alias := a;", "`&=` に限る");
}

#[test]
fn プローブの実例が全部通る() {
    ok("
struct Point { var x : i64 := 0; var y : i64 := 0; };

fn dist2 (a : Point alias, b : Point alias) {
    let dx := a.x - b.x, dy := a.y - b.y;
    dx * dx + dy * dy
} -> i64;

fn gcd (a : i64, b : i64) {
    if (b == 0) a else gcd(b, a mod b) fi
} -> i64;

fn find (a : i64 array alias, x : i64) {
    nfor (i, 0, a.len()) {
        if (a[i] == x) $return i; fi;
    };
} -> i64;

var xs := [ 3, 1, 4, 1, 5 ];
let p := new Point ( x := 1, y := 2 );
let pos := find(xs, 42) ?? -1;
");
}
