//! 型検査。**暗黙の数値変換は無い。** リテラルだけが置かれた場所の型を受け取る。

use vaak::parser::parse;
use vaak::types::check_types;

#[track_caller]
fn errs(src: &str) -> Vec<String> {
    let p = parse(src).unwrap_or_else(|e| panic!("{src:?} の解析に失敗: {e}"));
    check_types(&p).into_iter().map(|e| e.msg).collect()
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
fn 暗黙の数値変換は無い() {
    ok("var a : i64 := 1; var b : i64 := 2; a + b;");
    bad("var a : i64 := 1; var b : u8 := 2; a + b;", "型が合わない");
    bad("var a : i64 := 1; var b : f64 := 1.0; a + b;", "型が合わない");
}

#[test]
fn リテラルは置かれた場所の型を受け取る() {
    ok("var a : u8 := 1; a + 2;");
    ok("var a : f32 := 1.0; a * 2.0;");
    ok("var a : u8 := 200;");
}

#[test]
fn 注釈が無ければ整数は_i64_浮動小数は_f64() {
    ok("var a := 1; var b : i64 := a;");
    bad("var a := 1; var b : u8 := a;", "型が合わない");
    ok("var a := 1.0; var b : f64 := a;");
}

#[test]
fn if_の条件は_u1() {
    ok("var c : u1 := 1; if (c) 1 fi;");
    bad("var n : i64 := 1; if (n) 1 fi;", "型が合わない");
    ok("if (1 < 2) 1 fi;");
}

#[test]
fn while_の条件は任意の整数型() {
    ok("var n : i64 := 3; while (n) { n -= 1; };");
    ok("var n : u8 := 3; while (n) { n -= 1; };");
    bad("var f : f64 := 1.0; while (f) { };", "整数でなければ");
}

#[test]
fn 利用者定義メソッドにも_alias_制約が掛かる() {
    bad(
        "struct P { var x : i64 := 0; };
         fn P.take (self, var a : i64 array alias) { a.len() } -> i64;
         var p := new P ( ); var xs := [[1]]; p.take(xs[0]);",
        "名前だけ",
    );
    bad(
        "struct P { var x : i64 := 0; };
         fn P.set (self, var a : i64 alias) { a := 9; };
         var p := new P ( ); let a := 1; p.set(a);",
        "権限は増やせない",
    );
    bad(
        "struct P { var x : i64 := 0; };
         fn P.set2 (self, var a : i64 alias, var b : i64 alias) { a := 2; b := 3; };
         var p := new P ( ); var a := 1; p.set2(a, a);",
        "別名が二つ",
    );
    bad(
        "struct P { var x : i64 := 0; };
         fn P.join (self, other : P alias) { self.x } -> i64;
         var p := new P ( ); p.join(p);",
        "別名が二つ",
    );
    // 同名の別型メソッドでは、レシーバ型から選んだ宣言だけを見る。
    ok(
        "struct P { var x : i64 := 0; }; struct Q { var x : i64 := 0; };
         fn P.use (self, x : i64) { x } -> i64;
         fn Q.use (self, var x : i64 alias) { x } -> i64;
         var p := new P ( ); p.use(1 + 2);",
    );
}

#[test]
fn 分岐は同じ型でなければならない() {
    ok("if (1 < 2) 1 else 2 fi;");
    bad("if (1 < 2) 1 else 1.0 fi;", "型が合わない");
}

#[test]
fn coalesce_は左右が同じ型() {
    ok("var m : str i64 map := ( \"a\" => 1 ); m[\"a\"] ?? 0;");
    bad("var m : str i64 map := ( \"a\" => 1 ); m[\"a\"] ?? 1.0;", "型が合わない");
}

#[test]
fn 比較の結果は_u1() {
    ok("var c : u1 := 1 < 2;");
    bad("var n : i64 := 1 < 2;", "型が合わない");
}

#[test]
fn 関数の引数と返り値() {
    ok("fn f (a : i64) { a } -> i64; f(1);");
    bad("fn f (a : i64) { a } -> i64; f(1.0);", "型が合わない");
    bad("fn f (a : i64) { a } -> u8;", "型が合わない");
    ok("fn f (a : u8) { a } -> u8; var x : u8 := 1; f(x);");
}

#[test]
fn 添字と欄() {
    ok("var xs : i64 array := [1, 2]; var a : i64 := xs[0];");
    bad("var xs : i64 array := [1, 2]; var a : u8 := xs[0];", "型が合わない");
    ok("var s : str := \"ab\"; var b : u8 := s[0];");
    ok("var m : str i64 map := ( \"a\" => 1 ); var v : i64 := m[\"a\"];");
    bad("var m : str i64 map := ( \"a\" => 1 ); m[1];", "型が合わない");
}

#[test]
fn 構造体の欄() {
    ok("struct P { var x : i64 := 0; }; var p := new P ( x := 1 ); var a : i64 := p.x;");
    bad("struct P { var x : i64 := 0; }; new P ( x := 1.0 );", "型が合わない");
    bad("struct P { var x : i64 := 0; }; new P ( y := 1 );", "欄 `y` は無い");
    bad("struct P { var x : i64; }; new P ( );", "欄 `x` に値が無い");
}

#[test]
fn 配列と写像のリテラル() {
    ok("var xs : u8 array := [1, 2, 3];");
    bad("var xs : i64 array := [1, 1.0];", "型が合わない");
    ok("var m : str i64 map := ( \"a\" => 1, \"b\" => 2 );");
    bad("var m : str i64 map := ( \"a\" => 1, 2 => 2 );", "型が合わない");
}

#[test]
fn str_の包みは_new_でだけ行き来する() {
    ok(r#"var bytes : u8 array := [65, 66]; var text := new str(bytes);"#);
    ok(r#"var text := "AB"; var bytes := new u8 array(text);"#);
    bad(
        r#"var text := "AB"; var bytes : u8 array := text;"#,
        "型が合わない",
    );
}

#[test]
fn 代入は型が合わねばならない() {
    ok("var a : i64 := 1; a := 2;");
    bad("var a : i64 := 1; a := 1.0;", "型が合わない");
    ok("var xs : i64 array := [1]; xs[0] := 5;");
}

#[test]
fn 脱出が運ぶ値も型が合わねばならない() {
    ok("fn f () { loop { break 1; } } -> i64;");
    bad("fn f () { loop { break 1.0; } } -> i64;", "型が合わない");
}

#[test]
fn push_は要素の型を取る() {
    ok("var xs : i64 array := [1]; xs.push(2);");
    bad("var xs : i64 array := [1]; xs.push(1.0);", "型が合わない");
    ok("var s : str := \"a\"; var b : u8 := 98; s.push(b);");
}

#[test]
fn プローブの実例が型検査を通る() {
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

#[test]
fn 注釈は集合体の中まで届く() {
    // C-94：`i32 array` と書いたなら、要素も `i32`
    ok("var a : i32 array := [1, 2, 3]; var x : i32 := a[0];");
    bad("var a : i32 array := [1]; var x : i64 := a[0];", "型が合わない");
    ok("var m : str i32 map := ( \"a\" => 1 ); var v : i32 := m[\"a\"];");
}
