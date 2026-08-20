//! プローブの解析例をそのままテストにする。
//! **期待値が書いてあるので、そのまま移せる。**

use vaak::ast::*;
use vaak::parser::{parse, parse_expr};

/// 括弧付きの前置記法に落として比べる。木の形だけを見る。
fn sexp(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Int(s) | ExprKind::Float(s) => s.clone(),
        ExprKind::Str(s) => format!("{s:?}"),
        ExprKind::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
        ExprKind::Ascribe { expr, ty: tv } => format!("(-> {} {})", sexp(expr), ty(&tv.value)),
        ExprKind::Name(s) => s.clone(),
        ExprKind::Paren(v) => format!("(paren{})", items(v)),
        ExprKind::Block(v) => format!("(block{})", items(v)),
        ExprKind::ArrayLit(v) => format!("(array{})", items(v)),
        ExprKind::MapLit(v) => {
            let mut s = String::from("(map");
            for (k, val) in v {
                s.push_str(&format!(" [{} {}]", sexp(k), sexp(val)));
            }
            s + ")"
        }
        ExprKind::Unary { op, rhs } => format!("({} {})", un(*op), sexp(rhs)),
        ExprKind::Binary { op, lhs, rhs } => {
            format!("({} {} {})", bin(*op), sexp(lhs), sexp(rhs))
        }
        ExprKind::Discard(Some(x)) => format!("(; {})", sexp(x)),
        ExprKind::Discard(None) => "(; -)".to_string(),
        ExprKind::Field { base, name } => format!("(. {} {name})", sexp(base)),
        ExprKind::Index { base, index } => format!("(idx {} {})", sexp(base), sexp(index)),
        ExprKind::Call { callee, args } => format!("(call {}{})", sexp(callee), items(args)),
        ExprKind::Assign { op, lhs, rhs } => {
            format!("({} {} {})", asn(*op), sexp(lhs), sexp(rhs))
        }
        ExprKind::Escape(esc) => esc_s(esc),
        ExprKind::Decl(d) => {
            let mut s = format!("({}", match d.kind {
                BindKind::Var => "var",
                BindKind::Let => "let",
                BindKind::Const => "const",
            });
            for b in &d.bindings {
                match &b.init {
                    BindInit::Value(e) => s.push_str(&format!(" [{} := {}]", b.name, sexp(e))),
                    BindInit::AliasOf(n) => s.push_str(&format!(" [{} &= {n}]", b.name)),
                }
            }
            s + ")"
        }
        ExprKind::FnDecl(f) => format!(
            "(fn {}{} {}{})",
            match &f.owner { Some(t) => format!("{t}.{}", f.name), None => f.name.clone() },
            f.params.iter().map(|p| format!(" {}", p.name)).collect::<String>(),
            sexp(&f.body),
            f.ret.as_ref().map(|t| format!(" -> {}", ty(&t.value))).unwrap_or_default()
        ),
        ExprKind::FlowDecl(f) => format!("(flow {} {})", f.name, esc_s(&f.body)),
        ExprKind::StructDecl(s) => format!("(struct {})", s.name),
        ExprKind::WrapDecl(w) => format!("(wrap {})", w.name),
        ExprKind::Construct { ty: t, args } => {
            let a = match args {
                CtorArgs::Named(v) => {
                    v.iter().map(|(n, e)| format!(" [{n} {}]", sexp(e))).collect::<String>()
                }
                CtorArgs::Positional(v) => items(v),
            };
            format!("(new {}{a})", ty(&t.value))
        }
        ExprKind::If(i) => {
            let mut s = String::from("(if");
            for (c, b) in &i.arms {
                s.push_str(&format!(" [{} {}]", sexp(c), sexp(b)));
            }
            if let Some(e) = &i.els {
                s.push_str(&format!(" [else {}]", sexp(e)));
            }
            s + ")"
        }
        ExprKind::Loop(b) => format!("(loop {})", sexp(b)),
        ExprKind::While { cond, body } => format!("(while {} {})", sexp(cond), sexp(body)),
        ExprKind::NFor { name, start, count, body } => {
            format!("(nfor {name} {} {} {})", sexp(start), sexp(count), sexp(body))
        }
        ExprKind::Switch { subject, arms } => {
            let mut s = format!("(switch {}", sexp(subject));
            for a in arms {
                s.push_str(&format!(" [{} => {}]", sexp(&a.pattern), sexp(&a.value)));
            }
            s + ")"
        }
    }
}

fn items(v: &[Expr]) -> String {
    v.iter().map(|e| format!(" {}", sexp(e))).collect()
}

