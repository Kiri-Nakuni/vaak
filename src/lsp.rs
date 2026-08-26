//! Vaak の言語サーバ。
//!
//! **検査器がもう答えを持っている。** `check` も `check_types` も
//! `StaticError { msg, span }` を返すので、**繋ぐだけである。**
//!
//! # 強調しておくこと
//!
//! **色分けも検査器の側から出す。** 別に文法定義を書かない——
//! `.scm` を書けば、**一つの決定を二箇所で実装する**ことになり、
//! 二箇所で間違えられる（C-61 / C-93 で二度やった誤りである）。
//!
//! だから意味トークン（semantic tokens）を字句器から直接作る。
//!
//! # 位置
//!
//! LSP の桁は既定で **UTF-16 の符号単位**である。Vaak の `Span` はバイトである。
//! **変換を一箇所に閉じる**——[`Doc`] が行頭表と換算を持つ。

use crate::json::{n, obj, s, write, J};
use crate::lexer::Tok;
use crate::span::Span;
use std::collections::HashMap;
use std::io::{BufRead, Write as _};

// ========== 文書 ==========

/// 開かれている文書。**行頭の位置を控えておく。**
pub struct Doc {
    pub text: String,
    /// 各行の先頭のバイト位置
    lines: Vec<u32>,
}

impl Doc {
    pub fn new(text: String) -> Self {
        let mut lines = vec![0u32];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                lines.push(i as u32 + 1);
            }
        }
        Self { text, lines }
    }

    /// バイト位置 → (行, UTF-16 の桁)。**どちらも 0 始まり**（LSP の流儀）。
    pub fn pos(&self, off: u32) -> (u32, u32) {
        let line = match self.lines.binary_search(&off) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let start = self.lines[line] as usize;
        let end = (off as usize).min(self.text.len());
        let col = self
            .text
            .get(start..end)
            .map(|t| t.encode_utf16().count())
            .unwrap_or(0);
        (line as u32, col as u32)
    }

    /// (行, UTF-16 の桁) → バイト位置。
    pub fn off(&self, line: u32, col: u32) -> u32 {
        let Some(&start) = self.lines.get(line as usize) else {
            return self.text.len() as u32;
        };
        let rest = &self.text[start as usize..];
        let mut u16s = 0usize;
        for (i, ch) in rest.char_indices() {
            if u16s >= col as usize {
                return start + i as u32;
            }
            if ch == '\n' {
                return start + i as u32;
            }
            u16s += ch.len_utf16();
        }
        start + rest.len() as u32
    }

    fn range(&self, sp: Span) -> J {
        let (l1, c1) = self.pos(sp.start);
        let (l2, c2) = self.pos(sp.end.max(sp.start));
        obj(vec![
            (
                "start",
                obj(vec![("line", n(l1 as i64)), ("character", n(c1 as i64))]),
            ),
            (
                "end",
                obj(vec![("line", n(l2 as i64)), ("character", n(c2 as i64))]),
            ),
        ])
    }
}

// ========== 意味トークン ==========

/// LSP に渡す種類の一覧。**番号は並び順である。**
pub const TOKEN_TYPES: &[&str] = &[
    "keyword",
    "number",
    "string",
    "comment",
    "operator",
    "function",
    "variable",
    "type",
    "macro",
    "parameter",
    "property",
];

const T_KEYWORD: u32 = 0;
const T_NUMBER: u32 = 1;
const T_STRING: u32 = 2;
const T_COMMENT: u32 = 3;
const T_OPERATOR: u32 = 4;
const T_FUNCTION: u32 = 5;
const T_VARIABLE: u32 = 6;
const T_TYPE: u32 = 7;
const T_MACRO: u32 = 8;

/// 注釈の範囲を拾う。**字句器は注釈を捨てる**ので、ここで拾い直す。
fn comments(src: &str) -> Vec<Span> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'%' if i + 1 < b.len() && b[i + 1] == b'{' => {
                let start = i;
                i += 2;
                while i + 1 < b.len() && !(b[i] == b'}' && b[i + 1] == b'%') {
                    i += 1;
                }
                i = (i + 2).min(b.len());
                out.push(Span::new(start as u32, i as u32));
            }
            b'%' => {
                let start = i;
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                out.push(Span::new(start as u32, i as u32));
            }
            _ => i += 1,
        }
    }
    out
}

