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

#[track_caller]
fn same_value(src: &str, expect: &str) {
    for (name, got) in [("木", vaak::interp::run(src)), ("VM", vaak::vm::run(src))] {
        match got {
            Ok(Eval::Value(v)) => assert_eq!(v.show(), expect, "{name}: {src}"),
            other => panic!("{name}: {src} は値 {expect} のはずだが {other:?}"),
        }
    }
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
    // str の包みと剥がし（C-78）。どちらも深く複製する
    r#"var bytes : u8 array := [65, 66, 67]; var text := new str(bytes);
       text[0] := 90; (bytes[0] -> i64) + (text[0] -> i64)"#,
    r#"var bytes := new u8 array(3, 97); var text := new str(bytes);
       var roundtrip := new u8 array(text); roundtrip[1] -> i64"#,
];

#[test]
fn 木と_vm_が同じ結果になる() {
    for src in CASES {
        same(src);
    }
}

#[test]
fn strを剥がした配列を書き換えても元は変わらない() {
    same_value(
        r#"let text := "ABC"; var bytes := new u8 array(text);
           bytes[0] := 90; (text[0] -> i64) + (bytes[0] -> i64)"#,
        "155",
    );
}

#[test]
fn u8配列の一引数構築は長さを失わず零で埋める() {
    same_value(
        "var bytes := new u8 array(3);
         bytes.len() * 1000
           + (bytes[0] -> i64) * 100
           + (bytes[1] -> i64) * 10
           + (bytes[2] -> i64)",
        "3000",
    );
}

#[test]
fn 文脈で決まった数値型まで同じになる() {
    same_value("var x : u8 := 0; x := 300; x", "44");
    same_value(
        "struct P { var x : u8 := 0; };
         var p := new P (); p.x := 300; p.x",
        "44",
    );
    same_value("fn f (x : u8) { x } -> u8; f(300)", "44");
    same_value("fn f () { 300 } -> u8; f()", "44");
    same_value(
        "var x : f32 := 3.0e38;
         if (((x + x) ?? (7.0 -> f32)) == (7.0 -> f32)) 1 else 0 fi",
        "1",
    );
    same_value(
        "var x : f64 := 3.5e38;
         if (((x -> f32) ?? (7.0 -> f32)) == (7.0 -> f32)) 1 else 0 fi",
        "1",
    );
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
fn 入れ子の左辺は根を写さず末端だけを書き換える() {
    same_value(
        "struct Node { var value : i64 := 0; };
         var rows := [ [ new Node ( value := 1 ) ], [ new Node ( value := 2 ) ] ];
         rows[1][0].value := 9;
         rows[0][0].value * 10 + rows[1][0].value",
        "19",
    );
    same_value(
        "var rows := [ [1, 2], [3, 4] ];
         rows[1][0] += 7;
         rows[0][0] * 100 + rows[1][0]",
        "110",
    );
}

#[test]
fn 入れ子の読み取りも根を写さず欠損位置を区別する() {
    same_value(
        "struct Bucket { var data : i64 array; };
         var bucket := new Bucket ( data := [3, 4, 5] );
         bucket.data[1] * 10 + bucket.data.len()",
        "43",
    );
    // 最終添字の欠損だけが paradox。途中が欠ければ、その先は辿れない。
    same_value("var rows := [[1]]; rows[0][9] ?? 42", "42");
    same("var rows := [[1]]; rows[9][0] ?? 42");
}

#[test]
fn 代入の添字は右辺より先に一度だけ評価する() {
    same_value(
        "fn take_index (var calls : i64 alias) {
             let old := calls;
             calls += 1;
             old
         } -> i64;
         var calls := 0;
         var values := [0, 0];
         values[take_index(calls)] := calls * 10;
         calls * 100 + values[0] * 10 + values[1]",
        "200",
    );
    same_value(
        "fn take_index (var calls : i64 alias) {
             let old := calls;
             calls += 1;
             old
         } -> i64;
         var calls := 0;
         var values := [1, 2];
         values[take_index(calls)] += calls * 10;
         calls * 100 + values[0] * 10 + values[1]",
        "212",
    );
}

