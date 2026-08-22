//! **木を辿る実装（参照）とバイトコード VM の差分テスト。**
//!
//! この言語は意味論が特殊なので、**二つの実装を同じ入力に掛けて比べる**のが
//! 唯一の実際的な検証手段である（設計文書が繰り返し言っていること）。
//!
//! 落ちたら、**どちらかが仕様と違う。**

use vaak::interp::Eval;

fn shape(e: &Result<Eval, String>) -> String {
    match e {
        Ok(Eval::Value(v)) => format!("値 {}", v.show()),
        Ok(Eval::Paradox(_)) => "paradox".into(),
        Ok(Eval::Akasha) => "虚無".into(),
        Ok(Eval::Escape(_)) => "脱出".into(),
        Err(_) => "エラー".into(),
    }
}

#[track_caller]
fn same(src: &str) {
    let a = vaak::interp::run(src);
    let b = vaak::vm::run(src);
    assert_eq!(
        shape(&a),
        shape(&b),
        "\n  ソース: {src}\n  木: {a:?}\n  VM: {b:?}"
    );
}

/// プローブの「この通りに動きます」を、両方の実装に掛ける。
const CASES: &[&str] = &[
    // 領域
    "{ 1; 2; 3 }",
    "{ 1; 2; 3; }",
    "{ }",
    "( 1; 2 )",
    "( )",
    "1;;",
    "{ ; }",
    "{ -1 }",
    "{ 1 -2 }",
    "{ 1; -2 }",
    // paradox
    "1 / 0",
    "1 / 0 ?? 42",
    "1 / 0 ?? 2 / 0",
    "1 / 0 ?? 2 / 0 ?? 42",
    "( 1 / 0 ) ?? 42",
    "1 / 0;",
    "let x := 1 / 0 ?? 0; x",
    // 算術
    "-7 / 3",
    "-7 mod 3",
    "7 / -3",
    "7 mod -3",
    "-7 / -3",
    "-7 mod -3",
    "1.0 / 0.0",
    "1e308 * 1e308",
    "0 && (1 / 0)",
    "1 || (1 / 0)",
    "1 < 2",
    "2 < 1",
    "var a : u8 := 255; a + 1",
    "var a : i32 := 2147483647; a + 1",
    // 制御
    "if (1 == 1) 1 else 2 fi",
    "if (1 == 2) 1 else 2 fi",
    "if (1 == 2) 1 fi",
    "if (1 == 2) 1 fi ?? 0",
    "if (1 == 1) 3; fi",
    "if (1 == 1) 3 fi;",
    "while (0) { } ?? 7",
    "var m := 100; while (m) { m /= 10; }",
    "nfor (i, 0, 0) { }",
    "nfor (i, 0, 4) { }",
    "var s := 0; nfor (i, 10, 3) { s += i; }; s",
    "loop { break 5; }",
    "loop { break; }",
    "loop { 1; break 3; }",
    "switch (2) case 1 => 10 case 2 => 20",
    "switch (3) case 1 => 10 case 2 => 20",
    "switch (3) case 1 => 10 case 2 => 20 ?? 99",
    // 脱出
    "loop { break 5 ?? 42; }",
    "loop { break 2 + 3; }",
    "{{ let x := 2; break break x }}",
    "loop { { break break 9; }; }",
    "var n := 0; loop { { break; }; n += 1; if (n == 3) break n; fi; }",
    "loop { { $repeat(break, 2) 7; }; }",
    "$repeat(break, 0) 5",
    "nfor (i, 0, 10) { continue break i; }",
    "var n := 0; nfor (i, 0, 4) { if (i == 0) continue continue; fi; n += 1; }; n",
    // 束縛
    "var x := 5; var y := 6;",
    "var x := 1; { x := 5 }",
    "var a := [1, 2]; var b := a; b[0] := 9; a[0]",
    "var a := 1; var b &= a; b := 9; a",
    "var a := 1; var c := 2; var b &= a; b &= c; b",
    "var a := 10; a += 5; a",
    "var a := 10; a mod= 3; a",
    // 集合体
    "var xs := [ 3, 1, 4 ]; xs.len()",
    "var xs := [ 3, 1, 4 ]; xs[1]",
    "var xs := [ 3 ]; xs[9]",
    "var xs := [1, 2, 3]; xs.pop()",
    "var xs := [1, 2, 3]; xs.remove(0)",
    "var xs := [1, 2]; xs.insert(1, 9); xs[1]",
    "var xs := [1, 2]; xs.clear(); xs.len()",
    r#"var m := ( "a" => 1 ); m["a"]"#,
    r#"var m := ( "a" => 1 ); m["z"]"#,
    r#"var m := ( "a" => 1, "b" => 2 ); m.len()"#,
    r#"var m := ( "a" => 1 ); m.has("a")"#,
    r#"var m := ( "b" => 1, "a" => 2 ); var k := m.keys(); k[0]"#,
    r#"var s := "あい"; s.utf8_len()"#,
    r#"var s := "あい"; s.utf8_at(1)"#,
    r#"var s := "abc"; s.len()"#,
    "var arr := [ [ 1 ] ]; arr[0].push(3); arr[0].len()",
    // 関数
    "fn f () { 1 } -> i64; f()",
    "fn f () { break 42; } -> i64; f()",
    "fn f () { break; } -> i64; f()",
    "fn gcd (a : i64, b : i64) { if (b == 0) a else gcd(b, a mod b) fi } -> i64; gcd(48, 18)",
    "fn add (a : i64, b : i64) { a + b } -> i64; 1 |> add(2)",
    "fn is_odd (n : i64) { if (n == 0) 0 else is_even(n - 1) fi } -> u1;
     fn is_even (n : i64) { if (n == 0) 1 else is_odd(n - 1) fi } -> u1;
     is_odd(3)",
    "fn find (a : i64 array alias, x : i64) {
        nfor (i, 0, a.len()) { if (a[i] == x) $return i; fi; };
     } -> i64;
     var xs := [ 3, 1, 4 ]; find(xs, 4) ?? -1",
    "fn find (a : i64 array alias, x : i64) {
        nfor (i, 0, a.len()) { if (a[i] == x) $return i; fi; };
     } -> i64;
     var xs := [ 3, 1, 4 ]; find(xs, 9) ?? -1",
    // 構造体
    "struct P { var x : i64 := 0; var y : i64 := 0; };
     var p := new P ( x := 1, y := 2 ); p.x + p.y",
    "struct P { var x : i64 := 5; }; var p := new P ( ); p.x",
    "struct P { var x : i64 := 0; }; var p := new P ( x := 1 ); p.x := 7; p.x",
    "var a := new i64 array ( 3, 7 ); a[2]",
    "var m := new str i64 map ( ); m.len()",
    // メンバ関数（S-1）
    "struct P { var x : i64 := 0; };
     fn P.get (self) { self.x } -> i64;
     var p := new P ( x := 9 ); p.get()",
    "struct P { var x : i64 := 0; };
     fn P.bump (var self) { self.x += 1; };
     var p := new P ( x := 1 ); p.bump(); p.x",
    // ラップ型（S-2）
    "wrap M = i64; var m := new M ( 5 ); new i64 ( m )",
];