fn kind_of(t: &Tok, next: Option<&Tok>, prev: Option<&Tok>) -> Option<u32> {
    use Tok::*;
    Some(match t {
        Int(_) | Float(_) => T_NUMBER,
        True | False => T_KEYWORD,
        Str(_) => T_STRING,
        FlowName(_) => T_MACRO,
        Ident(_) => {
            // **呼び出しなら関数、`:` や `new` の後なら型、それ以外は名前。**
            // 完全な解決はしない——色分けに要る精度で足りる
            if matches!(next, Some(LParen)) {
                T_FUNCTION
            } else if matches!(
                prev,
                Some(Colon) | Some(New) | Some(Arrow) | Some(Struct) | Some(Wrap)
            ) || matches!(next, Some(Array) | Some(Map) | Some(Alias))
            {
                T_TYPE
            } else {
                T_VARIABLE
            }
        }
        Var | Let | Const | Fn | Flow | Struct | Wrap | New | If | Elif | Else | Fi | Loop
        | While | Nfor | Switch | Case | Break | Continue | Outward | Alias => T_KEYWORD,
        Mod | Array | Map => T_KEYWORD,
        LParen | RParen | LBrace | RBrace | LBracket | RBracket | Comma | Semi | Colon => {
            return None
        }
        Eof => return None,
        _ => T_OPERATOR,
    })
}

/// 組み込みの型の名前。**字句器は鍵語にしていない**（識別子である）ので、ここで拾う。
fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "bool" | "u1" | "u8" | "u16" | "u32" | "i32" | "i64" | "f32" | "f64" | "f80" | "str"
    )
}

pub fn semantic_tokens(doc: &Doc) -> Vec<i64> {
    let toks = crate::lexer::lex(&doc.text).unwrap_or_default();
    let mut items: Vec<(Span, u32)> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        let next = toks.get(i + 1).map(|x| &x.tok);
        let prev = if i > 0 { Some(&toks[i - 1].tok) } else { None };
        let k = match &t.tok {
            Tok::Ident(nm) if is_builtin_type(nm) => Some(T_TYPE),
            other => kind_of(other, next, prev),
        };
        if let Some(k) = k {
            items.push((t.span, k));
        }
    }
    for c in comments(&doc.text) {
        items.push((c, T_COMMENT));
    }
    items.sort_by_key(|(sp, _)| sp.start);

    // **差分符号化。** 行・桁・長さ・種類・修飾の五つ組
    let mut out = Vec::new();
    let (mut pl, mut pc) = (0u32, 0u32);
    for (sp, k) in items {
        let (l, c) = doc.pos(sp.start);
        let (l2, c2) = doc.pos(sp.end);
        // **行をまたぐものは出さない。** LSP は一行に収まる印しか受け取らない
        let len = if l2 == l {
            c2.saturating_sub(c)
        } else {
            continue;
        };
        if len == 0 {
            continue;
        }
        let dl = l - pl;
        let dc = if dl == 0 { c - pc } else { c };
        out.extend_from_slice(&[dl as i64, dc as i64, len as i64, k as i64, 0]);
        pl = l;
        pc = c;
    }
    out
}

// ========== 診断 ==========

pub fn diagnostics(doc: &Doc) -> Vec<J> {
    let mut out = Vec::new();
    let prog = match crate::parser::parse(&doc.text) {
        Ok(p) => p,
        Err(e) => {
            out.push(diag(doc, e.span, &e.msg));
            return out;
        }
    };
    for e in crate::check::check(&prog) {
        out.push(diag(doc, e.span, &e.msg));
    }
    for e in crate::types::check_types(&prog) {
        out.push(diag(doc, e.span, &e.msg));
    }
    out
}

