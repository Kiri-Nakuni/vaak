//! 字句解析。
//!
//! - コメントは**字句解析より前に消える**（行 `%`、ブロック `%{ }%`、入れ子可）
//! - **改行に意味は無い**。空白はすべて区切りにすぎない
//! - 数値リテラルは**ソースの表現のまま保持する**（型が決まるまで解釈しない）

use crate::span::Span;

#[derive(Clone, PartialEq, Debug)]
pub enum Tok {
    // --- 名前とリテラル ---
    Ident(String),
    /// `$` で始まる作用素式の名前。`$` は名前の一部（C-60）。
    FlowName(String),
    /// ソースの表現のまま。型が決まるまで解釈しない。
    Int(String),
    Float(String),
    Str(String),

    // --- 鍵語 ---
    Var,
    Let,
    Const,
    Fn,
    Flow,
    Struct,
    Wrap,
    New,
    If,
    Elif,
    Else,
    Fi,
    Loop,
    While,
    Nfor,
    Switch,
    Case,
    Break,
    Continue,
    Outward,
    Mod,
    /// **真偽の literal。** `u1` の 1 と 0（C-97）
    True,
    False,
    Array,
    Map,
    Hash,
    Alias,

    // --- 括弧と区切り ---
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Semi,
    Arrow,    // ->
    FatArrow, // =>

    // --- 演算子 ---
    Plus,
    Minus,
    Star,
    Slash,
    Bang,
    Coalesce, // ??
    Shl,
    Shr,
    Amp,
    Caret,
    Pipe,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    Ne,
    AndAnd,
    OrOr,
    Feed, // |>
    Dot,

    // --- 束縛と代入 ---
    Assign,      // :=
    AliasBind,   // &=
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    ModEq,
    ShlEq,
    ShrEq,
    CaretEq,
    PipeEq,

    /// `flow` の定義に使う。束縛演算子ではない（C-15）。
    Define, // =

    Eof,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

#[derive(Clone, PartialEq, Debug)]
pub struct LexError {
    pub msg: String,
    pub span: Span,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

pub fn lex(src: &str) -> Result<Vec<Token>, LexError> {
    Lexer::new(src).run()
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    out: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, bytes: src.as_bytes(), pos: 0, out: Vec::new() }
    }