#[test]
fn 木と_vm_が同じ結果になる() {
    for src in CASES {
        same(src);
    }
}

#[test]
fn 実例が同じ結果になる() {
    same(
        "
struct Point { var x : i64 := 0; var y : i64 := 0; };

fn dist2 (a : Point alias, b : Point alias) {
    let dx := a.x - b.x, dy := a.y - b.y;
    dx * dx + dy * dy
} -> i64;

var p := new Point ( x := 0, y := 0 );
var q := new Point ( x := 3, y := 4 );
dist2(p, q)
",
    );
}

#[test]
fn alias_引数は呼び出し元と同じセルを指す() {
    same("fn set (var x : i64 alias) { x := 9; }; var a := 1; set(a); a");
}

#[test]
fn 値引数は呼び出し元のセルを指さない() {
    same("fn set (var x : i64) { x := 9; }; var a := 1; set(a); a");
    same(
        "fn set (var x : i64 array) { x[0] := 9; }; var a := [1]; set(a); a[0]",
    );
    // 後の引数が元のセルを書いても、先の値引数は評価時点の複製。
    same(
        "fn set (var x : i64 alias) { x := 9; 0 } -> i64;
         fn first (a : i64, b : i64) { a } -> i64;
         var x := 1; first(x, set(x))",
    );
}

#[test]
fn 同じセルに_alias_引数が二つ届くと誤りになる() {
    same(
        "fn f (a : i64 alias, b : i64 alias) { a } -> i64; var p := 1; f(p, p);",
    );
    same(
        "fn f (a : i64 alias, b : i64 alias) { a } -> i64;
         var p := 1; var q : i64 alias &= p; f(p, q);",
    );
}