fn diag(doc: &Doc, sp: Span, msg: &str) -> J {
    let sp = if sp.end <= sp.start {
        Span::new(sp.start, sp.start + 1)
    } else {
        sp
    };
    obj(vec![
        ("range", doc.range(sp)),
        ("severity", n(1)),
        ("source", s("vaak")),
        ("message", s(msg)),
    ])
}

// ========== 記号一覧 ==========

pub fn symbols(doc: &Doc) -> Vec<J> {
    let Ok(prog) = crate::parser::parse(&doc.text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in &prog.body {
        collect_symbol(doc, e, &mut out);
    }
    out
}

fn collect_symbol(doc: &Doc, e: &crate::ast::Expr, out: &mut Vec<J>) {
    use crate::ast::ExprKind as E;
    // `;` は宣言を包む（C-80）ので、剥がしてから見る
    if let E::Discard(Some(inner)) = &e.kind {
        return collect_symbol(doc, inner, out);
    }
    let (name, kind, sp) = match &e.kind {
        E::FnDecl(f) => (
            match &f.owner {
                Some(o) => format!("{o}.{}", f.name),
                None => f.name.clone(),
            },
            12,
            f.span,
        ),
        E::StructDecl(d) => (d.name.clone(), 23, d.span),
        E::WrapDecl(d) => (d.name.clone(), 26, d.span),
        E::FlowDecl(d) => (d.name.clone(), 12, d.span),
        E::Decl(d) => {
            for b in &d.bindings {
                let k = if d.kind == crate::ast::BindKind::Const {
                    14
                } else {
                    13
                };
                out.push(obj(vec![
                    ("name", s(&b.name)),
                    ("kind", n(k)),
                    ("range", doc.range(b.span)),
                    ("selectionRange", doc.range(b.span)),
                ]));
            }
            return;
        }
        _ => return,
    };
    out.push(obj(vec![
        ("name", s(&name)),
        ("kind", n(kind)),
        ("range", doc.range(sp)),
        ("selectionRange", doc.range(sp)),
    ]));
}

// ========== 説明 ==========

/// 鍵語の説明。**この言語は普通でないので、説明が要る。**
fn explain(t: &Tok) -> Option<&'static str> {
    use Tok::*;
    Some(match t {
        Coalesce => "`??` — paradox の除去子。**唯一の回復手段**（C-22）。\n\n\
                     左右で束縛力が違う（左 60 / 右 31）。左は乗除の次、右は加減より下。\n\n\
                     ```\na + b / c ?? d + e   →  a + ((b / c) ?? (d + e))\n```",
        Semi => "`;` — 領域を潰し、内面を空にする。**唯一の黙殺手段**（C-22）。\n\n\
                 宣言の後に要るのは句読点だからではなく、**宣言が paradox を産むから**である。\n\n\
                 左辺は無くてもよい（C-80）。",
        Break => "段を**終結**させる。被演算子は**即時評価される**（C-73）。\n\n\
                  重ねると段数になる。`break break 5` は二段抜けて 5 を置く。\n\n\
                  フレームで止まる。越えるには `outward` と書く（C-34）。",
        Continue => "段を**再開**させる。被演算子は**即時評価されない**——\
                     再開した本体の先頭で処理される（C-73）。\n\n\
                     取れるのは**作用素式か虚無だけ**（C-71）。",
        Outward => "脱出が**フレームを越える**ことを許す。**どの段送りが越えるか**を指す（C-70）。",
        Loop => "無限ループ。**値は反復回数**（C-3）。本体は**その構文の段**であって、二重にはならない。",
        Nfor => "`nfor (名前, 開始, 回数) { … }` — 回数の決まった繰り返し。**値は反復回数**。",
        Flow => "**値でも関数でもない第三の束縛種。** 作用素式に名前を付ける。\n\n\
                 本体は**使用位置で読み直される**（C-15）——だから `getdepth()` が使用位置の深さを返す。\n\n\
                 `=` であって `:=` ではない。**セルを作らないので、書くべき値が無い。**",
        Alias => "別名で受ける。**別名は値の中に入らない**（C-48）——最も外側にのみ書ける。",
        Fi => "`if` を閉じる。**分岐は一つの式**なので、閉じる語で終端を決める。",
        Switch => "`switch (主題) case 値 => 腕 …` — 腕は**一つの式**（C-82）。\n\n\
                   どの腕にも当たらなければ paradox。`?? 既定値` で受ける（C-39）。",
        New => "`new 型 ( 引数 )` — 構造体・配列・写像・包み型を作る。**包むのも剥がすのも `new`**（S-2）。",
        Wrap => "`wrap 名前 = 型;` — 基底型を包んだ別の型を作る（S-2）。",
        Const => "凍っているセル。**書き換えられない。**",
        Var => "書き換えられるセル。",
        Let => "書き換えられないセル。**値は入っている。**",
        _ => return None,
    })
}

