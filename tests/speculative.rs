//! **S-1 〜 S-4：私が決めた部分。**
//! 承認を経ていないので、本流に採るかは別途の判断による。

use vaak::check::check;
use vaak::interp::{run, Eval};
use vaak::parser::parse;
use vaak::types::check_types;

fn statics(src: &str) -> Vec<String> {
    let Ok(p) = parse(src) else { return vec!["構文エラー".into()] };
    let mut e: Vec<String> = check(&p).into_iter().map(|x| x.msg).collect();
    e.extend(check_types(&p).into_iter().map(|x| x.msg));
    e
}
#[track_caller]
fn v(src: &str, expect: &str) {
    let e = statics(src);
    assert!(e.is_empty(), "{src:?} は静的に通るはずだが: {e:?}");
    match run(src) {
        Ok(Eval::Value(x)) => assert_eq!(x.show(), expect, "{src:?}"),
        other => panic!("{src:?} は値 {expect} のはずだが {other:?}"),
    }
}
#[track_caller]
fn paradox(src: &str) {
    assert!(matches!(run(src), Ok(Eval::Paradox(_))), "{src:?} は paradox のはず");
}
#[track_caller]
fn static_err(src: &str) {
    assert!(!statics(src).is_empty(), "{src:?} は静的エラーのはず");
}
#[track_caller]
fn ok(src: &str) {
    let e = statics(src);
    assert!(e.is_empty(), "{src:?} は静的に通るはずだが: {e:?}");
    assert!(run(src).is_ok(), "{src:?} は走るはずだが: {:?}", run(src).err());
}

// ================= S-1 メンバ関数 =================

const POINT: &str = "struct Point { var x : i64 := 0; var y : i64 := 0; };";

#[test]
fn s1_メンバ関数は型の名前空間に入る() {
    v(&format!(
        "{POINT}
         fn Point.norm2 (self) {{ self.x * self.x + self.y * self.y }} -> i64;
         var p := new Point ( x := 3, y := 4 );
         p.norm2()"
    ), "25");
}

#[test]
fn s1_self_には型注釈を書かない() {
    // `fn T.m` と書いた時点で型が決まっている
    ok(&format!("{POINT} fn Point.id (self) {{ self.x }} -> i64;"));
    // 自由関数では省けない
    static_err("fn f (a) { a } -> i64;");
}

#[test]
fn s1_var_self_なら破壊できる() {
    v(&format!(
        "{POINT}
         fn Point.scale (var self, k : i64) {{ self.x *= k; self.y *= k; }};
         var p := new Point ( x := 2, y := 3 );
         p.scale(10);
         p.x + p.y"
    ), "50");
}

#[test]
fn s1_var_self_でなければ破壊できない() {
    // レシーバが let なら、var self のメンバ関数は呼べない
    static_err(&format!(
        "{POINT}
         fn Point.scale (var self, k : i64) {{ self.x *= k; }};
         let p := new Point ( x := 1 );
         p.scale(2);"
    ));
}

#[test]
fn s1_メンバ関数の引数と返り値の型を見る() {
    static_err(&format!(
        "{POINT}
         fn Point.add (self, k : i64) {{ self.x + k }} -> i64;
         var p := new Point ( x := 1 );
         p.add(1.0);"
    ));
}

#[test]
fn s1_型が違えば別のメンバ関数() {
    v("struct A { var v : i64 := 0; };
       struct B { var v : i64 := 0; };
       fn A.get (self) { self.v + 1 } -> i64;
       fn B.get (self) { self.v + 2 } -> i64;
       var a := new A ( v := 10 );
       var b := new B ( v := 10 );
       a.get() + b.get()", "23");
}

// ================= S-2 ラップ型 =================

#[test]
fn s2_wrap_で宣言する() {
    v("wrap Meters = i64;
       var m := new Meters ( 5 );
       new i64 ( m )", "5");
}

#[test]
fn s2_ラップ型は別の型() {
    // 暗黙には行き来しない
    static_err("wrap Meters = i64; var m : Meters := 5;");
    ok("wrap Meters = i64; var m : Meters := new Meters ( 5 );");
}

#[test]
fn s2_ラップ型にもメンバ関数を書ける() {
    v("wrap Meters = i64;
       fn Meters.double (self) { new Meters ( new i64 ( self ) * 2 ) } -> Meters;
       var m := new Meters ( 21 );
       new i64 ( m.double() )", "42");
}

// ================= S-3 標準ライブラリ =================

#[test]
fn s3_配列() {
    v("var a := [1, 2, 3]; a.len()", "3");
    v("var a := [1, 2, 3]; a.pop()", "3");
    v("var a := [1, 2, 3]; a.remove(0)", "1");
    v("var a := [1, 2]; a.insert(1, 9); a[1]", "9");
    v("var a := [1, 2]; a.clear(); a.len()", "0");
    // 空なら paradox
    paradox("var a := new i64 array ( 0, 0 ); a.pop()");
    paradox("var a := [1]; a.remove(5)");
}

#[test]
fn s3_写像() {
    v(r#"var m := ( "a" => 1, "b" => 2 ); m.len()"#, "2");
    v(r#"var m := ( "a" => 1 ); m.has("a")"#, "1");
    v(r#"var m := ( "a" => 1 ); m.has("z")"#, "0");
    v(r#"var m := ( "a" => 1, "b" => 2 ); m.remove("a")"#, "1");
    // 鍵の配列。**全順序なので並びが決まる**
    v(r#"var m := ( "b" => 1, "a" => 2 ); var k := m.keys(); k[0]"#, r#""a""#);
    paradox(r#"var m := ( "a" => 1 ); m.remove("z")"#);
}

#[test]
fn s3_str_は名前で数え方を示す() {
    // 添字とレンはバイト
    v(r#"var s := "あ"; s.len()"#, "3");
    // 符号位置は utf8_ が数える
    v(r#"var s := "あい"; s.utf8_len()"#, "2");
    v(r#"var s := "あい"; s.utf8_at(1)"#, "12356");
    v(r#"var s := "abc"; s.utf8_valid()"#, "1");
    paradox(r#"var s := "あ"; s.utf8_at(9)"#);
}

#[test]
fn s3_配列のメソッドは_str_に直接効く() {
    v(r#"var s := "ab"; s.pop()"#, "98");
    v(r#"var s := "ab"; var c : u8 := 99; s.push(c); s.len()"#, "3");
}

#[test]
fn s3_出力は持たない() {
    // `print` は無い。**出力はホストの仕事**
    static_err(r#"print("x");"#);
}

// ================= S-0 属性機構は要らない =================

#[test]
fn s0_相互再帰は抑止なしで通る() {
    v("fn a (n : i64) { if (n == 0) 0 else b(n - 1) fi } -> i64;
       fn b (n : i64) { if (n == 0) 1 else a(n - 1) fi } -> i64;
       a(3)", "1");
}