#[test]
fn const_別名は同じセルへのほかの経路も凍らせる() {
    same("var a := 1; { const b : i64 alias &= a; a := 2; }; a");
    same("var a := 1; { const b : i64 alias &= a; }; a := 2; a");
    same("var a := [1]; { const b : i64 array alias &= a; a[0] := 2; }; a[0]");
    same("var a := 1; { const b : i64 alias &= a; break; }; a := 2; a");
    same("var a := 1; loop { const b : i64 alias &= a; break; }; a := 2; a");
    same(
        "var a := 1; nfor (i, 0, 2) { const b : i64 alias &= a; continue; }; a := 2; a",
    );
    same("fn f (const x : i64 alias) { x := 2; }; var a := 1; f(a); a");
    same("fn f (const x : i64 alias) { x; }; var a := 1; f(a); a := 2; a");
    same("fn f (const x : i64 alias) { break; }; var a := 1; f(a); a := 2; a");
}

#[test]
fn 可変メソッドの引数がレシーバを変えても変更を失わない() {
    let source =
        "fn add (var xs : i64 array alias) { xs.push(2); 3 } -> i64;
         var xs := [1]; xs.push(add(xs)); xs[1] * 10 + xs[2]";
    let reference = vaak::interp::run(source);
    let vm = vaak::vm::run(source);
    assert_eq!(shape(&reference), "値 23", "参照実装: {reference:?}");
    assert_eq!(shape(&vm), "値 23", "VM: {vm:?}");
}

#[test]
fn 名前の可変メソッドはセル上の配列を直接育てる() {
    same(
        "var stack : i64 array := new i64 array(0, 0);
         nfor (i, 0, 4096) { stack.push(i); };
         stack.len()",
    );
}

#[test]
fn 利用者定義メソッドの追加_alias_引数は呼び出し元のセルを指す() {
    let source =
        "struct P { var value : i64 := 0; };
         fn P.set (self, var target : i64 alias) { target := 9; };
         var p := new P ( );
         var target := 1;
         p.set(target);
         target";
    let reference = vaak::interp::run(source);
    let vm = vaak::vm::run(source);
    assert_eq!(shape(&reference), "値 9", "参照実装: {reference:?}");
    assert_eq!(shape(&vm), "値 9", "VM: {vm:?}");
}

#[test]
fn 利用者定義メソッドの値引数は後続引数より先に写す() {
    same(
        "struct P { var value : i64 := 0; };
         fn set (var target : i64 alias) { target := 9; 0 } -> i64;
         fn P.first (self, first : i64, second : i64) { first } -> i64;
         var p := new P ( );
         var target := 1;
         p.first(target, set(target)) * 10 + target",
    );
}

#[test]
fn 利用者定義メソッドでも同じ実セルに_alias_引数を二つ渡せない() {
    same(
        "struct P { var value : i64 := 0; };
         fn P.set2 (self, var a : i64 alias, var b : i64 alias) { a := 2; b := 3; };
         var p := new P ( ); var x := 1; p.set2(x, x); x",
    );
    // 名前が違っても &= で同じセルを指していれば拒む。
    same(
        "struct P { var value : i64 := 0; };
         fn P.set2 (self, var a : i64 alias, var b : i64 alias) { a := 2; b := 3; };
         var p := new P ( ); var x := 1; var y : i64 alias &= x; p.set2(x, y); x",
    );
}

// ===== S-16：分岐は領域である =====

/// 木を辿る実装と VM が同じ答えを出すこと。
fn 同じ(src: &str) {
    let p = vaak::parser::parse(src).unwrap_or_else(|e| panic!("{src}: {}", e.msg));
    let a = format!("{:?}", vaak::interp::Interp::new().run(&p));
    let c = vaak::vm::compile(&p).unwrap_or_else(|e| panic!("{src}: {}", e.msg));
    let b = format!("{:?}", vaak::vm::run_program(&c));
    assert_eq!(a, b, "{src}");
}

#[test]
fn s16_分岐が空になっても高さが揃う() {
    // **条件が真のとき**、分岐は `;` で空になる。それでも paradox が積まれる
    同じ("fn g (x : i64) { if (x == 0) x; fi; } -> i64; var f := 0; f += g(0) ?? 7; f");
    同じ("if (1 == 1) 5; fi ?? 9");
    同じ("if (1 == 2) 5; fi ?? 9");
    同じ("if (1 == 1) 5; else 6; fi ?? 9");
    同じ("var i := 0; if (i == 0) i += 1; else i += 2; fi; i");
}

#[test]
fn s16_括弧の領域は自分の底を持つ() {
    同じ("1 + (2)");
    同じ("1 + (2) + 3");
    同じ("var n := 1; n + (2)");
    同じ("1 + ( 2 ; 3 )");
    同じ("var c : u8 := 50; 1 + ((c - 48) -> i64)");
}