pub fn hover(doc: &Doc, off: u32) -> Option<J> {
    let toks = crate::lexer::lex(&doc.text).ok()?;
    let t = toks
        .iter()
        .find(|t| t.span.start <= off && off < t.span.end)?;
    let md = match &t.tok {
        Tok::Ident(name) => decl_doc(doc, name)?,
        other => explain(other)?.to_string(),
    };
    Some(obj(vec![
        (
            "contents",
            obj(vec![("kind", s("markdown")), ("value", s(&md))]),
        ),
        ("range", doc.range(t.span)),
    ]))
}

/// 名前の宣言を探して説明にする。**構文木を一度歩くだけ。**
fn decl_doc(doc: &Doc, name: &str) -> Option<String> {
    if is_builtin_type(name) {
        return Some(format!(
            "**組み込みの型** `{name}`\n\n\
            すべての整数演算は 2^N を法として折り返す。除算は**ユークリッド**（C-76）——\
            剰余は必ず非負である。"
        ));
    }
    let prog = crate::parser::parse(&doc.text).ok()?;
    let mut found = None;
    walk(&prog.body, &mut |e| {
        use crate::ast::ExprKind as E;
        match &e.kind {
            E::FnDecl(f) if f.name == name => {
                let ps: Vec<String> = f
                    .params
                    .iter()
                    .map(|p| format!("{} : {}", p.name, show_type(&p.ty)))
                    .collect();
                let ret = f
                    .ret
                    .as_ref()
                    .map(|r| format!(" -> {}", show_type(r)))
                    .unwrap_or_default();
                found = Some(format!("```vaak\nfn {name} ({}){ret}\n```", ps.join(", ")));
            }
            E::StructDecl(d) if d.name == name => {
                let fs: Vec<String> = d
                    .fields
                    .iter()
                    .map(|f| format!("    {} : {};", f.name, show_type(&f.ty)))
                    .collect();
                found = Some(format!(
                    "```vaak\nstruct {name} {{\n{}\n}}\n```",
                    fs.join("\n")
                ));
            }
            E::WrapDecl(d) if d.name == name => {
                found = Some(format!(
                    "```vaak\nwrap {name} = {};\n```",
                    show_type(&d.base)
                ));
            }
            E::Decl(d) => {
                for b in &d.bindings {
                    if b.name == name {
                        let k = match d.kind {
                            crate::ast::BindKind::Var => "var",
                            crate::ast::BindKind::Let => "let",
                            crate::ast::BindKind::Const => "const",
                        };
                        let ty =
                            b.ty.as_ref()
                                .map(|t| format!(" : {}", show_type(t)))
                                .unwrap_or_default();
                        found = Some(format!("```vaak\n{k} {name}{ty}\n```"));
                    }
                }
            }
            _ => {}
        }
    });
    found
}

fn walk(items: &[crate::ast::Expr], f: &mut impl FnMut(&crate::ast::Expr)) {
    use crate::ast::ExprKind as E;
    for e in items {
        f(e);
        match &e.kind {
            E::Discard(Some(i)) => walk(std::slice::from_ref(i), f),
            E::Block(v) | E::Paren(v) => walk(v, f),
            E::FnDecl(d) => walk(std::slice::from_ref(&d.body), f),
            _ => {}
        }
    }
}

fn show_type(t: &crate::ast::Type) -> String {
    let base = show_vt(&t.value);
    if t.is_alias {
        format!("{base} alias")
    } else {
        base
    }
}

