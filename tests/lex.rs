use vaak::lexer::{lex, Tok};

fn toks(src: &str) -> Vec<Tok> {
    let mut v: Vec<Tok> = lex(src).expect("字句解析に失敗").into_iter().map(|t| t.tok).collect();
    assert_eq!(v.pop(), Some(Tok::Eof));
    v
}

#[test]
fn 空白と改行に意味は無い() {
    assert_eq!(toks("a\n\tb  c"), toks("a b c"));
}

#[test]
fn 行コメントは消える() {
    assert_eq!(toks("a % これは消える\nb"), toks("a b"));
}

#[test]
fn ブロックコメントは入れ子にできる() {
    assert_eq!(toks("a %{ 外 %{ 内 }% 外 }% b"), toks("a b"));
}

#[test]
fn ブロックコメントの中身は何でもよい() {
    assert_eq!(toks(r#"a %{ " ; := 不対応な " }% b"#), toks("a b"));
}

#[test]
fn 閉じないブロックコメントはエラー() {
    assert!(lex("%{ 開きっぱなし").is_err());
}

#[test]
fn 識別子は_unicode() {
    assert_eq!(toks("あ_い1"), vec![Tok::Ident("あ_い1".into())]);
}

#[test]
fn flowname_は_ドル_を含む() {
    assert_eq!(toks("$return"), vec![Tok::FlowName("$return".into())]);
}

#[test]
fn ドルの後に名前が無ければエラー() {
    assert!(lex("$ 1").is_err());
}

#[test]
fn 数値はソースの表現のまま保持する() {
    assert_eq!(toks("1_000"), vec![Tok::Int("1_000".into())]);
    assert_eq!(toks("0xFF"), vec![Tok::Int("0xFF".into())]);
    assert_eq!(toks("0b1010"), vec![Tok::Int("0b1010".into())]);
    assert_eq!(toks("0o17"), vec![Tok::Int("0o17".into())]);
    assert_eq!(toks("1.5"), vec![Tok::Float("1.5".into())]);
    assert_eq!(toks("1e308"), vec![Tok::Float("1e308".into())]);
    assert_eq!(toks("1.5e-3"), vec![Tok::Float("1.5e-3".into())]);
}

#[test]
fn 型接尾辞は無いので識別子になる() {
    assert_eq!(toks("1i64"), vec![Tok::Int("1".into()), Tok::Ident("i64".into())]);
}

#[test]
fn 小数点とメンバアクセスを取り違えない() {
    assert_eq!(toks("a.b"), vec![Tok::Ident("a".into()), Tok::Dot, Tok::Ident("b".into())]);
    assert_eq!(toks("1.len"), vec![Tok::Int("1".into()), Tok::Dot, Tok::Ident("len".into())]);
}

#[test]
fn 文字列のエスケープ() {
    assert_eq!(toks(r#""a\nb""#), vec![Tok::Str("a\nb".into())]);
    assert_eq!(toks(r#""\x41""#), vec![Tok::Str("A".into())]);
    assert_eq!(toks(r#""\u{3042}""#), vec![Tok::Str("あ".into())]);
}

#[test]
fn unicode_escape_は_u32_を越えても折り返さない() {
    assert!(lex(r#""\u{100000000}""#).is_err());
    assert!(lex(r#""\u{ffffffffffffffff}""#).is_err());
    assert_eq!(toks(r#""\u{10ffff}""#), vec![Tok::Str("\u{10ffff}".into())]);
}

#[test]
fn 閉じない文字列はエラー() {
    assert!(lex(r#""ここで終わる"#).is_err());
}

#[test]
fn mod_は鍵語だが_mod_イコールは複合代入() {
    assert_eq!(toks("a mod b"), vec![Tok::Ident("a".into()), Tok::Mod, Tok::Ident("b".into())]);
    assert_eq!(toks("a mod= b"), vec![Tok::Ident("a".into()), Tok::ModEq, Tok::Ident("b".into())]);
    assert_eq!(toks("a mod == b")[1], Tok::Mod);
}

#[test]
fn 演算子の最長一致() {
    assert_eq!(toks("<<="), vec![Tok::ShlEq]);
    assert_eq!(toks("<<"), vec![Tok::Shl]);
    assert_eq!(toks("<="), vec![Tok::Le]);
    assert_eq!(toks("<"), vec![Tok::Lt]);
    assert_eq!(toks("&&"), vec![Tok::AndAnd]);
    assert_eq!(toks("&="), vec![Tok::AliasBind]);
    assert_eq!(toks("&"), vec![Tok::Amp]);
    assert_eq!(toks("||"), vec![Tok::OrOr]);
    assert_eq!(toks("|>"), vec![Tok::Feed]);
    assert_eq!(toks("|="), vec![Tok::PipeEq]);
    assert_eq!(toks("|"), vec![Tok::Pipe]);
    assert_eq!(toks(":="), vec![Tok::Assign]);
    assert_eq!(toks(":"), vec![Tok::Colon]);
    assert_eq!(toks("=>"), vec![Tok::FatArrow]);
    assert_eq!(toks("->"), vec![Tok::Arrow]);
    assert_eq!(toks("="), vec![Tok::Define]);
    assert_eq!(toks("??"), vec![Tok::Coalesce]);
}

#[test]
fn 鍵語() {
    assert_eq!(toks("var let const fn flow struct new"),
        vec![Tok::Var, Tok::Let, Tok::Const, Tok::Fn, Tok::Flow, Tok::Struct, Tok::New]);
    assert_eq!(toks("if elif else fi"), vec![Tok::If, Tok::Elif, Tok::Else, Tok::Fi]);
    assert_eq!(toks("loop while nfor switch case"),
        vec![Tok::Loop, Tok::While, Tok::Nfor, Tok::Switch, Tok::Case]);
    assert_eq!(toks("break continue outward"), vec![Tok::Break, Tok::Continue, Tok::Outward]);
    assert_eq!(toks("array map alias"), vec![Tok::Array, Tok::Map, Tok::Alias]);
}

#[test]
fn セミコロンの連続() {
    assert_eq!(toks("1;;"), vec![Tok::Int("1".into()), Tok::Semi, Tok::Semi]);
}
