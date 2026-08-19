//! 字句解析。
//!
//! 全体を先にトークン化するが、**構文解析はしない**。設計書 §4.3 が要求するのは
//! 「通過していない区間はパースされていない」であって、トークン化されていないこと
//! ではない（TeX も文字→トークンの変換は先行して行う）。マクロ層を載せる際は
//! catcode 相当の再トークン化が必要になるが、Tier 0 では素直に一度で読む。

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Ident(String),

    Assign,    // :=
    AliasBind, // &=
    CloneBind, // ^=
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,

    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,

    Plus,
    Minus,
    Star,
    Slash,

    AndAnd,
    OrOr,
    Bang,

    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,

    Semi,
    Comma,
    Define, // = 定義。thunk を作らない束縛（D-42）。代入は `:=`、比較は `==`
    FatArrow, // => switch の腕
    Colon,  // 束縛子への型注釈（D-8 で呼出側アノテーションは `->` に移した）
    Dollar, // 作用素式の印（D-38）。値の層と分ける
    Arrow, // ->
    Hash,

    Eof,
}

impl Tok {
    pub fn is_kw(&self, k: &str) -> bool {
        matches!(self, Tok::Ident(s) if s == k)
    }
}

pub struct Lexed {
    pub toks: Vec<Tok>,
    /// 物理行。エラー位置に使う。
    pub lines: Vec<u32>,
    /// 論理行。改行の意味（アトリビュートと返り値型の終端）に使う。
    /// コメントが飲んだ改行では進まない。
    pub logical: Vec<u32>,
}

pub fn lex(src: &str) -> Result<Lexed, String> {
    let b: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    let mut line = 1u32;
    let mut logical_line = 1u32;
    let mut toks = Vec::new();
    let mut lines = Vec::new();
    let mut logical = Vec::new();

    macro_rules! push {
        ($t:expr, $n:expr) => {{
            toks.push($t);
            lines.push(line);
            logical.push(logical_line);
            i += $n;
        }};
    }

    while i < b.len() {
        let c = b[i];

        if c == '\n' {
            line += 1;
            logical_line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // D-7: `%` はコメント専用。剰余は `mod`。
        // D-66: `%` から改行までを**閉区間で**（改行を含めて）飲む。TeX と同一機構。
        // 物理行は進むが**論理行は進まない**ので、行末 `%` が行継続になり、
        // アトリビュートの後にコメントを置けないことが導出される。
        if c == '%' {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            if i < b.len() {
                i += 1; // 改行も飲む
                line += 1;
            }
            continue;
        }

        if c.is_ascii_digit() {
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let s: String = b[start..i].iter().collect();
            let v: i64 = s.parse().map_err(|_| format!("{}行: 整数が大きすぎます: {}", line, s))?;
            toks.push(Tok::Int(v));
            lines.push(line);
            logical.push(logical_line);
            continue;
        }

        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            let s: String = b[start..i].iter().collect();
            toks.push(Tok::Ident(s));
            lines.push(line);
            logical.push(logical_line);
            continue;
        }

        let two = if i + 1 < b.len() {
            Some((b[i], b[i + 1]))
        } else {
            None
        };
        match two {
            Some((':', '=')) => push!(Tok::Assign, 2),
            Some(('&', '=')) => push!(Tok::AliasBind, 2),
            Some(('^', '=')) => push!(Tok::CloneBind, 2),
            Some(('+', '=')) => push!(Tok::PlusEq, 2),
            Some(('-', '=')) => push!(Tok::MinusEq, 2),
            Some(('*', '=')) => push!(Tok::StarEq, 2),
            Some(('/', '=')) => push!(Tok::SlashEq, 2),
            Some(('=', '=')) => push!(Tok::Eq, 2),
            Some(('!', '=')) => push!(Tok::Ne, 2),
            Some(('<', '=')) => push!(Tok::Le, 2),
            Some(('>', '=')) => push!(Tok::Ge, 2),
            Some(('&', '&')) => push!(Tok::AndAnd, 2),
            Some(('|', '|')) => push!(Tok::OrOr, 2),
            Some(('-', '>')) => push!(Tok::Arrow, 2),
            Some(('=', '>')) => push!(Tok::FatArrow, 2),
            _ => match c {
                '+' => push!(Tok::Plus, 1),
                '-' => push!(Tok::Minus, 1),
                '*' => push!(Tok::Star, 1),
                '/' => push!(Tok::Slash, 1),
                '=' => push!(Tok::Define, 1),
                '<' => push!(Tok::Lt, 1),
                '>' => push!(Tok::Gt, 1),
                '!' => push!(Tok::Bang, 1),
                '(' => push!(Tok::LParen, 1),
                ')' => push!(Tok::RParen, 1),
                '{' => push!(Tok::LBrace, 1),
                '}' => push!(Tok::RBrace, 1),
                '[' => push!(Tok::LBracket, 1),
                ']' => push!(Tok::RBracket, 1),
                ';' => push!(Tok::Semi, 1),
                ',' => push!(Tok::Comma, 1),
                ':' => push!(Tok::Colon, 1),
                '$' => push!(Tok::Dollar, 1),
                '#' => push!(Tok::Hash, 1),
                _ => return Err(format!("{}行: 認識できない文字: {:?}", line, c)),
            },
        }
    }

    toks.push(Tok::Eof);
    lines.push(line);
    logical.push(logical_line);
    Ok(Lexed { toks, lines, logical })
}