fn esc_s(e: &Escape) -> String {
    let head = match &e.kind {
        EscapeKind::Break { outward: true } => "break-outward".to_string(),
        EscapeKind::Break { outward: false } => "break".to_string(),
        EscapeKind::Continue => "continue".to_string(),
        EscapeKind::Flow { name, args } => {
            if args.is_empty() {
                name.clone()
            } else {
                let a: String = args
                    .iter()
                    .map(|x| match x {
                        FlowArg::Escape(e) => format!(" {}", esc_s(e)),
                        FlowArg::Value(v) => format!(" {}", sexp(v)),
                    })
                    .collect();
                format!("{name}({})", a.trim())
            }
        }
    };
    match &e.operand {
        None => format!("({head})"),
        Some(Operand::Value(v)) => format!("({head} {})", sexp(v)),
        Some(Operand::Escape(x)) => format!("({head} {})", esc_s(x)),
    }
}

fn un(o: UnOp) -> &'static str {
    match o {
        UnOp::Neg => "neg",
        UnOp::Pos => "pos",
        UnOp::Not => "!",
    }
}

fn bin(o: BinOp) -> &'static str {
    use BinOp::*;
    match o {
        Add => "+", Sub => "-", Mul => "*", Div => "/", Mod => "mod",
        Shl => "<<", Shr => ">>", BitAnd => "&", BitXor => "^", BitOr => "|",
        Lt => "<", Le => "<=", Gt => ">", Ge => ">=", Eq => "==", Ne => "!=",
        And => "&&", Or => "||", Coalesce => "??", Feed => "|>",
    }
}

fn asn(o: AssignOp) -> &'static str {
    use AssignOp::*;
    match o {
        Set => ":=", Alias => "&=", Add => "+=", Sub => "-=", Mul => "*=",
        Div => "/=", Mod => "mod=", Shl => "<<=", Shr => ">>=", BitXor => "^=", BitOr => "|=",
    }
}

fn ty(t: &ValueType) -> String {
    use ValueType::*;
    match t {
        U1 => "u1".into(), U8 => "u8".into(), U16 => "u16".into(), U32 => "u32".into(),
        I32 => "i32".into(), I64 => "i64".into(), F32 => "f32".into(), F64 => "f64".into(),
        Str => "str".into(),
        Array(i) => format!("{} array", ty(i)),
        Map(k, v) => format!("{} {} map", ty(k), ty(v)),
        Named(n) => n.clone(),
    }
}

#[track_caller]
fn t(src: &str, expect: &str) {
    let e = parse_expr(src).unwrap_or_else(|e| panic!("{src:?} の解析に失敗: {e}"));
    assert_eq!(sexp(&e), expect, "{src:?}");
}

#[track_caller]
fn all(src: &str, expect: &str) {
    let p = parse(src).unwrap_or_else(|e| panic!("{src:?} の解析に失敗: {e}"));
    let s: Vec<String> = p.body.iter().map(sexp).collect();
    assert_eq!(s.join(" "), expect, "{src:?}");
}

#[track_caller]
fn bad(src: &str) {
    assert!(parse(src).is_err(), "{src:?} は構文エラーのはず");
}

// ---- 束縛力表からそのまま ----

#[test]
fn coalesce_は非対称() {
    t("a + b / c ?? d + e", "(+ a (?? (/ b c) (+ d e)))");
    t("a ?? b ?? c", "(?? a (?? b c))");
    t("a ?? b < c", "(< (?? a b) c)");
    t("table[k] ?? 0 - 1", "(?? (idx table k) (- 0 1))");
    t("a + b / c ?? d", "(+ a (?? (/ b c) d))");
}

#[test]
fn 脱出の被演算子は_parse8() {
    all("break 5;", "(; (break 5))");
    t("break x + y", "(break (+ x y))");
    t("break 5 ?? 42", "(break (?? 5 42))");
    t("break break break", "(break (break (break)))");
}

#[test]
fn 代入は演算子で_セミコロンより強い() {
    all("x := 5;", "(; (:= x 5))");
    t("break x := 5", "(:= (break x) 5)");
    t("a := b := c", "(:= a (:= b c))"); // 非結合は別の検査で禁じる
}

// ---- `;` と has_value ----

#[test]
fn セミコロンは領域を要求し_値は要求しない() {
    all("a; -b", "(; a) (neg b)");
    all("a -b", "(- a b)");
    all("1;;", "(; (; 1))");
    all("{ ; }", "(block (; -))");
    all("( ; )", "(paren (; -))");
    all("{ -1 }", "(block (neg 1))");
}

#[test]
fn 括弧は呼び出しになる() {
    all("{ a (-b) }", "(block (call a (neg b)))");
}

#[test]
fn 領域の中身は並ぶ() {
    all("{ a; b; c }", "(block (; a) (; b) c)");
    all("{ a; b; c; }", "(block (; a) (; b) (; c))");
    all("{ a b }", "(block a b)"); // 値が二つ。静的検査で落とす
    all("{ }", "(block)");
}