    fn err<T>(&self, msg: impl Into<String>, start: usize) -> Result<T, LexError> {
        Err(LexError { msg: msg.into(), span: Span::new(start as u32, self.pos as u32) })
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<u8> {
        self.bytes.get(self.pos + n).copied()
    }

    /// 現在位置の文字（Unicode）。識別子の判定に使う。
    fn peek_char(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn push(&mut self, tok: Tok, start: usize) {
        self.out.push(Token { tok, span: Span::new(start as u32, self.pos as u32) });
    }

    fn run(mut self) -> Result<Vec<Token>, LexError> {
        loop {
            self.skip_trivia()?;
            let start = self.pos;
            let Some(c) = self.peek() else {
                self.push(Tok::Eof, start);
                return Ok(self.out);
            };
            match c {
                b'0'..=b'9' => self.number(start)?,
                b'"' => self.string(start)?,
                b'$' => self.flowname(start)?,
                _ if is_ident_start(self.peek_char().unwrap()) => self.word(start),
                _ => self.operator(start)?,
            }
        }
    }

    /// 空白とコメントを飛ばす。**どちらも字句解析より前に消える。**
    fn skip_trivia(&mut self) -> Result<(), LexError> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => self.pos += 1,
                Some(b'%') => {
                    let start = self.pos;
                    if self.peek_at(1) == Some(b'{') {
                        self.block_comment(start)?;
                    } else {
                        while let Some(c) = self.peek() {
                            if c == b'\n' {
                                break;
                            }
                            self.pos += 1;
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    /// `%{ }%` は入れ子にできる。
    fn block_comment(&mut self, start: usize) -> Result<(), LexError> {
        let mut depth = 0usize;
        loop {
            match (self.peek(), self.peek_at(1)) {
                (Some(b'%'), Some(b'{')) => {
                    depth += 1;
                    self.pos += 2;
                }
                (Some(b'}'), Some(b'%')) => {
                    self.pos += 2;
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                (Some(_), _) => {
                    // 多バイト文字を割らないように文字単位で進む
                    let ch = self.peek_char().unwrap();
                    self.pos += ch.len_utf8();
                }
                (None, _) => return self.err("ブロックコメントが閉じていない", start),
            }
        }
    }

    fn word(&mut self, start: usize) {
        while let Some(ch) = self.peek_char() {
            if is_ident_continue(ch) {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        let text = &self.src[start..self.pos];
        let tok = match text {
            "var" => Tok::Var,
            "let" => Tok::Let,
            "const" => Tok::Const,
            "fn" => Tok::Fn,
            "flow" => Tok::Flow,
            "struct" => Tok::Struct,
            "wrap" => Tok::Wrap,
            "new" => Tok::New,
            "if" => Tok::If,
            "elif" => Tok::Elif,
            "else" => Tok::Else,
            "fi" => Tok::Fi,
            "loop" => Tok::Loop,
            "while" => Tok::While,
            "nfor" => Tok::Nfor,
            "switch" => Tok::Switch,
            "case" => Tok::Case,
            "break" => Tok::Break,
            "continue" => Tok::Continue,
            "outward" => Tok::Outward,
            "array" => Tok::Array,
            "map" => Tok::Map,
            "hash" => Tok::Hash,
            "alias" => Tok::Alias,
            // **真偽の literal。`u1` である**（C-97）
            "true" => Tok::True,
            "false" => Tok::False,
            // `mod` だけは直後の `=` を取り込む（`mod=` は複合代入）
            "mod" => {
                if self.peek() == Some(b'=') && self.peek_at(1) != Some(b'=') {
                    self.pos += 1;
                    Tok::ModEq
                } else {
                    Tok::Mod
                }
            }
            _ => Tok::Ident(text.to_string()),
        };
        self.push(tok, start);
    }

    /// `$` は名前の一部。これで文法が完全に文脈自由になる（C-60）。
    fn flowname(&mut self, start: usize) -> Result<(), LexError> {
        self.pos += 1; // $
        let name_start = self.pos;
        match self.peek_char() {
            Some(ch) if is_ident_start(ch) => {}
            _ => return self.err("`$` の後に名前が要る", start),
        }
        while let Some(ch) = self.peek_char() {
            if is_ident_continue(ch) {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        let _ = name_start;
        let text = self.src[start..self.pos].to_string();
        self.push(Tok::FlowName(text), start);
        Ok(())
    }

    /// 数値。**ソースの表現のまま保持する。型接尾辞は無い。**
    fn number(&mut self, start: usize) -> Result<(), LexError> {
        // 基数付き
        if self.peek() == Some(b'0') {
            if let Some(b) = self.peek_at(1) {
                let radix = match b {
                    b'x' | b'X' => Some(16),
                    b'b' | b'B' => Some(2),
                    b'o' | b'O' => Some(8),
                    _ => None,
                };
                if let Some(radix) = radix {
                    self.pos += 2;
                    let digits_start = self.pos;
                    while let Some(c) = self.peek() {
                        if c == b'_' || (c as char).is_digit(radix) {
                            self.pos += 1;
                        } else {
                            break;
                        }
                    }
                    if self.pos == digits_start {
                        return self.err("基数の後に数字が要る", start);
                    }
                    let text = self.src[start..self.pos].to_string();
                    self.push(Tok::Int(text), start);
                    return Ok(());
                }
            }
        }

        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == b'_') {
            self.pos += 1;
        }

        let mut is_float = false;
        // `1.5` の小数点。`a.b` の `.` と衝突しないよう、直後が数字のときだけ取る
        if self.peek() == Some(b'.') && matches!(self.peek_at(1), Some(c) if c.is_ascii_digit()) {
            is_float = true;
            self.pos += 1;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == b'_') {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let save = self.pos;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                is_float = true;
                while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == b'_') {
                    self.pos += 1;
                }
            } else {
                self.pos = save;
            }
        }

        let text = self.src[start..self.pos].to_string();
        self.push(if is_float { Tok::Float(text) } else { Tok::Int(text) }, start);
        Ok(())
    }

    fn string(&mut self, start: usize) -> Result<(), LexError> {
        self.pos += 1; // "
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return self.err("文字列が閉じていない", start);
            };
            match c {
                b'"' => {
                    self.pos += 1;
                    self.push(Tok::Str(out), start);
                    return Ok(());
                }
                b'\\' => {
                    self.pos += 1;
                    let Some(e) = self.peek() else {
                        return self.err("文字列が閉じていない", start);
                    };
                    self.pos += 1;
                    match e {
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        b'\\' => out.push('\\'),
                        b'"' => out.push('"'),
                        b'0' => out.push('\0'),
                        b'x' => {
                            let h = self.take_hex(2, start)?;
                            out.push(h as u8 as char);
                        }
                        b'u' => {
                            if self.peek() != Some(b'{') {
                                return self.err("`\\u` の後に `{` が要る", start);
                            }
                            self.pos += 1;
                            let mut v: u32 = 0;
                            let mut n = 0;
                            while let Some(c) = self.peek() {
                                if c == b'}' {
                                    break;
                                }
                                let Some(d) = (c as char).to_digit(16) else {
                                    return self.err("16進数ではない", start);
                                };
                                v = v * 16 + d;
                                n += 1;
                                self.pos += 1;
                            }
                            if n == 0 || self.peek() != Some(b'}') {
                                return self.err("`\\u{...}` が閉じていない", start);
                            }
                            self.pos += 1;
                            let Some(ch) = char::from_u32(v) else {
                                return self.err("符号位置ではない", start);
                            };
                            out.push(ch);
                        }
                        _ => return self.err("知らないエスケープ", start),
                    }
                }
                _ => {
                    let ch = self.peek_char().unwrap();
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    fn take_hex(&mut self, n: usize, start: usize) -> Result<u32, LexError> {
        let mut v = 0u32;
        for _ in 0..n {
            let Some(c) = self.peek() else {
                return self.err("16進数が足りない", start);
            };
            let Some(d) = (c as char).to_digit(16) else {
                return self.err("16進数ではない", start);
            };
            v = v * 16 + d;
            self.pos += 1;
        }
        Ok(v)
    }

    fn operator(&mut self, start: usize) -> Result<(), LexError> {
        let c = self.peek().unwrap();
        let n1 = self.peek_at(1);
        let n2 = self.peek_at(2);

        macro_rules! t {
            ($len:expr, $tok:expr) => {{
                self.pos += $len;
                self.push($tok, start);
                return Ok(());
            }};
        }

        match (c, n1, n2) {
            (b'?', Some(b'?'), _) => t!(2, Tok::Coalesce),
            (b'<', Some(b'<'), Some(b'=')) => t!(3, Tok::ShlEq),
            (b'>', Some(b'>'), Some(b'=')) => t!(3, Tok::ShrEq),
            (b'<', Some(b'<'), _) => t!(2, Tok::Shl),
            (b'>', Some(b'>'), _) => t!(2, Tok::Shr),
            (b'<', Some(b'='), _) => t!(2, Tok::Le),
            (b'>', Some(b'='), _) => t!(2, Tok::Ge),
            (b'=', Some(b'='), _) => t!(2, Tok::EqEq),
            (b'!', Some(b'='), _) => t!(2, Tok::Ne),
            (b'=', Some(b'>'), _) => t!(2, Tok::FatArrow),
            (b'-', Some(b'>'), _) => t!(2, Tok::Arrow),
            (b'&', Some(b'&'), _) => t!(2, Tok::AndAnd),
            (b'&', Some(b'='), _) => t!(2, Tok::AliasBind),
            (b'|', Some(b'|'), _) => t!(2, Tok::OrOr),
            (b'|', Some(b'>'), _) => t!(2, Tok::Feed),
            (b'|', Some(b'='), _) => t!(2, Tok::PipeEq),
            (b':', Some(b'='), _) => t!(2, Tok::Assign),
            (b'+', Some(b'='), _) => t!(2, Tok::PlusEq),
            (b'-', Some(b'='), _) => t!(2, Tok::MinusEq),
            (b'*', Some(b'='), _) => t!(2, Tok::StarEq),
            (b'/', Some(b'='), _) => t!(2, Tok::SlashEq),
            (b'^', Some(b'='), _) => t!(2, Tok::CaretEq),
            (b'(', _, _) => t!(1, Tok::LParen),
            (b')', _, _) => t!(1, Tok::RParen),
            (b'{', _, _) => t!(1, Tok::LBrace),
            (b'}', _, _) => t!(1, Tok::RBrace),
            (b'[', _, _) => t!(1, Tok::LBracket),
            (b']', _, _) => t!(1, Tok::RBracket),
            (b',', _, _) => t!(1, Tok::Comma),
            (b':', _, _) => t!(1, Tok::Colon),
            (b';', _, _) => t!(1, Tok::Semi),
            (b'.', _, _) => t!(1, Tok::Dot),
            (b'+', _, _) => t!(1, Tok::Plus),
            (b'-', _, _) => t!(1, Tok::Minus),
            (b'*', _, _) => t!(1, Tok::Star),
            (b'/', _, _) => t!(1, Tok::Slash),
            (b'!', _, _) => t!(1, Tok::Bang),
            (b'&', _, _) => t!(1, Tok::Amp),
            (b'^', _, _) => t!(1, Tok::Caret),
            (b'|', _, _) => t!(1, Tok::Pipe),
            (b'<', _, _) => t!(1, Tok::Lt),
            (b'>', _, _) => t!(1, Tok::Gt),
            (b'=', _, _) => t!(1, Tok::Define),
            _ => {
                let ch = self.peek_char().unwrap();
                self.pos += ch.len_utf8();
                self.err(format!("知らない文字 `{ch}`"), start)
            }
        }
    }
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}
