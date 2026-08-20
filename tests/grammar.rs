//! 文法と字句器が食い違っていないことを確かめる。
//!
//! **色分けの文法は別に書かざるを得なかった**——Zed は LSP の意味トークンで
//! 色を付けないので、tree-sitter が要る。
//!
//! だが**鍵語の一覧が二箇所にあること**は認めない。ここで突き合わせる。
//! 片方に足してもう片方に足し忘れたら、この試験が落ちる。

use std::collections::BTreeSet;

/// `grammar.js` の `choice(...)` から文字列リテラルを拾う。
fn choices(src: &str, rule: &str) -> BTreeSet<String> {
    let head = format!("{rule}: $ => choice(");
    let i = src.find(&head).unwrap_or_else(|| panic!("`{rule}` が文法に無い"));
    let rest = &src[i + head.len()..];
    let end = rest.find("),").expect("閉じ括弧が無い");
    let mut out = BTreeSet::new();
    let mut chars = rest[..end].chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            let mut w = String::new();
            for c in chars.by_ref() {
                if c == '\'' {
                    break;
                }
                w.push(c);
            }
            out.insert(w);
        }
    }
    out
}

/// `lexer.rs` の鍵語表から拾う。
fn lexer_keywords() -> BTreeSet<String> {
    let src = std::fs::read_to_string("src/lexer.rs").unwrap();
    let mut out = BTreeSet::new();
    for line in src.lines() {
        let t = line.trim();
        // `"var" => Tok::Var,` の形
        if let Some(rest) = t.strip_prefix('"') {
            if let Some(j) = rest.find('"') {
                if rest[j..].contains("=> Tok::") {
                    out.insert(rest[..j].to_string());
                }
            }
        }
    }
    // `mod` は `mod=` があるので表の外で分岐している。**鍵語ではある**
    if src.contains(r#""mod" => {"#) {
        out.insert("mod".to_string());
    }
    assert!(!out.is_empty(), "鍵語表を読み取れなかった");
    out
}

#[test]
fn 鍵語が一致する() {
    let g = std::fs::read_to_string("editors/tree-sitter-vaak/grammar.js").unwrap();
    let mut from_grammar = choices(&g, "keyword");
    from_grammar.extend(choices(&g, "boolean"));

    let from_lexer = lexer_keywords();

    let missing: Vec<_> = from_lexer.difference(&from_grammar).collect();
    let extra: Vec<_> = from_grammar.difference(&from_lexer).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "文法に足りない: {missing:?}\n文法に余計: {extra:?}"
    );
}

#[test]
fn 組み込みの型が一致する() {
    // 字句器は組み込みの型を鍵語にしていない（識別子である）ので、
    // **構文解析器の型表**と突き合わせる
    let g = std::fs::read_to_string("editors/tree-sitter-vaak/grammar.js").unwrap();
    let from_grammar = choices(&g, "type_name");

    let p = std::fs::read_to_string("src/parser.rs").unwrap();
    let mut from_parser = BTreeSet::new();
    for line in p.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('"') {
            if let Some(j) = rest.find('"') {
                if rest[j..].contains("=> ValueType::") {
                    from_parser.insert(rest[..j].to_string());
                }
            }
        }
    }
    assert_eq!(from_grammar, from_parser, "組み込みの型が食い違っている");
}
