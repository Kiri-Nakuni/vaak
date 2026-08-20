//! STEEL vaak — LLVM IR を吐く。**走らせるのではなく、翻訳する。**
//!
//! # 何が違うか
//!
//! 木を辿る実装（参照実装）と VM は、**実行しながら意味を決める。**
//! STEEL は**書き出す前に全部決めなければならない。**
//! だからこの実装は、意味論のうち「静的に決まっている部分」を洗い出す装置でもある。
//!
//! # 値の表し方
//!
//! **すべての型は paradox との直和である。** だから値は二つ組で運ぶ。
//!
//! ```text
//! (i1 ok, iN v)     ok = 0 なら paradox。v は不定
//! ```
//!
//! **アーカーシャは値ではない**（C-14）ので、組すら作らない——
//! この実装では `None` である。**領域を占めない**とはそういうことである。
//!
//! LLVM の `mem2reg` と `sroa` が二つ組を潰すので、
//! **paradox を運ぶ費用は、paradox が実際に起こりうる場所にしか残らない。**
//!
//! # 領域・スコープ・脱出段
//!
//! 三つは別の機構になる（C-20）。
//!
//! | | LLVM で何になるか |
//! |---|---|
//! | **領域** | 高々一つの値。**式の連なりの中で値を置くのは一つだけ**（C-14） |
//! | **スコープ** | `alloca`。`mem2reg` がレジスタに上げる |
//! | **脱出段** | **基本ブロックの組**——出口・（ループなら）再開点・積み荷の置き場 |
//!
//! **段が入れ子の基本ブロックになるので、`break` は `br` 一つになる。**
//! 段数が静的に分かっていれば、探索は要らない。
//!
//! # いま扱う範囲
//!
//! 整数・制御構造・関数まで。**扱えないものは書き出さずに断る**——
//! 黙って違う意味のコードを吐くよりよい。

use crate::ast::*;
use crate::span::Span;
use std::collections::HashMap;
use std::fmt::Write;

pub struct SteelError {
    pub msg: String,
    pub span: Span,
}

fn err<T>(msg: impl Into<String>, span: Span) -> Result<T, SteelError> {
    Err(SteelError { msg: msg.into(), span })
}

type R<T> = Result<T, SteelError>;

/// 一つの値。**paradox かもしれない。**
#[derive(Clone)]
struct Val {
    /// `i1`。0 なら paradox
    ok: String,
    /// `iN`
    v: String,
    ty: ValueType,
}

/// 領域の結果。**アーカーシャは値ではないので `None`。**
type Region = Option<Val>;

/// 脱出段。**基本ブロックの組である。**
struct Stage {
    /// 段を出た先
    exit: String,
    /// 積み荷の置き場（`alloca`）
    slot: String,
    ok_slot: String,
    ty: ValueType,
    /// ループなら再開点。**`continue` が行く先**
    cont: Option<String>,
    /// ループの反復回数の置き場（C-3：ループは回数を産む）
    count_slot: Option<String>,
}

struct Scope {
    names: HashMap<String, (String, ValueType)>,
}

pub struct Steel {
    head: String,
    body: String,
    n: u32,
    label_n: u32,
    scopes: Vec<Scope>,
    stages: Vec<Stage>,
    fns: HashMap<String, FnDecl>,
    /// いまの基本ブロックが終端済みか。**終端の後に命令は置けない**
    done: bool,
    /// 文字列定数
    strings: Vec<(String, Vec<u8>)>,
}

/// 型の幅。**混ぜられないので、幅は左の被演算子が決める**（実装は `wrap_like` と同じ）。
fn width(t: &ValueType) -> Option<u32> {
    Some(match t {
        ValueType::U1 => 1,
        ValueType::U8 => 8,
        ValueType::U16 => 16,
        ValueType::U32 | ValueType::I32 => 32,
        ValueType::I64 => 64,
        _ => return None,
    })
}

fn signed(t: &ValueType) -> bool {
    matches!(t, ValueType::I32 | ValueType::I64)
}

fn ity(t: &ValueType) -> String {
    format!("i{}", width(t).unwrap_or(64))
}

impl Steel {
    pub fn new() -> Self {
        Self {
            head: String::new(),
            body: String::new(),
            n: 0,
            label_n: 0,
            scopes: Vec::new(),
            stages: Vec::new(),
            fns: HashMap::new(),
            done: false,
            strings: Vec::new(),
        }
    }

    fn tmp(&mut self) -> String {
        self.n += 1;
        format!("%t{}", self.n)
    }

    fn label(&mut self, what: &str) -> String {
        self.label_n += 1;
        format!("{what}.{}", self.label_n)
    }

    fn emit(&mut self, s: &str) {
        if self.done {
            // **終端の後は届かない。** 新しいブロックを開いて続ける
            let l = self.label("dead");
            let _ = writeln!(self.body, "{l}:");
            self.done = false;
        }
        let _ = writeln!(self.body, "  {s}");
    }

    fn br(&mut self, l: &str) {
        if !self.done {
            let _ = writeln!(self.body, "  br label %{l}");
            self.done = true;
        }
    }

    fn cbr(&mut self, c: &str, a: &str, b: &str) {
        if !self.done {
            let _ = writeln!(self.body, "  br i1 {c}, label %{a}, label %{b}");
            self.done = true;
        }
    }

    fn place(&mut self, l: &str) {
        if !self.done {
            let _ = writeln!(self.body, "  br label %{l}");
        }
        let _ = writeln!(self.body, "{l}:");
        self.done = false;
    }

    /// `alloca` は入口のブロックに置く——**`mem2reg` が上げられるように。**
    fn alloca(&mut self, ty: &str) -> String {
        self.n += 1;
        let name = format!("%a{}", self.n);
        let _ = writeln!(self.head, "  {name} = alloca {ty}");
        name
    }

    // ========== 名前 ==========

    fn push_scope(&mut self) {
        self.scopes.push(Scope { names: HashMap::new() });
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &str, ty: ValueType) -> String {
        let p = self.alloca(&ity(&ty));
        self.scopes.last_mut().unwrap().names.insert(name.to_string(), (p.clone(), ty));
        p
    }

    fn lookup(&self, name: &str) -> Option<(String, ValueType)> {
        for s in self.scopes.iter().rev() {
            if let Some(x) = s.names.get(name) {
                return Some(x.clone());
            }
        }
        None
    }
}