fn show_vt(t: &crate::ast::ValueType) -> String {
    use crate::ast::ValueType as V;
    match t {
        V::U1 => "u1".into(),
        V::U8 => "u8".into(),
        V::U16 => "u16".into(),
        V::U32 => "u32".into(),
        V::I32 => "i32".into(),
        V::I64 => "i64".into(),
        V::F32 => "f32".into(),
        V::F80 => "f80".into(),
        V::F64 => "f64".into(),
        V::Str => "str".into(),
        V::Array(e) => format!("{} array", show_vt(e)),
        V::Map(k, v) => format!("{} {} map", show_vt(k), show_vt(v)),
        V::Hash(k, v) => format!("{} {} hash", show_vt(k), show_vt(v)),
        V::Named(n) => n.clone(),
    }
}

// ========== 補完 ==========

const KEYWORDS: &[&str] = &[
    "var", "let", "const", "fn", "flow", "struct", "wrap", "new", "if", "elif", "else", "fi",
    "loop", "while", "nfor", "switch", "case", "break", "continue", "outward", "mod", "array",
    "map", "alias", "true", "false", "bool", "u1", "u8", "u16", "u32", "i32", "i64", "f32", "f64",
    "str",
];

pub fn completions(doc: &Doc) -> Vec<J> {
    let mut out: Vec<J> = KEYWORDS
        .iter()
        .map(|k| obj(vec![("label", s(k)), ("kind", n(14))]))
        .collect();
    if let Ok(prog) = crate::parser::parse(&doc.text) {
        let mut seen = Vec::new();
        walk(&prog.body, &mut |e| {
            use crate::ast::ExprKind as E;
            match &e.kind {
                E::FnDecl(f) => seen.push((f.name.clone(), 3)),
                E::StructDecl(d) => seen.push((d.name.clone(), 22)),
                E::WrapDecl(d) => seen.push((d.name.clone(), 22)),
                E::FlowDecl(d) => seen.push((d.name.clone(), 3)),
                E::Decl(d) => {
                    for b in &d.bindings {
                        seen.push((b.name.clone(), 6));
                    }
                }
                _ => {}
            }
        });
        for (nm, k) in seen {
            out.push(obj(vec![("label", s(&nm)), ("kind", n(k))]));
        }
    }
    out
}

// ========== 本体 ==========

pub struct Server {
    docs: HashMap<String, Doc>,
}

impl Server {
    pub fn new() -> Self {
        Self {
            docs: HashMap::new(),
        }
    }

    pub fn run(&mut self) {
        let stdin = std::io::stdin();
        let mut r = stdin.lock();
        loop {
            let Some(body) = read_message(&mut r) else {
                return;
            };
            let Some(msg) = crate::json::parse(&body) else {
                continue;
            };
            let method = msg
                .get("method")
                .and_then(|m| m.str())
                .unwrap_or("")
                .to_string();
            let id = msg.get("id").cloned();
            if method == "exit" {
                return;
            }
            self.handle(&method, &msg, id);
        }
    }