#[test]
fn 構造体の除去子は脱出側で型を失わない() {
    同じ(
        "flow $return = $repeat(break, getdepth());
         struct P { let x : i64 := 42; };
         fn maybe (yes : u1) { if (yes) new P ( ) fi } -> P;
         fn use (yes : u1) {
             let p := maybe(yes) ?? $return;
             p.x
         } -> i64;
         use(true) ?? 0",
    );
}

/// `hash` は木を辿る実装と VM で同じでなければならない（C-98）。
#[test]
fn hashも二つの実装で一致する() {
    同じ("var h := new i64 i64 hash ( ); h.len()");
    同じ("var h := new i64 i64 hash ( ); h[5] := 7; h[5]");
    同じ("var h := new i64 i64 hash ( ); h[5] := 7; h[9] := 3; h[5] * 100 + h[9] * 10 + h.len()");
    同じ("var h : i64 i64 hash := ( 1 => 10, 2 => 20 ); h[2]");
    同じ("var h : i64 i64 hash := ( 1 => 10 ); h[9]");
    同じ("var h : i64 i64 hash := ( 1 => 10 ); h[9] ?? 42");
    // **入れた順**である
    同じ("var h := new i64 i64 hash ( ); h[9] := 1; h[3] := 2; h[7] := 3; var k := h.keys(); k[0] * 100 + k[1] * 10 + k[2]");
    同じ("var h : i64 i64 hash := ( 1 => 1, 5 => 5 ); h.has(5)");
    同じ("var h : i64 i64 hash := ( 1 => 1 ); h.has(9)");
    同じ("var h : i64 i64 hash := ( 1 => 10, 2 => 20 ); h.remove(1) + h.len()");
    同じ("var h : i64 i64 hash := ( 1 => 10 ); h.remove(9)");
    同じ("var h : i64 i64 hash := ( 1 => 1, 2 => 2 ); h.remove(2) ?? 0; h[2] ?? 7");
    同じ("var h : i64 i64 hash := ( 1 => 1, 2 => 2 ); h.clear(); h.len()");
    同じ("var a : i64 i64 hash := ( 1 => 10 ); var b := a; b[1] := 99; a[1] * 100 + b[1]");
    同じ("var h : str i64 hash := ( \"a\" => 1, \"bb\" => 2 ); h[\"bb\"] * 10 + h.len()");
    // 抜いてから入れ直すと**末尾へ回る**
    同じ("var h := new i64 i64 hash ( ); h[1] := 1; h[2] := 2; h.remove(1) ?? 0; h[1] := 9; var k := h.keys(); k[0] * 10 + k[1]");
    同じ("var h := new i64 i64 hash ( ); var i := 0; while (i < 100) { h[i * 7] := i; i += 1; }; h.len() + h[7 * 40]");
}

/// 浮動小数の鍵は**数の順**に並び、`-0.0` は `0.0` と同じ鍵である（C-99）。
#[test]
fn 浮動小数の鍵も二つの実装で一致する() {
    同じ("var m : f64 i64 map := ( 1.0 => 1, 0.0 - 2.0 => 2, 3.0 => 3 ); var k := m.keys(); k[0]");
    同じ("var m : f64 i64 map := ( 1.0 => 1, 0.0 - 2.0 => 2 ); var k := m.keys(); k[0] < k[1]");
    同じ("var m := new f64 i64 map ( ); m[0.0] := 1; m[(0.0 - 1.0) * 0.0] := 2; m.len()");
    同じ("var h := new f64 i64 hash ( ); h[0.0] := 1; h[(0.0 - 1.0) * 0.0] := 2; h.len()");
    同じ("var m := new f64 i64 map ( ); m[0.0] := 1; m[(0.0 - 1.0) * 0.0] := 2; m[0.0]");
    同じ("var m : f64 i64 map := ( 1.5 => 7, 2.5 => 9 ); m[2.5]");
    同じ("var h : f64 i64 hash := ( 1.5 => 7, 2.5 => 9 ); h[2.5]");
    同じ("var m : f64 i64 map := ( 1.5 => 7 ); m[9.5] ?? 42");
    同じ("var m : f32 i64 map := ( 1.5 => 7, 0.0 - 2.5 => 9 ); var k := m.keys(); m[k[0]]");
    同じ("var m : f64 i64 map := ( 1.5 => 7, 2.5 => 9 ); m.remove(1.5) + m.len()");
    同じ("var h : f64 i64 hash := ( 1.5 => 7, 2.5 => 9 ); h.remove(1.5) + h.len()");
}