// ---- 制御 ----

#[test]
fn if_の分岐は_e0_で_セミコロンを吸う() {
    all("if (c) a; fi", "(if [(paren c) (; a)])");
    all("if (c) a fi;", "(; (if [(paren c) a]))");
    all("if (c) ( a; b ) fi", "(if [(paren c) (paren (; a) b)])");
    bad("if (c) a b fi");
}

#[test]
fn switch_の腕は_e6_で_セミコロンを吸わない() {
    all("let x := switch (a) case 1 => 10;", "(; (let [x := (switch (paren a) [1 => 10])]))");
    bad("switch (x) case 1 => foo; case 2 => bar");
    all(
        "switch (x) case 1 => foo case 2 => bar",
        "(switch (paren x) [1 => foo] [2 => bar])",
    );
}

#[test]
fn 腕は最上位の_coalesce_を消費しない() {
    all(
        "switch (v) case 1 => 2 ?? 0",
        "(?? (switch (paren v) [1 => 2]) 0)",
    );
    all(
        "switch (v) case 1 => ( 2 ?? 0 )",
        "(switch (paren v) [1 => (paren (?? 2 0))])",
    );
}

#[test]
fn 入れ子の_switch_は内側が_case_を食う() {
    // 内側が case 3 まで取る。囲めば取られない
    all(
        "switch (x) case 1 => ( switch (y) case 2 => 10 ) case 3 => 20",
        "(switch (paren x) [1 => (paren (switch (paren y) [2 => 10]))] [3 => 20])",
    );
}

#[test]
fn ループ() {
    all("loop { };", "(; (loop (block)))");
    all("while (m) { m /= 10; }", "(while (paren m) (block (; (/= m 10))))");
    all("nfor (i, 0, 10) { };", "(; (nfor i 0 10 (block)))");
}

// ---- 脱出 ----

#[test]
fn 脱出の形() {
    all("continue continue;", "(; (continue (continue)))");
    all("break continue;", "(; (break (continue)))");
    all("break outward break break;", "(; (break-outward (break (break))))");
    all("$return i;", "(; ($return i))");
    all("$repeat(break, n);", "(; ($repeat((break) n)))");
    bad("continue 1;");   // continue は値を取れない
    bad("break outward;"); // outward は脱出を要求する
}

// ---- 宣言 ----

#[test]
fn 宣言と代入は束縛種の有無で分かれる() {
    all("var x := 5; var y := 6;", "(; (var [x := 5])) (; (var [y := 6]))");
    all("x := 5;", "(; (:= x 5))");
    all("var b &= a;", "(; (var [b &= a]))");
    bad("var e &= a[0];"); // &= の右辺は名前だけ
}

#[test]
fn 関数() {
    all("fn f () { } ;", "(; (fn f (block)))");
    all("fn f () { 1 } -> i64;", "(; (fn f (block 1) -> i64))");
    bad("fn f (a) { } ;"); // 引数の型注釈は省けない
}

#[test]
fn 型は逆ポーランド() {
    all("var a : i64 array := x;", "(; (var [a := x]))");
    all("fn f () { } -> str i64 map;", "(; (fn f (block) -> str i64 map))");
    all("fn f () { } -> str i64 array map;", "(; (fn f (block) -> str i64 array map))");
    bad("fn f () { } -> i64 array map;"); // map は 2 つ取るが 1 つしか無い
}

#[test]
fn 構築() {
    all("new Point ( x := 1, y := 2 );", "(; (new Point [x 1] [y 2]))");
    all("new i64 array ( 10, 0 );", "(; (new i64 array 10 0))");
    all("new str i64 map ( );", "(; (new str i64 map))");
}

#[test]
fn 写像リテラルと領域を先読みで分ける() {
    all(r#"( "a" => 1, "b" => 2 );"#, r#"(; (map ["a" 1] ["b" 2]))"#);
    all("( a; b );", "(; (paren (; a) b))");
}

#[test]
fn 構造体と作用素式() {
    all("struct Point { var x : i64 := 0; };", "(; (struct Point))");
    all(
        "flow $return = $repeat(break, getdepth());",
        "(; (flow $return ($repeat((break) (call getdepth)))))",
    );
}

// ===== C-97：真偽と後置の型注釈 =====

#[test]
fn 真偽のリテラル() {
    t("true", "true");
    t("! false", "(! false)");
}

#[test]
fn 後置の型注釈は最も強く結合する() {
    // **`->` は領域に付く**（C-30）。後置なので算術より先に取る
    t("200 -> u8 + 100", "(+ (-> 200 u8) 100)");
    t("1 -> u1", "(-> 1 u1)");
}

#[test]
fn boolはu1の別綴り() {
    // **包み型ではない。** 型としては同じものになる
    t("1 -> bool", "(-> 1 u1)");
}