    fn handle(&mut self, method: &str, msg: &J, id: Option<J>) {
        match method {
            "initialize" => {
                let caps = obj(vec![
                    ("textDocumentSync", n(1)),
                    ("documentSymbolProvider", J::Bool(true)),
                    ("hoverProvider", J::Bool(true)),
                    (
                        "completionProvider",
                        obj(vec![("triggerCharacters", J::Arr(vec![s("."), s("$")]))]),
                    ),
                    (
                        "semanticTokensProvider",
                        obj(vec![
                            (
                                "legend",
                                obj(vec![
                                    (
                                        "tokenTypes",
                                        J::Arr(TOKEN_TYPES.iter().map(|t| s(t)).collect()),
                                    ),
                                    ("tokenModifiers", J::Arr(vec![])),
                                ]),
                            ),
                            ("full", J::Bool(true)),
                        ]),
                    ),
                ]);
                reply(
                    id,
                    obj(vec![
                        ("capabilities", caps),
                        (
                            "serverInfo",
                            obj(vec![("name", s("vaak-lsp")), ("version", s("0.1.0"))]),
                        ),
                    ]),
                );
            }
            "shutdown" => reply(id, J::Null),
            "textDocument/didOpen" => {
                let uri = msg
                    .path(&["params", "textDocument", "uri"])
                    .and_then(|u| u.str());
                let text = msg
                    .path(&["params", "textDocument", "text"])
                    .and_then(|t| t.str());
                if let (Some(u), Some(t)) = (uri, text) {
                    self.docs.insert(u.to_string(), Doc::new(t.to_string()));
                    self.publish(u);
                }
            }
            "textDocument/didChange" => {
                let uri = msg
                    .path(&["params", "textDocument", "uri"])
                    .and_then(|u| u.str());
                let text = msg
                    .path(&["params", "contentChanges"])
                    .and_then(|c| c.at(0))
                    .and_then(|c| c.get("text"))
                    .and_then(|t| t.str());
                if let (Some(u), Some(t)) = (uri, text) {
                    self.docs.insert(u.to_string(), Doc::new(t.to_string()));
                    self.publish(u);
                }
            }
            "textDocument/didClose" => {
                if let Some(u) = msg
                    .path(&["params", "textDocument", "uri"])
                    .and_then(|u| u.str())
                {
                    self.docs.remove(u);
                }
            }
            "textDocument/documentSymbol" => {
                let d = self.doc_of(msg);
                reply(id, J::Arr(d.map(symbols).unwrap_or_default()));
            }
            "textDocument/semanticTokens/full" => {
                let data = self.doc_of(msg).map(semantic_tokens).unwrap_or_default();
                reply(
                    id,
                    obj(vec![(
                        "data",
                        J::Arr(data.into_iter().map(J::from_i64).collect()),
                    )]),
                );
            }
            "textDocument/hover" => {
                let line = msg
                    .path(&["params", "position", "line"])
                    .and_then(|x| x.int());
                let ch = msg
                    .path(&["params", "position", "character"])
                    .and_then(|x| x.int());
                let r = match (self.doc_of(msg), line, ch) {
                    (Some(d), Some(l), Some(c)) => {
                        let off = d.off(l as u32, c as u32);
                        hover(d, off).unwrap_or(J::Null)
                    }
                    _ => J::Null,
                };
                reply(id, r);
            }
            "textDocument/completion" => {
                let d = self.doc_of(msg);
                reply(id, J::Arr(d.map(completions).unwrap_or_default()));
            }
            _ => {
                // **問い合わせには必ず答える。** 黙ると相手が待ち続ける
                if id.is_some() {
                    reply(id, J::Null);
                }
            }
        }
    }

    fn doc_of(&self, msg: &J) -> Option<&Doc> {
        let u = msg.path(&["params", "textDocument", "uri"])?.str()?;
        self.docs.get(u)
    }

    fn publish(&self, uri: &str) {
        let Some(d) = self.docs.get(uri) else { return };
        notify(
            "textDocument/publishDiagnostics",
            obj(vec![
                ("uri", s(uri)),
                ("diagnostics", J::Arr(diagnostics(d))),
            ]),
        );
    }
}

impl J {
    fn from_i64(x: i64) -> J {
        J::Num(x as f64)
    }
}

fn read_message(r: &mut impl BufRead) -> Option<String> {
    let mut len = 0usize;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let t = line.trim_end();
        if t.is_empty() {
            break;
        }
        if let Some(v) = t.strip_prefix("Content-Length:") {
            len = v.trim().parse().ok()?;
        }
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

fn send(j: J) {
    let body = write(&j);
    let out = std::io::stdout();
    let mut o = out.lock();
    let _ = write!(o, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = o.flush();
}

fn reply(id: Option<J>, result: J) {
    let Some(id) = id else { return };
    send(J::Obj(vec![
        ("jsonrpc".into(), s("2.0")),
        ("id".into(), id),
        ("result".into(), result),
    ]));
}

fn notify(method: &str, params: J) {
    send(obj(vec![
        ("jsonrpc", s("2.0")),
        ("method", s(method)),
        ("params", params),
    ]));
}