#[test]
fn 複合代入は右辺が変えた後の現在値へ重ねる() {
    same_value(
        "fn replace (var values : i64 array alias) {
             values[0] := 20;
             3
         } -> i64;
         var values := [5];
         values[0] += replace(values);
         values[0]",
        "23",
    );
    same_value(
        "fn replace (var value : i64 alias) { value := 20; 3 } -> i64;
         var value := 5;
         value += replace(value);
         value",
        "23",
    );
}

#[test]
fn 入れ子の左辺も別名と凍結を実セルで判定する() {
    same_value(
        "var rows := [[1]];
         var view : i64 array array alias &= rows;
         view[0][0] := 9;
         rows[0][0]",
        "9",
    );
    same(
        "var rows := [[1]];
         { const frozen : i64 array array alias &= rows; rows[0][0] := 9; };
         rows[0][0]",
    );
}

#[test]
fn 入れ子の置き場の数値幅は右辺の途中まで届く() {
    same_value(
        "struct Pixel { var channel : u8 := 0; };
         var pixels : Pixel array := [new Pixel ()];
         pixels[0].channel := 100 * 3 / 2;
         pixels[0].channel",
        "22",
    );
    same_value(
        "var bytes : u8 array array := [[0]];
         bytes[0][0] := 100 * 3 / 2;
         bytes[0][0]",
        "22",
    );
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

/// **注釈は演算の途中まで届く**（C-100）。
///
/// `100 * 3 / 2` は `u8` の場では 22 である——`100 * 3` が先に折り返す。
/// `i64` の場では 150 のままである。**場所が計算の幅を決める。**
#[test]
fn 文脈の型は演算の中まで届く() {
    同じ("var x : u8 := 100 * 3 / 2; x");
    同じ("var x : u8 := 0; x := 100 * 3 / 2; x");
    同じ("var x : u8 := { 100 * 3 / 2 }; x");
    同じ("var x : u8 := ( 100 * 3 / 2 ); x");
    同じ("var x : u8 := if (1 == 1) 100 * 3 / 2 else 0 fi; x");
    同じ("(100 * 3 / 2) -> u8");
    同じ("fn f (a : u8) { a } -> u8; f(100 * 3 / 2)");
    同じ("fn f () { 100 * 3 / 2 } -> u8; f()");
    同じ("struct P { var x : u8 := 0; }; var p := new P ( x := 100 * 3 / 2 ); p.x");
    同じ("struct P { var x : u8 := 100 * 3 / 2; }; var p := new P (); p.x");
    同じ("var a : u8 array := [ 100 * 3 / 2 ]; a[0]");
    // **リテラル自体が枠を越える場合**
    同じ("var x : u8 := 256 / 2; x");
    同じ("var x : u8 := 256; x");
    同じ("var x : i32 := 100000 * 100000 / 1000; x");
    // **`i64` の場では今までどおり**
    同じ("var x : i64 := 100 * 3 / 2; x");
    同じ("100 * 3 / 2");
    // **名前で書いても同じ答えになる**（これが揃っていなかった）
    同じ("var a : u8 := 100; var b : u8 := 3; var c : u8 := 2; var x : u8 := 0; x := a * b / c; x");
    // **添字は `i64` のまま。** 外の型は届かない
    同じ("var a := new i64 array(300, 0); a[299] := 7; var x : i64 := 0; x := a[299]; x");
    // **比較は左右で揃う。** 外の型は届かない
    同じ("var x : u8 := 200; if (x == 200) 1 else 0 fi");
    // **桁数は別の型でよい。** そして幅以上ずらすとこぼれる
    同じ("var x : u8 := 1 << 9; x");
    同じ("var x : u8 := 1 << 7; x");
    同じ("var x : u8 := 255 >> 9; x");
    同じ("var x : i32 := 0 - 1; x := x >> 40; x");
    同じ("var x : i64 := 1 << 64; x");
    同じ("var x : i64 := 0 - 8; x := x >> 100; x");
}

/// 数のメンバ関数（S-23）。**LLVM の命令にあるものを名前で言える。**
#[test]
fn 数のメンバ関数() {
    // 絶対値は**折り返す**（C-21）
    同じ("var x : i64 := 0 - 5; x.abs()");
    同じ("var x : i32 := 0 - 2147483648; x.abs()");
    同じ("var x : u8 := 200; x.abs()");
    同じ("var a : i64 := 5; a.min(3)");
    同じ("var a : i64 := 5; a.max(3)");
    同じ("var a : u8 := 5; a.min(200)");
    // 数える系は **`i64` を返す**（値ではなく個数）
    同じ("var x : u8 := 0b1011; x.count_ones()");
    同じ("var x : i64 := 0; x.count_ones()");
    同じ("var x : u8 := 0b00010000; x.leading_zeros()");
    同じ("var x : u8 := 0; x.leading_zeros()");
    同じ("var x : u8 := 0b00010000; x.trailing_zeros()");
    同じ("var x : u8 := 0; x.trailing_zeros()");
    同じ("var x : i64 := 1; x.leading_zeros()");
    同じ("var x : u8 := 0b11010000; x.reverse_bits()");
    同じ("var x : u16 := 0x1234; x.swap_bytes()");
    同じ("var x : i64 := 1; x.swap_bytes()");
    // 回すのは幅で割った余りだけ
    同じ("var x : u8 := 1; x.rotate_left(1)");
    同じ("var x : u8 := 1; x.rotate_left(8)");
    同じ("var x : u8 := 1; x.rotate_left(9)");
    同じ("var x : u8 := 1; x.rotate_right(1)");
    同じ("var x : u8 := 0b10000000; x.rotate_left(1)");
    // **折り返すか止まるかが名前で分かる**
    同じ("var x : u8 := 250; x.saturating_add(10)");
    同じ("var x : u8 := 250; x + 10");
    同じ("var x : u8 := 5; x.saturating_sub(10)");
    同じ("var x : i32 := 100000; x.saturating_mul(100000)");
    同じ("var x : i32 := 0 - 100000; x.saturating_mul(100000)");
    同じ("var x : u8 := 20; x.saturating_mul(20)");
    // 浮動小数。**非有限は paradox**（C-84）
    同じ("var x : f64 := 4.0; x.sqrt()");
    同じ("var x : f64 := 0.0 - 1.0; x.sqrt()");
    同じ("var x : f64 := 0.0 - 1.0; (x.sqrt() ?? 0.0) > 0.0 - 1.0");
    同じ("var x : f64 := 0.0 - 2.5; x.abs()");
    同じ("var x : f64 := 2.5; x.floor()");
    同じ("var x : f64 := 2.5; x.ceil()");
    同じ("var x : f64 := 0.0 - 2.5; x.trunc()");
    同じ("var x : f64 := 2.5; x.round()");
    同じ("var x : f64 := 1.0; x.min(2.0)");
    同じ("var x : f64 := 1.0; x.max(2.0)");
    同じ("var x : f64 := 3.0; x.copysign(0.0 - 1.0)");
    同じ("var x : f64 := 2.0; x.mul_add(3.0, 1.0)");
    同じ("var x : f64 := 2.0; x.pow(10.0)");
    同じ("var x : f64 := 0.0; x.ln()");
    同じ("var x : f64 := 8.0; x.log2()");
    同じ("var x : f64 := 1000.0; x.log10()");
    同じ("var x : f64 := 0.0; x.exp()");
    同じ("var x : f64 := 0.0; x.sin()");
    同じ("var x : f64 := 0.0; x.cos()");
    // f32 でも同じ形
    同じ("var x : f32 := 4.0; x.sqrt()");
    同じ("var x : f32 := 0.0 - 2.5; x.abs()");
}

/// 表記の読み方は**三実装で同じ**でなければならない。
#[test]
fn 十進以外の表記() {
    同じ("0b1011");
    同じ("0xff");
    同じ("0o777");
    同じ("1_000");
    同じ("0xFF_FF");
    同じ("var x : u8 := 0b1111_0000; x");
}

/// 包み型は**実装から見れば基底型**である（S-2）。関数の返りでも剥がれる。
#[test]
fn 包み型は関数の返りでも剥がれる() {
    同じ("wrap NodeId = i64; fn mk (n : i64) { new NodeId(n) } -> NodeId; mk(7) -> i64");
    同じ("wrap NodeId = i64; fn mk (n : i64) { new NodeId(n) } -> NodeId;
          fn use2 (h : NodeId) { (h -> i64) * 2 } -> i64; use2(mk(21))");
    同じ("wrap M = i64; var a : M array := [ new M(1), new M(2) ]; (a[1] ?? new M(0)) -> i64");
}
