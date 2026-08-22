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
    /// 名前 → （置き場、型、**間接か**）。
    ///
    /// 間接なら置き場は**指し先を入れた枠**であって、値の枠ではない。
    /// 別名を使わない名前は真っ直ぐなままである——**使わない機能は費用を持たない**
    names: HashMap<String, (String, ValueType, bool)>,
}

pub struct Steel {
    head: String,
    body: String,
    n: u32,
    label_n: u32,
    scopes: Vec<Scope>,
    stages: Vec<Stage>,
    fns: HashMap<String, FnDecl>,
    /// 作用素式。**本体は使用位置で読み直される**（C-15）
    flows: HashMap<String, Escape>,
    /// いまの基本ブロックが終端済みか。**終端の後に命令は置けない**
    done: bool,
    /// 文字列定数
    strings: Vec<(String, Vec<u8>)>,
    /// 組み込みの宣言（`llvm.fabs` など）。**重ねて出さない**
    decls: Vec<String>,
    /// 型ごとの写す関数。**一度だけ出す**
    copy_fns: Vec<String>,
    /// **文脈が求めている型**（C-94）。写像・配列のリテラルは自分では型を決められない
    want: Option<ValueType>,
    /// 構造体の宣言。**欄の並びが位置を決める**
    structs: HashMap<String, StructDecl>,
    /// 包み型（S-2）。名前 → 包んだ型
    wraps: HashMap<String, ValueType>,
    /// 出した構造体の型。**重ねて出さない**
    struct_tys: Vec<String>,
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
    match t {
        ValueType::F32 => "float".into(),
        ValueType::F64 => "double".into(),
        // **方言が足した基底型**（S-20）。符号 1・指数 15・仮数 64 ビット
        ValueType::F80 => "x86_fp80".into(),
        // **集合体は場所を指す**
        ValueType::Array(_)
        | ValueType::Str
        | ValueType::Map(..)
        | ValueType::Hash(..)
        | ValueType::Named(_) => "ptr".into(),
        _ => format!("i{}", width(t).unwrap_or(64)),
    }
}

fn is_float(t: &ValueType) -> bool {
    matches!(t, ValueType::F32 | ValueType::F64 | ValueType::F80)
}

/// **場に置かれるもの。** 値そのものではなく、場所を指す。
fn is_heap(t: &ValueType) -> bool {
    // **`Named` はここに来る時点で構造体である**——包みは `resolve` で剥がしてある
    matches!(
        t,
        ValueType::Array(_)
            | ValueType::Str
            | ValueType::Map(..)
            | ValueType::Hash(..)
            | ValueType::Named(_)
    )
}

/// **鍵の値で飛ぶ連想**（C-98）。写像とは頭の形も並びの約束も違う
fn is_hash(t: &ValueType) -> bool {
    matches!(t, ValueType::Hash(..))
}

/// **写像だけは頭の形が違う。** 個数・容量・鍵の並び・値の並びの四つを持つ
fn is_map(t: &ValueType) -> bool {
    matches!(t, ValueType::Map(..))
}

/// 集合体の要素の型。`str` は `u8 array` を包んだもの（C-77）。
fn elem_of(t: &ValueType) -> Option<ValueType> {
    match t {
        ValueType::Array(e) => Some((**e).clone()),
        ValueType::Str => Some(ValueType::U8),
        _ => None,
    }
}

/// 要素一つの大きさ（バイト）。
fn elem_size(t: &ValueType) -> u64 {
    if is_heap(t) {
        return 8;
    }
    match t {
        // **`x86_fp80` は 10 バイトだが、置き場は 16 バイトである**（x86-64 の揃え）。
        // `getelementptr` が使うのは置き場の大きさなので、そちらに合わせる
        ValueType::F80 => 16,
        ValueType::F64 => 8,
        ValueType::F32 => 4,
        _ => match width(t) {
            Some(1) => 1,
            Some(w) => (w as u64) / 8,
            None => 8,
        },
    }
}

/// LLVM の浮動小数リテラル。**十進では丸めが入る**ので、ビット列で書く。
///
/// `float` も**倍精度のビット列**で書くのが LLVM の流儀である
/// （その値が単精度で表せることを検証してくれる）。
fn fbits(v: f64, ty: &ValueType) -> String {
    if matches!(ty, ValueType::F80) {
        return f80_bits(v);
    }
    let d = if matches!(ty, ValueType::F32) { v as f32 as f64 } else { v };
    format!("0x{:016X}", d.to_bits())
}

/// `x86_fp80` のビット列。LLVM の綴りは `0xK` ＋ 二十桁。
///
/// **`f64` を広げる。** `f64` の値はすべて `f80` で正確に表せる——
/// 指数も仮数も広いので、丸めが起きない。
///
/// # 限り
///
/// **リテラルは `f64` の精度でしか書けない。**
/// `f80` の余分な精度は**演算で得るもの**であって、綴りで入れるものではない——
/// 十進の綴りを八十ビットへ正しく丸めるには、任意精度の変換が要る。
fn f80_bits(v: f64) -> String {
    let b = v.to_bits();
    let sign = (b >> 63) & 1;
    let exp64 = ((b >> 52) & 0x7FF) as i64;
    let frac = b & 0x000F_FFFF_FFFF_FFFF;
    // 零は零。**非有限はこの言語に存在しない**（C-79）ので考えない
    if exp64 == 0 && frac == 0 {
        return format!("0xK{:04X}{:016X}", sign << 15, 0u64);
    }
    // 指数の下駄を履き替える。1023 → 16383
    let exp80 = (exp64 - 1023 + 16383) as u64;
    // **整数ビットは明示である**（`f64` の暗黙の 1 を立てる）
    let mant = (1u64 << 63) | (frac << 11);
    format!("0xK{:04X}{:016X}", (sign << 15) | exp80, mant)
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
            flows: HashMap::new(),
            done: false,
            strings: Vec::new(),
            decls: Vec::new(),
            copy_fns: Vec::new(),
            want: None,
            structs: HashMap::new(),
            wraps: HashMap::new(),
            struct_tys: Vec::new(),
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
        self.scopes.last_mut().unwrap().names.insert(name.to_string(), (p.clone(), ty, false));
        p
    }

    /// `x alias &= y` — **枠を作らず、y の枠を指す枠を作る**（C-53）。
    ///
    /// 指し先を入れ替えられる（`x &= z`）ので、指し先そのものではなく
    /// **指し先を入れた枠**を持つ。真っ直ぐな名前はこの一段を通らない。
    fn declare_alias(&mut self, name: &str, target: &str, ty: ValueType) -> String {
        let r = self.alloca("ptr");
        self.emit(&format!("store ptr {target}, ptr {r}"));
        self.scopes.last_mut().unwrap().names.insert(name.to_string(), (r.clone(), ty, true));
        r
    }

    fn lookup(&self, name: &str) -> Option<(String, ValueType, bool)> {
        for s in self.scopes.iter().rev() {
            if let Some(x) = s.names.get(name) {
                return Some(x.clone());
            }
        }
        None
    }

    /// 値の枠を返す。**間接なら一段辿る。**
    fn addr(&mut self, name: &str) -> Option<(String, ValueType)> {
        let (p, ty, indirect) = self.lookup(name)?;
        if !indirect {
            return Some((p, ty));
        }
        let q = self.tmp();
        self.emit(&format!("{q} = load ptr, ptr {p}"));
        Some((q, ty))
    }

    /// 別名そのものの枠（指し先を入れ替えるため）。**真っ直ぐな名前には無い。**
    fn alias_slot(&self, name: &str) -> Option<(String, ValueType)> {
        match self.lookup(name)? {
            (p, ty, true) => Some((p, ty)),
            _ => None,
        }
    }
}

// ================= 式 =================

impl Steel {
    /// **包み型を剥がす**（S-2）。構造体はそのまま。
    ///
    /// `wrap Meters = i64;` の `Meters` は、実装から見れば `i64` である——
    /// **包みは型検査のためにあり、置き場は変えない。**
    fn resolve(&self, t: &ValueType) -> ValueType {
        match t {
            ValueType::Named(n) => match self.wraps.get(n) {
                Some(base) => self.resolve(base),
                None => t.clone(),
            },
            ValueType::Array(e) => ValueType::Array(Box::new(self.resolve(e))),
            ValueType::Map(k, v) => {
                ValueType::Map(Box::new(self.resolve(k)), Box::new(self.resolve(v)))
            }
            _ => t.clone(),
        }
    }

    /// 構造体の欄の並び。**宣言の順が位置である。**
    fn fields_of(&self, name: &str) -> Option<Vec<(String, ValueType)>> {
        let d = self.structs.get(name)?;
        Some(
            d.fields
                .iter()
                .map(|f| (f.name.clone(), self.resolve(&f.ty.value)))
                .collect(),
        )
    }

    /// LLVM の型の名前。**一度だけ出す。**
    fn struct_ty(&mut self, name: &str) -> R<String> {
        let ll = format!("%vaak.s.{name}");
        if self.struct_tys.contains(&ll) {
            return Ok(ll);
        }
        self.struct_tys.push(ll.clone());
        let Some(fields) = self.fields_of(name) else {
            return err(format!("知らない構造体 `{name}`"), Span::NONE);
        };
        // **欄の型を先に出す。** 入れ子の構造体があるので
        for (_, t) in &fields {
            if let ValueType::Named(n) = t {
                self.struct_ty(n)?;
            }
        }
        let inner: Vec<String> = fields.iter().map(|(_, t)| ity(t)).collect();
        self.head_global(&format!("{ll} = type {{ {} }}", inner.join(", ")));
        Ok(ll)
    }

    /// 構造体の大きさ。**LLVM に数えさせる**——揃えを自分で数えない
    fn struct_size(&mut self, name: &str) -> R<String> {
        let ll = self.struct_ty(name)?;
        let g = self.tmp();
        self.emit(&format!("{g} = getelementptr {ll}, ptr null, i64 1"));
        let n = self.tmp();
        self.emit(&format!("{n} = ptrtoint ptr {g} to i64"));
        Ok(n)
    }

    /// 欄の場所。
    fn field_ptr(&mut self, base: &str, name: &str, idx: usize) -> R<String> {
        let ll = self.struct_ty(name)?;
        let g = self.tmp();
        self.emit(&format!("{g} = getelementptr {ll}, ptr {base}, i64 0, i32 {idx}"));
        Ok(g)
    }

    fn konst(&mut self, ty: &ValueType, v: i128) -> Val {
        let w = width(ty).unwrap_or(64);
        // **2^N を法として折り返す**（C-76）
        let m = if w >= 128 { v } else { v & ((1i128 << w) - 1) };
        let m = if signed(ty) && w < 128 && (m >> (w - 1)) & 1 == 1 {
            m - (1i128 << w)
        } else {
            m
        };
        Val { ok: "true".into(), v: format!("{m}"), ty: ty.clone() }
    }

    fn paradox(&mut self, ty: &ValueType) -> Val {
        Val { ok: "false".into(), v: "0".into(), ty: ty.clone() }
    }

    /// 幅を合わせる。**結果の型は左が決める**（`wrap_like` と同じ）。
    fn conv(&mut self, x: &str, from: &ValueType, to: &ValueType) -> String {
        // **浮動小数どうし。** 幅が違えば伸ばす／縮める
        if is_float(from) || is_float(to) {
            if from == to {
                return x.to_string();
            }
            let (f, t) = (ity(from), ity(to));
            let r = self.tmp();
            let op = match (is_float(from), is_float(to)) {
                (true, true) => {
                    // **広いか狭いかで決める。** f32 < f64 < f80
                    let rank = |t: &ValueType| match t {
                        ValueType::F32 => 0,
                        ValueType::F64 => 1,
                        _ => 2,
                    };
                    if rank(from) < rank(to) { "fpext" } else { "fptrunc" }
                }
                // **整数と浮動小数は混ぜられない**（検査器が捕らえる）。
                // ここへ来るのは注釈で明示したときだけ
                (false, true) => {
                    if signed(from) { "sitofp" } else { "uitofp" }
                }
                (true, false) => {
                    if signed(to) { "fptosi" } else { "fptoui" }
                }
                (false, false) => unreachable!(),
            };
            self.emit(&format!("{r} = {op} {f} {x} to {t}"));
            return r;
        }
        let (a, b) = (width(from).unwrap_or(64), width(to).unwrap_or(64));
        if a == b {
            return x.to_string();
        }
        let t = self.tmp();
        let op = if a > b {
            "trunc"
        } else if signed(from) {
            "sext"
        } else {
            "zext"
        };
        self.emit(&format!("{t} = {op} i{a} {x} to i{b}"));
        t
    }

    /// 二つ組を一つに畳む。**片方でも paradox なら paradox**（C-22：伝播する）。
    fn both_ok(&mut self, a: &str, b: &str) -> String {
        if a == "true" {
            return b.to_string();
        }
        if b == "true" {
            return a.to_string();
        }
        let t = self.tmp();
        self.emit(&format!("{t} = and i1 {a}, {b}"));
        t
    }

    fn binary(&mut self, op: BinOp, l: Val, r: Val, span: Span) -> R<Val> {
        use BinOp::*;
        let ok = self.both_ok(&l.ok, &r.ok);

        // **浮動小数は別の道。** 型は混ぜられないので左を見れば足りる
        if is_float(&l.ty) {
            return self.float_binary(op, l, r, ok, span);
        }

        // 比較は **i64 に伸ばしてから**（`as_int` が i128 で比べるのと同じ順序）
        if matches!(op, Lt | Le | Gt | Ge | Eq | Ne) {
            let a = self.widen64(&l);
            let b = self.widen64(&r);
            let p = match op {
                Lt => "slt",
                Le => "sle",
                Gt => "sgt",
                Ge => "sge",
                Eq => "eq",
                _ => "ne",
            };
            let t = self.tmp();
            self.emit(&format!("{t} = icmp {p} i64 {a}, {b}"));
            return Ok(Val { ok, v: t, ty: ValueType::U1 });
        }

        if matches!(op, And | Or) {
            let a = self.nonzero(&l);
            let b = self.nonzero(&r);
            let t = self.tmp();
            self.emit(&format!("{t} = {} i1 {a}, {b}", if op == And { "and" } else { "or" }));
            return Ok(Val { ok, v: t, ty: ValueType::U1 });
        }

        let ty = l.ty.clone();
        let w = width(&ty).unwrap_or(64);
        let rv = self.conv(&r.v, &r.ty, &ty);
        let t = self.tmp();
        match op {
            Add | Sub | Mul | Shl | BitAnd | BitXor | BitOr => {
                let o = match op {
                    Add => "add",
                    Sub => "sub",
                    Mul => "mul",
                    Shl => "shl",
                    BitAnd => "and",
                    BitXor => "xor",
                    _ => "or",
                };
                // **`nsw` も `nuw` も付けない。** 溢れは折り返す（C-76）
                self.emit(&format!("{t} = {o} i{w} {}, {rv}", l.v));
                Ok(Val { ok, v: t, ty })
            }
            Shr => {
                let o = if signed(&ty) { "ashr" } else { "lshr" };
                self.emit(&format!("{t} = {o} i{w} {}, {rv}", l.v));
                Ok(Val { ok, v: t, ty })
            }
            Div | Mod => Ok(self.euclid(op == Div, &l.v, &rv, &ty, ok)),
            _ => err("この演算子は STEEL がまだ扱えない", span),
        }
    }

    /// 浮動小数の演算。**inf も NaN も、この言語には存在しない**（C-79）。
    ///
    /// 演算のあとで有限性を確かめ、有限でなければ **paradox** にする。
    /// 検査は `fabs` と一度の比較で済む——除算そのものが遅いので、
    /// **相対的な費用は無視できる。**
    fn float_binary(&mut self, op: BinOp, l: Val, r: Val, ok: String, span: Span) -> R<Val> {
        use BinOp::*;
        let t = ity(&l.ty);

        if matches!(op, Lt | Le | Gt | Ge | Eq | Ne) {
            let p = match op {
                Lt => "olt",
                Le => "ole",
                Gt => "ogt",
                Ge => "oge",
                Eq => "oeq",
                _ => "one",
            };
            let x = self.tmp();
            self.emit(&format!("{x} = fcmp {p} {t} {}, {}", l.v, r.v));
            return Ok(Val { ok, v: x, ty: ValueType::U1 });
        }

        let o = match op {
            Add => "fadd",
            Sub => "fsub",
            Mul => "fmul",
            Div => "fdiv",
            Mod => "frem",
            _ => return err("この演算子は浮動小数に使えない", span),
        };
        let x = self.tmp();
        self.emit(&format!("{x} = {o} {t} {}, {}", l.v, r.v));
        let fin = self.finite(&x, &l.ty);
        let ok2 = self.both_ok(&ok, &fin);
        Ok(Val { ok: ok2, v: x, ty: l.ty })
    }

    /// **有限か。** NaN なら `olt` が偽になるので、これ一つで両方を捕まえる。
    fn finite(&mut self, x: &str, ty: &ValueType) -> String {
        let t = ity(ty);
        let bits = match ty {
            ValueType::F32 => "f32",
            ValueType::F80 => "f80",
            _ => "f64",
        };
        self.declare_fabs(bits, &t);
        let a = self.tmp();
        self.emit(&format!("{a} = call {t} @llvm.fabs.{bits}({t} {x})"));
        let f = self.tmp();
        // **無限との比較。** `f80` は綴りが違う（`0xK` ＋ 二十桁）
        let inf = if matches!(ty, ValueType::F80) {
            "0xK7FFF8000000000000000"
        } else {
            "0x7FF0000000000000"
        };
        self.emit(&format!("{f} = fcmp olt {t} {a}, {inf}"));
        f
    }

    fn declare_fabs(&mut self, bits: &str, t: &str) {
        let d = format!("declare {t} @llvm.fabs.{bits}({t})\n");
        if !self.decls.contains(&d) {
            self.decls.push(d);
        }
    }

    /// **ユークリッド除算**（C-76）。剰余は必ず非負。
    ///
    /// ```text
    /// q = x / y;  r = x % y
    /// r < 0 ならば y の符号に応じて寄せる
    /// ```
    ///
    /// **零で割れば paradox。** 落ちない
    fn euclid(&mut self, want_q: bool, x: &str, y: &str, ty: &ValueType, ok: String) -> Val {
        let w = width(ty).unwrap_or(64);
        let i = format!("i{w}");
        let zero = self.tmp();
        self.emit(&format!("{zero} = icmp eq {i} {y}, 0"));
        let live = self.tmp();
        self.emit(&format!("{live} = xor i1 {zero}, true"));
        let ok2 = self.both_ok(&ok, &live);

        // **零で割らないように守る。** LLVM の除算は未定義動作を持つ
        let safe = self.tmp();
        self.emit(&format!("{safe} = select i1 {zero}, {i} 1, {i} {y}"));

        let (q, r) = if signed(ty) {
            let q = self.tmp();
            self.emit(&format!("{q} = sdiv {i} {x}, {safe}"));
            let r = self.tmp();
            self.emit(&format!("{r} = srem {i} {x}, {safe}"));
            (q, r)
        } else {
            let q = self.tmp();
            self.emit(&format!("{q} = udiv {i} {x}, {safe}"));
            let r = self.tmp();
            self.emit(&format!("{r} = urem {i} {x}, {safe}"));
            // 符号無しなら剰余は既に非負
            return Val { ok: ok2, v: if want_q { q } else { r }, ty: ty.clone() };
        };

        let neg = self.tmp();
        self.emit(&format!("{neg} = icmp slt {i} {r}, 0"));
        let ypos = self.tmp();
        self.emit(&format!("{ypos} = icmp sgt {i} {safe}, 0"));
        // q の寄せ：r<0 のとき y>0 なら q-1、y<0 なら q+1
        let d = self.tmp();
        self.emit(&format!("{d} = select i1 {ypos}, {i} -1, {i} 1"));
        let q2 = self.tmp();
        self.emit(&format!("{q2} = add {i} {q}, {d}"));
        let qf = self.tmp();
        self.emit(&format!("{qf} = select i1 {neg}, {i} {q2}, {i} {q}"));
        // r の寄せ：r<0 のとき |y| を足す
        let absy = self.tmp();
        self.emit(&format!("{absy} = sub {i} 0, {safe}"));
        let ay = self.tmp();
        self.emit(&format!("{ay} = select i1 {ypos}, {i} {safe}, {i} {absy}"));
        let r2 = self.tmp();
        self.emit(&format!("{r2} = add {i} {r}, {ay}"));
        let rf = self.tmp();
        self.emit(&format!("{rf} = select i1 {neg}, {i} {r2}, {i} {r}"));

        Val { ok: ok2, v: if want_q { qf } else { rf }, ty: ty.clone() }
    }

    // ================= 集合体 =================

    /// 場に新しい集合体を作る。**長さは実行時に決まってよい。**
    fn new_collection(&mut self, len: &str, ty: &ValueType) -> String {
        let es = elem_size(&elem_of(ty).unwrap_or(ValueType::I64));
        let p = self.tmp();
        self.emit(&format!("{p} = call ptr @vaak.new(i64 {len}, i64 {es})"));
        p
    }

    /// 要素の場所。**添字は確かめてから渡すこと。**
    fn elem_ptr(&mut self, base: &str, idx: &str, ty: &ValueType) -> String {
        let el = elem_of(ty).unwrap_or(ValueType::I64);
        let d = self.tmp();
        self.emit(&format!("{d} = call ptr @vaak.data(ptr {base})"));
        let g = self.tmp();
        self.emit(&format!("{g} = getelementptr {}, ptr {d}, i64 {idx}", ity(&el)));
        g
    }

    fn coll_len(&mut self, base: &str) -> String {
        let n = self.tmp();
        self.emit(&format!("{n} = call i64 @vaak.len(ptr {base})"));
        n
    }

    /// **添字が範囲に入っているか。** 外なら paradox（C-46）。
    ///
    /// 返すのは（範囲内か, 安全な添字）。外れていても**読める場所**を渡す——
    /// 落ちるより paradox の方がよい。
    fn bounds(&mut self, base: &str, idx: &str) -> (String, String) {
        let n = self.coll_len(base);
        let lo = self.tmp();
        self.emit(&format!("{lo} = icmp sge i64 {idx}, 0"));
        let hi = self.tmp();
        self.emit(&format!("{hi} = icmp slt i64 {idx}, {n}"));
        let ok = self.tmp();
        self.emit(&format!("{ok} = and i1 {lo}, {hi}"));
        let safe = self.tmp();
        self.emit(&format!("{safe} = select i1 {ok}, i64 {idx}, i64 0"));
        (ok, safe)
    }

    /// **深く複製する**（C-33）。
    ///
    /// 要素が場を持たなければ丸ごと写すだけで済むが、
    /// **入れ子なら要素も写さねばならない**——型ごとに写す関数を出す。
    fn deep_copy(&mut self, p: &str, ty: &ValueType) -> String {
        let f = self.copy_fn(ty);
        let q = self.tmp();
        self.emit(&format!("{q} = call ptr {f}(ptr {p})"));
        q
    }

    /// 型を名前にする。**同じ型なら同じ名前**になること。
    fn mangle(t: &ValueType) -> String {
        match t {
            ValueType::U1 => "u1".into(),
            ValueType::U8 => "u8".into(),
            ValueType::U16 => "u16".into(),
            ValueType::U32 => "u32".into(),
            ValueType::I32 => "i32".into(),
            ValueType::I64 => "i64".into(),
            ValueType::F32 => "f32".into(),
            ValueType::F64 => "f64".into(),
            ValueType::F80 => "f80".into(),
            ValueType::Str => "str".into(),
            ValueType::Array(e) => format!("a{}", Self::mangle(e)),
            ValueType::Map(k, v) => format!("m{}_{}", Self::mangle(k), Self::mangle(v)),
            ValueType::Hash(k, v) => format!("h{}_{}", Self::mangle(k), Self::mangle(v)),
            ValueType::Named(n) => format!("n{n}"),
        }
    }

    /// 型ごとの**深く写す関数**を出し、その名前を返す。
    ///
    /// **一度だけ出す。** 同じ型で二度呼ばれても、二つ目は名前だけ返る。
    ///
    /// 要素が場を持たなければ丸ごと写す（`@vaak.copy`）。
    /// 持つなら**一つずつ、要素の写す関数を呼ぶ**——
    /// **値のグラフに循環は無い**（C-63）ので、この再帰は必ず止まる。
    fn copy_fn(&mut self, ty: &ValueType) -> String {
        // **包みを剥がしてから見る**（S-2）。剥がさないと構造体と間違える
        let ty = &self.resolve(ty);
        // **hash も頭の形が違う。** 並びと bucket を持っている
        if let ValueType::Hash(kt, vt) = ty {
            let (kt, vt) = ((**kt).clone(), (**vt).clone());
            let fname = format!("@vaak.copy.{}", type_tag(ty));
            if self.copy_fns.contains(&fname) {
                return fname;
            }
            self.copy_fns.push(fname.clone());
            let ht = ValueType::Hash(Box::new(kt.clone()), Box::new(vt.clone()));
            let stem = match self.hash_fns(&ht) {
                Ok(x) => x,
                Err(_) => return fname,
            };
            let ent = self.hash_ent_ty(&ht);
            let (ki, vi) = (ity(&kt), ity(&vt));
            let kcopy = if is_heap(&kt) {
                let f = self.copy_fn(&kt);
                format!("  %kc = call ptr {f}(ptr %k)\n")
            } else {
                format!("  %kc = bitcast {ki} %k to {ki}\n")
            };
            let vcopy = if is_heap(&vt) {
                let f = self.copy_fn(&vt);
                format!("  %vc = call ptr {f}(ptr %v)\n")
            } else {
                format!("  %vc = bitcast {vi} %v to {vi}\n")
            };
            // **入れ直す。** 並びも鎖も作り直るので、穴が消えて順序は保たれる
            let body = format!(
                "define internal ptr {fname}(ptr %p) {{\n\
                 entry:\n  \
                 %q = call ptr @vaak.hash.new()\n  \
                 %ng = getelementptr i8, ptr %p, i64 8\n  \
                 %n = load i64, ptr %ng\n  \
                 %eg = getelementptr i8, ptr %p, i64 24\n  \
                 %ents = load ptr, ptr %eg\n  \
                 br label %head\n\
                 head:\n  \
                 %i = phi i64 [ 0, %entry ], [ %i2, %next ]\n  \
                 %go = icmp slt i64 %i, %n\n  \
                 br i1 %go, label %body, label %done\n\
                 body:\n  \
                 %lp = getelementptr {ent}, ptr %ents, i64 %i, i32 3\n  \
                 %lv = load i8, ptr %lp\n  \
                 %alive = icmp ne i8 %lv, 0\n  \
                 br i1 %alive, label %take, label %next\n\
                 take:\n  \
                 %kp = getelementptr {ent}, ptr %ents, i64 %i, i32 0\n  \
                 %k = load {ki}, ptr %kp\n  \
                 %vp = getelementptr {ent}, ptr %ents, i64 %i, i32 1\n  \
                 %v = load {vi}, ptr %vp\n\
                 {kcopy}{vcopy}  \
                 call void {stem}.put(ptr %q, {ki} %kc, {vi} %vc)\n  \
                 br label %next\n\
                 next:\n  \
                 %i2 = add i64 %i, 1\n  \
                 br label %head\n\
                 done:\n  ret ptr %q\n\
                 }}"
            );
            self.head_global(&body);
            return fname;
        }
        // **写像は頭の形が違う。** 並びを二つ持っている
        if let ValueType::Map(kt, vt) = ty {
            let (kt, vt) = ((**kt).clone(), (**vt).clone());
            let fname = format!("@vaak.copy.{}", type_tag(ty));
            if self.copy_fns.contains(&fname) {
                return fname;
            }
            self.copy_fns.push(fname.clone());
            let (ks, vs) = (elem_size(&kt), elem_size(&vt));
            let mut body = format!(
                "define internal ptr {fname}(ptr %p) {{\nentry:\n  %q = call ptr @vaak.map.clone(ptr %p, i64 {ks}, i64 {vs})\n  %n = load i64, ptr %q\n"
            );
            // 場を持つ鍵・値は**一つずつ深く写す**
            for (which, ty2, sz) in
                [("keys", kt.clone(), ks), ("vals", vt.clone(), vs)]
            {
                let _ = sz;
                if !is_heap(&ty2) {
                    continue;
                }
                let inner = self.copy_fn(&ty2);
                body.push_str(&format!(
                    "  %d{which} = call ptr @vaak.map.{which}(ptr %q)\n  br label %h{which}\n                     h{which}:\n  %i{which} = phi i64 [ 0, %entry ], [ %j{which}, %b{which} ]\n                       %g{which} = icmp slt i64 %i{which}, %n\n                       br i1 %g{which}, label %b{which}, label %e{which}\n                     b{which}:\n                       %p{which} = getelementptr ptr, ptr %d{which}, i64 %i{which}\n                       %v{which} = load ptr, ptr %p{which}\n                       %c{which} = call ptr {inner}(ptr %v{which})\n                       store ptr %c{which}, ptr %p{which}\n                       %j{which} = add i64 %i{which}, 1\n  br label %h{which}\n                     e{which}:\n"
                ));
            }
            body.push_str("  ret ptr %q\n}");
            self.head_global(&body);
            return fname;
        }
        // **構造体は欄ごとに写す**
        if let ValueType::Named(name) = ty {
            let name = name.clone();
            let fname = format!("@vaak.copy.n{name}");
            if self.copy_fns.contains(&fname) {
                return fname;
            }
            self.copy_fns.push(fname.clone());
            let fields = self.fields_of(&name).unwrap_or_default();
            let ll = self.struct_ty(&name).unwrap_or_else(|_| "%err".into());
            let mut body = format!(
                "define internal ptr {fname}(ptr %p) {{\nentry:\n  %sz = getelementptr {ll}, ptr null, i64 1\n  %n = ptrtoint ptr %sz to i64\n  %q = call ptr @vaak.alloc(i64 %n)\n"
            );
            for (i, (_, fty)) in fields.iter().enumerate() {
                let t = ity(fty);
                body.push_str(&format!(
                    "  %sp{i} = getelementptr {ll}, ptr %p, i64 0, i32 {i}\n  %sv{i} = load {t} , ptr %sp{i}\n"
                ));
                let stored = if is_heap(fty) {
                    let inner = self.copy_fn(fty);
                    body.push_str(&format!("  %cv{i} = call ptr {inner}(ptr %sv{i})\n"));
                    format!("%cv{i}")
                } else {
                    format!("%sv{i}")
                };
                body.push_str(&format!(
                    "  %dp{i} = getelementptr {ll}, ptr %q, i64 0, i32 {i}\n  store {t} {stored}, ptr %dp{i}\n"
                ));
            }
            body.push_str("  ret ptr %q\n}");
            self.head_global(&body);
            return fname;
        }
        let el = elem_of(ty).unwrap_or(ValueType::I64);
        let es = elem_size(&el);
        let name = format!("@vaak.copy.{}", Self::mangle(ty));
        if self.copy_fns.contains(&name) {
            return name;
        }
        self.copy_fns.push(name.clone());
        if !is_heap(&el) {
            // **丸ごと写せる。** 要素が場を持たない
            self.head_global(&format!(
                "define internal ptr {name}(ptr %p) {{\nentry:\n  %q = call ptr @vaak.copy(ptr %p, i64 {es})\n  ret ptr %q\n}}"
            ));
            return name;
        }
        // **入れ子。** 要素も写す
        let inner = self.copy_fn(&el);
        let body = format!(
            "define internal ptr {name}(ptr %p) {{\nentry:\n  %n = load i64, ptr %p\n  %q = call ptr @vaak.new(i64 %n, i64 {es})\n  %sd = call ptr @vaak.data(ptr %p)\n  %dd = call ptr @vaak.data(ptr %q)\n  br label %head\nhead:\n  %i = phi i64 [ 0, %entry ], [ %i2, %body ]\n  %go = icmp slt i64 %i, %n\n  br i1 %go, label %body, label %done\nbody:\n  %sp = getelementptr ptr, ptr %sd, i64 %i\n  %sv = load ptr, ptr %sp\n  %cv = call ptr {inner}(ptr %sv)\n  %dp = getelementptr ptr, ptr %dd, i64 %i\n  store ptr %cv, ptr %dp\n  %i2 = add i64 %i, 1\n  br label %head\ndone:\n  ret ptr %q\n}}"
        );
        self.head_global(&body);
        name
    }

    /// `new T ( 引数 )` — **配列と写像は位置で、構造体は名前で**（C-78）。
    fn construct(&mut self, ty: &Type, args: &CtorArgs, span: Span) -> R<Region> {
        let t = self.resolve(&ty.value);
        // **構造体は欄を名前で**（C-78）
        if let ValueType::Named(name) = &t {
            let name = name.clone();
            let Some(fields) = self.fields_of(&name) else {
                return err(format!("知らない構造体 `{name}`"), span);
            };
            // **欄を一つも書かないのは名前で書いたのと同じ**——既定値で埋まる
            let empty = Vec::new();
            let given = match args {
                CtorArgs::Named(g) => g,
                CtorArgs::Positional(p) if p.is_empty() => &empty,
                _ => return err("構造体は欄を名前で構築する", span),
            };
            let size = self.struct_size(&name)?;
            let p = self.tmp();
            self.emit(&format!("{p} = call ptr @vaak.alloc(i64 {size})"));
            for (i, (fname, fty)) in fields.iter().enumerate() {
                // 与えられた値。**無ければ既定**（検査器が「値が無い」を捕らえている）
                let v = match given.iter().find(|(g, _)| g == fname) {
                    Some((_, e)) => {
                        let v = self.expr(e)?;
                        let Some(v) = v else { return err("欄に値が無い", e.span) };
                        if is_heap(fty) {
                            self.deep_copy(&v.v.clone(), fty)
                        } else {
                            self.conv(&v.v.clone(), &v.ty.clone(), fty)
                        }
                    }
                    None => match self.default_of(fname, &name, fty)? {
                        Some(v) => v,
                        None => return err(format!("欄 `{fname}` に値が無い"), span),
                    },
                };
                let g = self.field_ptr(&p, &name, i)?;
                self.emit(&format!("store {} {v}, ptr {g}", ity(fty)));
            }
            return Ok(Some(Val { ok: "true".into(), v: p, ty: t }));
        }
        // **包み型を剥がす／包む**（S-2）。`new i64 ( m )` は数を数に
        if !is_heap(&t) {
            let CtorArgs::Positional(a) = args else {
                return err("この型は STEEL がまだ構築できない", span);
            };
            let Some(first) = a.first() else {
                return err("`new` に値が要る", span);
            };
            let v = self.expr(first)?;
            let Some(v) = v else { return err("`new` に値が要る", span) };
            let c = self.conv(&v.v.clone(), &v.ty.clone(), &t);
            return Ok(Some(Val { ok: v.ok, v: c, ty: t }));
        }
        let CtorArgs::Positional(a) = args else {
            return err("集合体は位置で構築する", span);
        };
        // `new K V hash ( )` — **空**
        if is_hash(&t) {
            if !a.is_empty() {
                return err("`new … hash` は空でなければならない", span);
            }
            self.hash_fns(&t)?;
            let p = self.tmp();
            self.emit(&format!("{p} = call ptr @vaak.hash.new()"));
            return Ok(Some(Val { ok: "true".into(), v: p, ty: t }));
        }
        // `new K V map ( )` — **空。** 中身は写像リテラルで書く
        if is_map(&t) {
            if !a.is_empty() {
                return err("`new … map` は空でなければならない（中身は `( 鍵 => 値 )` で書く）", span);
            }
            let p = self.map_new();
            return Ok(Some(Val { ok: "true".into(), v: p, ty: t }));
        }
        let el = elem_of(&t).unwrap_or(ValueType::I64);
        // `new T array ( )` — 空
        let Some(nx) = a.first() else {
            let p = self.new_collection("0", &t);
            return Ok(Some(Val { ok: "true".into(), v: p, ty: t }));
        };
        let n = self.expr(nx)?;
        let Some(n) = n else { return err("`new` の個数に値が無い", nx.span) };
        // **包む／剥がす**（C-78 / S-2）。集合体なら長さではなく構築である。
        // 構築は深い複製なので、同じ置き場を型だけ変えて返してはならない。
        if is_heap(&n.ty) {
            let copied = self.deep_copy(&n.v, &t);
            return Ok(Some(Val { ok: n.ok, v: copied, ty: t }));
        }
        let len = self.widen64(&n);
        // **負の個数は零とみなす。** 落ちるより畳む
        let neg = self.tmp();
        self.emit(&format!("{neg} = icmp slt i64 {len}, 0"));
        let safe = self.tmp();
        self.emit(&format!("{safe} = select i1 {neg}, i64 0, i64 {len}"));
        let p = self.new_collection(&safe, &t);

        // 埋める値。**無ければ零**
        let fill = match a.get(1) {
            Some(fx) => {
                let f = self.expr(fx)?;
                let Some(f) = f else { return err("埋める値が無い", fx.span) };
                self.conv(&f.v.clone(), &f.ty.clone(), &el)
            }
            None => match el {
                ValueType::F80 => "0xK00000000000000000000".into(),
                _ if is_float(&el) => "0.0".into(),
                _ => "0".into(),
            },
        };
        // 埋める。**入れ子なら一つずつ写す**——
        // 同じ場所を全部の枡に入れると、一つ書き換えたら全部変わる（C-33 に反する）
        let head = self.label("fill.head");
        let body = self.label("fill.body");
        let done = self.label("fill.done");
        let iv = self.alloca("i64");
        self.emit(&format!("store i64 0, ptr {iv}"));
        self.br(&head);
        self.place(&head);
        let i = self.tmp();
        self.emit(&format!("{i} = load i64, ptr {iv}"));
        let go = self.tmp();
        self.emit(&format!("{go} = icmp slt i64 {i}, {safe}"));
        self.cbr(&go, &body, &done);
        self.place(&body);
        let g = self.elem_ptr(&p, &i, &t);
        let one = if is_heap(&el) {
            self.deep_copy(&fill, &el)
        } else {
            fill.clone()
        };
        self.emit(&format!("store {} {one}, ptr {g}", ity(&el)));
        let i2 = self.tmp();
        self.emit(&format!("{i2} = add i64 {i}, 1"));
        self.emit(&format!("store i64 {i2}, ptr {iv}"));
        self.br(&head);
        self.place(&done);

        Ok(Some(Val { ok: "true".into(), v: p, ty: t }))
    }

    /// 欄の既定値（`let x : i64 := 0;` の `0`）。
    fn default_of(&mut self, fname: &str, sname: &str, fty: &ValueType) -> R<Option<String>> {
        let d = self.structs.get(sname).cloned();
        let Some(d) = d else { return Ok(None) };
        let Some(f) = d.fields.iter().find(|f| f.name == fname) else {
            return Ok(None);
        };
        let Some(def) = f.default.clone() else { return Ok(None) };
        let v = self.expr(&def)?;
        let Some(v) = v else { return Ok(None) };
        Ok(Some(if is_heap(fty) {
            self.deep_copy(&v.v.clone(), fty)
        } else {
            self.conv(&v.v.clone(), &v.ty.clone(), fty)
        }))
    }

    /// 文字列の定数。**場の形（長さ・容量・中身）で置く。**
    fn string_const(&mut self, b: &[u8]) -> String {
        let name = format!("@.vaak.s{}", self.strings.len());
        let body: String =
            b.iter().map(|c| format!("\\{c:02X}")).collect::<Vec<_>>().join("");
        let n = b.len();
        self.head_global(&format!(
            "{name} = internal constant {{ i64, i64, [{n} x i8] }} \
             {{ i64 {n}, i64 {n}, [{n} x i8] c\"{body}\" }}"
        ));
        self.strings.push((name.clone(), b.to_vec()));
        name
    }

    fn head_global(&mut self, d: &str) {
        let line = format!("{d}\n");
        if !self.decls.contains(&line) {
            self.decls.push(line);
        }
    }

    /// 枡（`i64`）へ入れる形にする。**集合体は場所を数として入れる。**
    ///
    /// 段と分岐の合流点は一つの枡を使う（領域は高々一つの値、C-14）ので、
    /// **型ごとに枡を分けない。**
    fn to_slot(&mut self, v: &Val) -> String {
        if is_heap(&v.ty) {
            let t = self.tmp();
            self.emit(&format!("{t} = ptrtoint ptr {} to i64", v.v));
            return t;
        }
        self.conv(&v.v.clone(), &v.ty.clone(), &ValueType::I64)
    }

    /// 枡から取り出す。
    fn from_slot(&mut self, raw: &str, ty: &ValueType) -> String {
        if is_heap(ty) {
            let t = self.tmp();
            self.emit(&format!("{t} = inttoptr i64 {raw} to ptr"));
            return t;
        }
        self.conv(raw, &ValueType::I64, ty)
    }

    fn widen64(&mut self, x: &Val) -> String {
        self.conv(&x.v.clone(), &x.ty.clone(), &ValueType::I64)
    }

    /// **`≠ 0` で真**（条件は任意の整数型）。
    fn nonzero(&mut self, x: &Val) -> String {
        if x.ty == ValueType::U1 {
            return x.v.clone();
        }
        if is_float(&x.ty) {
            let t = ity(&x.ty);
            let r = self.tmp();
            let zero = if matches!(x.ty, ValueType::F80) {
                "0xK00000000000000000000"
            } else {
                "0.0"
            };
            self.emit(&format!("{r} = fcmp one {t} {}, {zero}", x.v));
            return r;
        }
        let w = width(&x.ty).unwrap_or(64);
        let t = self.tmp();
        self.emit(&format!("{t} = icmp ne i{w} {}, 0", x.v));
        t
    }
}

// ================= 領域・スコープ・脱出段 =================

impl Steel {
    /// 領域。**高々一つの値**（C-14）。
    ///
    /// 値を置くものが二つあれば検査器が捕らえているので、ここでは数えない——
    /// **最後に残ったものを領域の値とする。**
    fn region(&mut self, items: &[Expr]) -> R<Region> {
        let mut out: Region = None;
        for e in items {
            let r = self.expr(e)?;
            if r.is_some() {
                out = r;
            }
        }
        Ok(out)
    }

    /// 段を開き、本体を組み、段を閉じる。**戻り値は段の外界面。**
    fn open_stage(&mut self, cont: Option<String>, count: bool) -> usize {
        let exit = self.label("stage.exit");
        let slot = self.alloca("i64");
        let ok_slot = self.alloca("i1");
        let count_slot = if count { Some(self.alloca("i64")) } else { None };
        self.emit(&format!("store i1 false, ptr {ok_slot}"));
        self.emit(&format!("store i64 0, ptr {slot}"));
        if let Some(c) = &count_slot {
            self.emit(&format!("store i64 0, ptr {c}"));
        }
        self.stages.push(Stage {
            exit,
            slot,
            ok_slot,
            ty: ValueType::I64,
            cont,
            count_slot,
        });
        self.stages.len() - 1
    }

    /// 段を閉じて外界面を読み出す。
    fn close_stage(&mut self) -> Val {
        let st = self.stages.pop().unwrap();
        self.place(&st.exit.clone());
        let ok = self.tmp();
        self.emit(&format!("{ok} = load i1, ptr {}", st.ok_slot));
        let raw = self.tmp();
        self.emit(&format!("{raw} = load i64, ptr {}", st.slot));
        let v = self.from_slot(&raw, &st.ty.clone());
        Val { ok, v, ty: st.ty }
    }

    /// 段へ積み荷を置いて出る。
    fn leave(&mut self, depth: usize, payload: Option<Val>) -> R<()> {
        if depth == 0 || depth > self.stages.len() {
            return err("段が足りない", Span::NONE);
        }
        let idx = self.stages.len() - depth;
        let (slot, ok_slot, exit) = {
            let st = &self.stages[idx];
            (st.slot.clone(), st.ok_slot.clone(), st.exit.clone())
        };
        match payload {
            Some(p) => {
                let w = self.to_slot(&p);
                self.stages[idx].ty = p.ty.clone();
                self.emit(&format!("store i64 {w}, ptr {slot}"));
                self.emit(&format!("store i1 {}, ptr {ok_slot}", p.ok));
            }
            None => {
                // **`break` の値は paradox**（C-43）
                self.emit(&format!("store i1 false, ptr {ok_slot}"));
            }
        }
        self.br(&exit);
        Ok(())
    }
}

// ================= 式を組む =================

impl Steel {
    fn expr(&mut self, e: &Expr) -> R<Region> {
        use ExprKind as E;
        match &e.kind {
            E::Int(t) => {
                let v: i128 = t.parse().map_err(|_| SteelError {
                    msg: "整数として読めない".into(),
                    span: e.span,
                })?;
                Ok(Some(self.konst(&ValueType::I64, v)))
            }

            // **`u1` で確定。** 文脈を見ない（C-97）
            &E::Bool(b) => Ok(Some(Val {
                ok: "true".into(),
                v: (if b { "1" } else { "0" }).into(),
                ty: ValueType::U1,
            })),

            E::Float(t) => {
                let v: f64 = t.parse().map_err(|_| SteelError {
                    msg: "浮動小数として読めない".into(),
                    span: e.span,
                })?;
                Ok(Some(Val {
                    ok: "true".into(),
                    v: fbits(v, &ValueType::F64),
                    ty: ValueType::F64,
                }))
            }

            // `[ a, b, c ]` — **場に置く**
            E::ArrayLit(items) => {
                let mut vals = Vec::new();
                for it in items {
                    let v = self.expr(it)?;
                    let Some(v) = v else { return err("配列の要素に値が無い", it.span) };
                    vals.push(v);
                }
                let el = vals.first().map(|v| v.ty.clone()).unwrap_or(ValueType::I64);
                let ty = ValueType::Array(Box::new(el.clone()));
                let p = self.new_collection(&vals.len().to_string(), &ty);
                for (i, v) in vals.iter().enumerate() {
                    let g = self.elem_ptr(&p, &i.to_string(), &ty);
                    // **リテラルの要素も深く複製する**（C-33）
                    let c = if is_heap(&el) {
                        self.deep_copy(&v.v.clone(), &el)
                    } else {
                        self.conv(&v.v.clone(), &v.ty.clone(), &el)
                    };
                    self.emit(&format!("store {} {c}, ptr {g}", ity(&el)));
                }
                Ok(Some(Val { ok: "true".into(), v: p, ty }))
            }

            // `new T array(n, 埋める値)`
            E::Construct { ty, args } => self.construct(ty, args, e.span),

            // 文字列は `u8 array` を包んだ型（C-77）。**場へ写してから渡す**
            E::Str(t) => {
                let bytes = t.as_bytes().to_vec();
                let g = self.string_const(&bytes);
                let p = self.tmp();
                self.emit(&format!("{p} = call ptr @vaak.copy(ptr {g}, i64 1)"));
                Ok(Some(Val { ok: "true".into(), v: p, ty: ValueType::Str }))
            }

            // `p.欄`
            E::Field { base, name } => {
                let b = self.expr(base)?;
                let Some(b) = b else { return err("受け手に値が無い", base.span) };
                let ValueType::Named(sname) = &b.ty else {
                    return err("欄を持つのは構造体だけ", base.span);
                };
                let sname = sname.clone();
                let Some(fields) = self.fields_of(&sname) else {
                    return err(format!("知らない構造体 `{sname}`"), base.span);
                };
                let Some(i) = fields.iter().position(|(f, _)| f == name) else {
                    return err(format!("`{sname}` に欄 `{name}` は無い"), e.span);
                };
                let fty = fields[i].1.clone();
                let g = self.field_ptr(&b.v.clone(), &sname, i)?;
                let t = self.tmp();
                self.emit(&format!("{t} = load {}, ptr {g}", ity(&fty)));
                Ok(Some(Val { ok: b.ok, v: t, ty: fty }))
            }

            // `a[i]` — **範囲外は paradox**（C-46）
            E::Index { base, index } => {
                let b = self.expr(base)?;
                let i = self.expr(index)?;
                let (Some(b), Some(i)) = (b, i) else {
                    return err("添字に値が無い", e.span);
                };
                if !is_heap(&b.ty) {
                    return err("添字を取れるのは集合体だけ", base.span);
                }
                // **hash も鍵で引く。** 引き方が違うだけ
                if is_hash(&b.ty) {
                    let ValueType::Hash(kt, vt) = &b.ty else { unreachable!() };
                    let (kt, vt) = ((**kt).clone(), (**vt).clone());
                    let stem = self.hash_fns(&b.ty.clone())?;
                    let kc = self.conv(&i.v.clone(), &i.ty.clone(), &kt);
                    let at = self.tmp();
                    self.emit(&format!(
                        "{at} = call i64 {stem}.find(ptr {}, {} {kc})",
                        b.v,
                        ity(&kt)
                    ));
                    let hit = self.tmp();
                    self.emit(&format!("{hit} = icmp sge i64 {at}, 0"));
                    let vty = ity(&vt);
                    let slot = self.alloca(&vty);
                    let zero = self.zero_of(&vt);
                    self.emit(&format!("store {vty} {zero}, ptr {slot}"));
                    let take = self.label("hget.hit");
                    let after = self.label("hget.after");
                    self.cbr(&hit, &take, &after);
                    self.place(&take);
                    let ents = self.hash_entries(&b.v.clone());
                    let ent = self.hash_ent_ty(&b.ty.clone());
                    let vp = self.tmp();
                    self.emit(&format!(
                        "{vp} = getelementptr {ent}, ptr {ents}, i64 {at}, i32 1"
                    ));
                    let got = self.tmp();
                    self.emit(&format!("{got} = load {vty}, ptr {vp}"));
                    self.emit(&format!("store {vty} {got}, ptr {slot}"));
                    self.br(&after);
                    self.place(&after);
                    let out = self.tmp();
                    self.emit(&format!("{out} = load {vty}, ptr {slot}"));
                    let ok1 = self.both_ok(&b.ok, &i.ok);
                    let ok = self.both_ok(&ok1, &hit);
                    return Ok(Some(Val { ok, v: out, ty: vt }));
                }
                // **写像は添字ではなく鍵で引く**
                if is_map(&b.ty) {
                    let got = self.map_get(&b.v.clone(), &b.ty.clone(), i.clone(), e.span)?;
                    let ok1 = self.both_ok(&b.ok, &i.ok);
                    let ok = self.both_ok(&ok1, &got.ok);
                    return Ok(Some(Val { ok, v: got.v, ty: got.ty }));
                }
                let idx = self.widen64(&i);
                let (inb, safe) = self.bounds(&b.v.clone(), &idx);
                let g = self.elem_ptr(&b.v.clone(), &safe, &b.ty.clone());
                let el = elem_of(&b.ty).unwrap_or(ValueType::I64);
                let t = self.tmp();
                self.emit(&format!("{t} = load {}, ptr {g}", ity(&el)));
                let ok1 = self.both_ok(&b.ok, &i.ok);
                let ok = self.both_ok(&ok1, &inb);
                Ok(Some(Val { ok, v: t, ty: el }))
            }

            E::Name(n) => {
                let Some((p, ty)) = self.addr(n) else {
                    return err(format!("知らない名前 `{n}`"), e.span);
                };
                let t = self.tmp();
                self.emit(&format!("{t} = load {}, ptr {p}", ity(&ty)));
                Ok(Some(Val { ok: "true".into(), v: t, ty }))
            }

            // **`;` は領域を潰し、内面を空にする**（C-22）。左辺は無くてもよい（C-80）
            E::Discard(inner) => {
                if let Some(i) = inner {
                    self.expr(i)?;
                }
                Ok(None)
            }

            E::Paren(items) => self.region(items),

            // **裸のブロックは領域・スコープ・脱出段の三つを作る**（C-20）
            E::Block(items) => {
                self.push_scope();
                self.open_stage(None, false);
                let r = self.region(items)?;
                // 中身が残っていれば、それが段の値
                if let Some(v) = r {
                    self.leave(1, Some(v))?;
                } else {
                    self.leave(1, None)?;
                }
                let v = self.close_stage();
                self.pop_scope();
                Ok(Some(v))
            }

            E::Unary { op, rhs } => {
                let r = self.expr(rhs)?;
                let Some(r) = r else { return err("被演算子に値が無い", e.span) };
                match op {
                    UnOp::Pos => Ok(Some(r)),
                    UnOp::Neg => {
                        let t = self.tmp();
                        if is_float(&r.ty) {
                            self.emit(&format!("{t} = fneg {} {}", ity(&r.ty), r.v));
                        } else {
                            let w = width(&r.ty).unwrap_or(64);
                            self.emit(&format!("{t} = sub i{w} 0, {}", r.v));
                        }
                        Ok(Some(Val { ok: r.ok, v: t, ty: r.ty }))
                    }
                    // **`u1` なら論理否定、それ以外はビット反転。**
                    // 参照実装がそうしている——`! 0` は `i64` で −1 である
                    UnOp::Not => {
                        let w = width(&r.ty).unwrap_or(64);
                        let t = self.tmp();
                        if r.ty == ValueType::U1 {
                            self.emit(&format!("{t} = xor i1 {}, true", r.v));
                        } else {
                            self.emit(&format!("{t} = xor i{w} {}, -1", r.v));
                        }
                        Ok(Some(Val { ok: r.ok, v: t, ty: r.ty }))
                    }
                }
            }

            E::Binary { op, lhs, rhs } => {
                // **`??` は右を評価しないことがある**——左が値なら右は走らない
                if *op == BinOp::Coalesce {
                    return self.coalesce(lhs, rhs, e.span).map(Some);
                }
                // **`|>` は構文の水準の糖衣**（C-15）。`x |> f(a)` は `f(x, a)`
                if *op == BinOp::Feed {
                    let ExprKind::Call { callee, args } = &rhs.kind else {
                        return err("`|>` の右は呼び出しでなければならない", rhs.span);
                    };
                    let mut all = vec![(**lhs).clone()];
                    all.extend(args.iter().cloned());
                    return self.call(callee, &all, e.span);
                }
                let l = self.expr(lhs)?;
                let r = self.expr(rhs)?;
                let (Some(l), Some(r)) = (l, r) else {
                    return err("被演算子に値が無い", e.span);
                };
                self.binary(*op, l, r, e.span).map(Some)
            }

            E::Decl(d) => {
                for b in &d.bindings {
                    // **別名は値を作らない。** 既にある枠を指すだけ（C-53）
                    if let BindInit::AliasOf(target) = &b.init {
                        let Some((q, tty)) = self.addr(target) else {
                            return err(format!("知らない名前 `{target}`"), b.span);
                        };
                        // **注釈があっても置き場は変えない。** 包みを剥がすだけ（S-2）
                        let ty = match b.ty.as_ref() {
                            Some(t) => self.resolve(&t.value),
                            None => tty,
                        };
                        self.declare_alias(&b.name, &q, ty);
                        continue;
                    }
                    let BindInit::Value(init) = &b.init else {
                        return err("`&=` は STEEL がまだ扱えない", b.span);
                    };
                    // **注釈があるなら、それが求める型である**（C-94）
                    let saved = self.want.take();
                    self.want = b.ty.as_ref().map(|t| self.resolve(&t.value));
                    let v = self.expr(init);
                    self.want = saved;
                    let v = v?;
                    let Some(v) = v else {
                        return err("束縛する値が無い", b.span);
                    };
                    // **注釈の包みを剥がす**（S-2）
                    let ty = match b.ty.as_ref() {
                        Some(t) => self.resolve(&t.value),
                        None => v.ty.clone(),
                    };
                    if width(&ty).is_none() && !is_float(&ty) && !is_heap(&ty) {
                        return err("STEEL はまだ数と集合体しか扱えない", b.span);
                    }
                    // **`:=` は深い複製である**（C-33）
                    let conv = if is_heap(&ty) {
                        self.deep_copy(&v.v.clone(), &ty)
                    } else {
                        self.conv(&v.v.clone(), &v.ty.clone(), &ty)
                    };
                    let p = self.declare(&b.name, ty.clone());
                    self.emit(&format!("store {} {conv}, ptr {p}", ity(&ty)));
                }
                // **宣言は値を置かない。** 外界面は paradox だが、`;` が潰す
                Ok(None)
            }

            E::Assign { op, lhs, rhs } => {
                // `p.欄 := v`
                if let ExprKind::Field { base, name } = &lhs.kind {
                    let b = self.expr(base)?;
                    let Some(b) = b else { return err("受け手に値が無い", base.span) };
                    let ValueType::Named(sname) = &b.ty else {
                        return err("欄を持つのは構造体だけ", base.span);
                    };
                    let sname = sname.clone();
                    let Some(fields) = self.fields_of(&sname) else {
                        return err(format!("知らない構造体 `{sname}`"), base.span);
                    };
                    let Some(i) = fields.iter().position(|(f, _)| f == name) else {
                        return err(format!("`{sname}` に欄 `{name}` は無い"), e.span);
                    };
                    let fty = fields[i].1.clone();
                    let r = self.expr(rhs)?;
                    let Some(r) = r else { return err("代入する値が無い", e.span) };
                    let g = self.field_ptr(&b.v.clone(), &sname, i)?;
                    // **場所は一度だけ数える。** 読み書きで別々に数えない
                    let c = self.rmw(*op, &g, &fty, r, e.span)?;
                    self.emit(&format!("store {} {c}, ptr {g}", ity(&fty)));
                    return Ok(None);
                }
                // `a[i] := v` — **範囲外なら何も起きない**（paradox）
                if let ExprKind::Index { base, index } = &lhs.kind {
                    let b = self.expr(base)?;
                    let i = self.expr(index)?;
                    let r = self.expr(rhs)?;
                    let (Some(b), Some(i), Some(r)) = (b, i, r) else {
                        return err("代入に値が無い", e.span);
                    };
                    if !is_heap(&b.ty) {
                        return err("添字を取れるのは集合体だけ", base.span);
                    }
                    // **hash も枠を持たない**
                    if is_hash(&b.ty) {
                        if *op != AssignOp::Set {
                            return err("hash への複合代入は STEEL がまだ扱えない", e.span);
                        }
                        let ValueType::Hash(kt, vt) = &b.ty else { unreachable!() };
                        let (kt, vt) = ((**kt).clone(), (**vt).clone());
                        let stem = self.hash_fns(&b.ty.clone())?;
                        let kc = if is_heap(&kt) {
                            self.deep_copy(&i.v.clone(), &kt)
                        } else {
                            self.conv(&i.v.clone(), &i.ty.clone(), &kt)
                        };
                        let vc = if is_heap(&vt) {
                            self.deep_copy(&r.v.clone(), &vt)
                        } else {
                            self.conv(&r.v.clone(), &r.ty.clone(), &vt)
                        };
                        self.emit(&format!(
                            "call void {stem}.put(ptr {}, {} {kc}, {} {vc})",
                            b.v,
                            ity(&kt),
                            ity(&vt)
                        ));
                        return Ok(None);
                    }
                    // **写像は枠を持たない。** 無い鍵は挿す
                    if is_map(&b.ty) {
                        if *op != AssignOp::Set {
                            return err("写像への複合代入は STEEL がまだ扱えない", e.span);
                        }
                        self.map_put(&b.v.clone(), &b.ty.clone(), i, r, e.span)?;
                        return Ok(None);
                    }
                    let idx = self.widen64(&i);
                    let (inb, safe) = self.bounds(&b.v.clone(), &idx);
                    let g = self.elem_ptr(&b.v.clone(), &safe, &b.ty.clone());
                    let el = elem_of(&b.ty).unwrap_or(ValueType::I64);
                    // **範囲外への書き込みは誤りである。**
                    //
                    // 読みなら paradox でよい（「そこに値が無い」と言える）が、
                    // 書きは違う——**代入は元々 paradox を産む**ので、
                    // 「書けなかった」を paradox で表すと**書けた場合と区別がつかない。**
                    let good = self.label("store.ok");
                    let bad = self.label("store.bad");
                    self.cbr(&inb, &good, &bad);
                    self.place(&bad);
                    self.emit("call void @vaak.fail()");
                    self.emit("unreachable");
                    self.done = true;
                    self.place(&good);
                    // **集合体は自分の要素の型を知っている**（C-94）。
                    // 読み書きは範囲を確かめた後——**枠の外を読んでから足さない**
                    let c = self.rmw(*op, &g, &el, r, e.span)?;
                    self.emit(&format!("store {} {c}, ptr {g}", ity(&el)));
                    return Ok(None);
                }
                let ExprKind::Name(n) = &lhs.kind else {
                    return err("STEEL はまだ名前への代入しか扱えない", e.span);
                };
                // `x &= z` — **指し直す。** 値は動かない（C-53）
                if *op == AssignOp::Alias {
                    let Some((slot, _)) = self.alias_slot(n) else {
                        return err("指し直せるのは別名だけ", lhs.span);
                    };
                    let ExprKind::Name(t) = &rhs.kind else {
                        return err("`&=` の右は名前でなければならない", rhs.span);
                    };
                    let Some((q, _)) = self.addr(t) else {
                        return err(format!("知らない名前 `{t}`"), rhs.span);
                    };
                    self.emit(&format!("store ptr {q}, ptr {slot}"));
                    return Ok(None);
                }
                let Some((p, ty)) = self.addr(n) else {
                    return err(format!("知らない名前 `{n}`"), lhs.span);
                };
                let r = self.expr(rhs)?;
                let Some(r) = r else { return err("代入する値が無い", e.span) };
                let v = if *op == AssignOp::Set {
                    if is_heap(&ty) {
                        self.deep_copy(&r.v.clone(), &ty)
                    } else {
                        self.conv(&r.v.clone(), &r.ty.clone(), &ty)
                    }
                } else {
                    let cur = self.tmp();
                    self.emit(&format!("{cur} = load {}, ptr {p}", ity(&ty)));
                    let Some(b) = assign_binop(*op) else {
                        return err("扱えない代入", e.span);
                    };
                    let lv = Val { ok: "true".into(), v: cur, ty: ty.clone() };
                    let out = self.binary(b, lv, r, e.span)?;
                    out.v
                };
                self.emit(&format!("store {} {v}, ptr {p}", ity(&ty)));
                Ok(None)
            }

            // `( 鍵 => 値, … )` — **文脈の型が要る**（C-94）
            E::MapLit(pairs) => {
                let Some(t) = self.want.clone() else {
                    return err("写像リテラルには型が要る", e.span);
                };
                let t = self.resolve(&t);
                // **同じリテラルが両方に使える。** 文脈の型が決める（C-98）
                if is_hash(&t) {
                    let ValueType::Hash(kt, vt) = &t else { unreachable!() };
                    let (kt, vt) = ((**kt).clone(), (**vt).clone());
                    let stem = self.hash_fns(&t)?;
                    let h = self.tmp();
                    self.emit(&format!("{h} = call ptr @vaak.hash.new()"));
                    for (k, v) in pairs {
                        let kv = self.expr(k)?;
                        let vv = self.expr(v)?;
                        let (Some(kv), Some(vv)) = (kv, vv) else {
                            return err("hash の要素に値が無い", e.span);
                        };
                        let kc = if is_heap(&kt) {
                            self.deep_copy(&kv.v.clone(), &kt)
                        } else {
                            self.conv(&kv.v.clone(), &kv.ty.clone(), &kt)
                        };
                        let vc = if is_heap(&vt) {
                            self.deep_copy(&vv.v.clone(), &vt)
                        } else {
                            self.conv(&vv.v.clone(), &vv.ty.clone(), &vt)
                        };
                        self.emit(&format!(
                            "call void {stem}.put(ptr {h}, {} {kc}, {} {vc})",
                            ity(&kt),
                            ity(&vt)
                        ));
                    }
                    return Ok(Some(Val { ok: "true".into(), v: h, ty: t }));
                }
                if !is_map(&t) {
                    return err("写像リテラルに写像でない型が求められている", e.span);
                }
                let m = self.map_new();
                for (k, v) in pairs {
                    let kv = self.expr(k)?;
                    let vv = self.expr(v)?;
                    let (Some(kv), Some(vv)) = (kv, vv) else {
                        return err("写像の要素に値が無い", e.span);
                    };
                    self.map_put(&m, &t, kv, vv, e.span)?;
                }
                Ok(Some(Val { ok: "true".into(), v: m, ty: t }))
            }

            E::If(i) => self.if_expr(i, e.span).map(Some),
            E::Loop(body) => self.loop_expr(None, body, e.span).map(Some),
            E::While { cond, body } => self.loop_expr(Some(cond), body, e.span).map(Some),
            E::NFor { name, start, count, body } => {
                self.nfor(name, start, count, body, e.span).map(Some)
            }
            E::Switch { subject, arms } => self.switch(subject, arms, e.span).map(Some),

            E::Escape(x) => {
                self.escape(x)?;
                // **脱出の値は paradox**（C-43）。ここから先へは進まない
                Ok(Some(self.paradox(&ValueType::I64)))
            }

            // `E -> T` — **領域に型を付ける**（C-30）
            E::Ascribe { expr, ty } => {
                let v = self.expr(expr)?;
                let Some(v) = v else { return err("注釈する値が無い", e.span) };
                let t = self.resolve(&ty.value);
                if width(&t).is_none() && !is_float(&t) && !is_heap(&t) {
                    return err("STEEL はまだ数と集合体しか扱えない", e.span);
                }
                // **包みを剥がしても置き場は変わらない**（S-2）。数だけ幅を合わせる
                let c = if is_heap(&t) {
                    v.v.clone()
                } else {
                    self.conv(&v.v.clone(), &v.ty.clone(), &t)
                };
                Ok(Some(Val { ok: v.ok, v: c, ty: t }))
            }

            E::Call { callee, args } => self.call(callee, args, e.span),

            // **関数の宣言はここでは組まない。** 先に集めてある
            E::FlowDecl(d) => {
                self.flows.insert(d.name.clone(), (*d.body).clone());
                Ok(None)
            }
            E::FnDecl(_) | E::StructDecl(_) | E::WrapDecl(_) => Ok(None),
        }
    }

    /// 鍵の型ごとの二分探索。返すのは（置き場、見つかったか）。
    ///
    /// **見つからなくても置き場を返す**——そこへ挿せば順序が保たれる。
    /// 引くのと挿すのが同じ一度の探索で済む。
    fn map_find_fn(&mut self, kty: &ValueType) -> R<String> {
        let tag = type_tag(kty);
        let name = format!("@vaak.find.{tag}");
        if self.copy_fns.contains(&name) {
            return Ok(name);
        }
        self.copy_fns.push(name.clone());
        let kt = ity(kty);
        // 鍵を比べて -1／0／1 を出す
        let cmp = if matches!(kty, ValueType::Str) {
            "  %c = call i32 @vaak.strcmp(ptr %kv, ptr %k)\n".to_string()
        } else if is_float(kty) {
            if matches!(kty, ValueType::F80) {
                // **`f80` は鍵にできない。** 参照実装が持たない型なので、
                // 突き合わせる相手がいない（S-20）
                return err("STEEL は f80 を写像の鍵にできない", Span::default());
            }
            // **単調な写しにしてから比べる**（C-99）
            let widen = if matches!(kty, ValueType::F32) {
                "  %kvd = fpext float %kv to double
  %kd = fpext float %k to double
"
            } else {
                "  %kvd = fadd double %kv, 0.0
  %kd = fadd double %k, 0.0
"
            };
            format!(
                "{widen}  %ka = call i64 @vaak.fkey(double %kvd)
                   %kb = call i64 @vaak.fkey(double %kd)
                   %lt = icmp ult i64 %ka, %kb
                   %gt = icmp ugt i64 %ka, %kb
                   %a = select i1 %lt, i32 -1, i32 0
                   %c = select i1 %gt, i32 1, i32 %a
"
            )
        } else {
            let (lt, gt) = if signed(kty) { ("slt", "sgt") } else { ("ult", "ugt") };
            format!(
                "  %lt = icmp {lt} {kt} %kv, %k\n  \
                 %gt = icmp {gt} {kt} %kv, %k\n  \
                 %a = select i1 %lt, i32 -1, i32 0\n  \
                 %c = select i1 %gt, i32 1, i32 %a\n"
            )
        };
        let body = format!(
            "define internal {{ i64, i1 }} {name}(ptr %m, {kt} %k) {{\n\
             entry:\n  \
             %n = load i64, ptr %m\n  \
             %keys = call ptr @vaak.map.keys(ptr %m)\n  \
             br label %head\n\
             head:\n  \
             %lo = phi i64 [ 0, %entry ], [ %lo2, %step ]\n  \
             %hi = phi i64 [ %n, %entry ], [ %hi2, %step ]\n  \
             %go = icmp slt i64 %lo, %hi\n  \
             br i1 %go, label %body, label %miss\n\
             body:\n  \
             %sum = add i64 %lo, %hi\n  \
             %mid = lshr i64 %sum, 1\n  \
             %kp = getelementptr {kt}, ptr %keys, i64 %mid\n  \
             %kv = load {kt}, ptr %kp\n\
             {cmp}  \
             %eq = icmp eq i32 %c, 0\n  \
             br i1 %eq, label %hit, label %step\n\
             step:\n  \
             %less = icmp slt i32 %c, 0\n  \
             %mid1 = add i64 %mid, 1\n  \
             %lo2 = select i1 %less, i64 %mid1, i64 %lo\n  \
             %hi2 = select i1 %less, i64 %hi, i64 %mid\n  \
             br label %head\n\
             hit:\n  \
             %h1 = insertvalue {{ i64, i1 }} undef, i64 %mid, 0\n  \
             %h2 = insertvalue {{ i64, i1 }} %h1, i1 true, 1\n  \
             ret {{ i64, i1 }} %h2\n\
             miss:\n  \
             %m1 = insertvalue {{ i64, i1 }} undef, i64 %lo, 0\n  \
             %m2 = insertvalue {{ i64, i1 }} %m1, i1 false, 1\n  \
             ret {{ i64, i1 }} %m2\n\
             }}"
        );
        self.head_global(&body);
        Ok(name)
    }

    /// 零。**型ごとに書き方が違う**
    fn zero_of(&mut self, t: &ValueType) -> String {
        if is_heap(t) {
            "null".into()
        } else if matches!(t, ValueType::F80) {
            "0xK00000000000000000000".into()
        } else if is_float(t) {
            "0.0".into()
        } else {
            "0".into()
        }
    }

    fn hash_entries(&mut self, h: &str) -> String {
        let g = self.tmp();
        self.emit(&format!("{g} = getelementptr i8, ptr {h}, i64 24"));
        let e = self.tmp();
        self.emit(&format!("{e} = load ptr, ptr {g}"));
        e
    }

    fn hash_ent_ty(&mut self, ht: &ValueType) -> String {
        let ValueType::Hash(k, v) = ht else { return "%err".into() };
        format!("%hent.{}_{}", type_tag(k), type_tag(v))
    }

    /// `hash` の一式を出す（C-98）。**鍵と値の組ごとに一度だけ。**
    ///
    /// 連鎖法である。開番地法より**抜くのが素直**で、
    /// 並びが密なので入れた順もそのまま取れる。
    fn hash_fns(&mut self, ht: &ValueType) -> R<String> {
        let ValueType::Hash(kt, vt) = ht else {
            return err("hash ではない", Span::default());
        };
        let (kt, vt) = ((**kt).clone(), (**vt).clone());
        let tag = format!("{}_{}", type_tag(&kt), type_tag(&vt));
        let stem = format!("@vaak.h.{tag}");
        if self.copy_fns.contains(&stem) {
            return Ok(stem);
        }
        self.copy_fns.push(stem.clone());
        let (ki, vi) = (ity(&kt), ity(&vt));
        let ent = format!("%hent.{tag}");
        // **並べ方は LLVM に決めさせる。** 揃えを自分で数えない
        self.head_global(&format!("{ent} = type {{ {ki}, {vi}, i64, i8 }}"));

        // 鍵を数にする／比べる
        let (hash_call, eq) = if matches!(kt, ValueType::Str) {
            (
                "  %hv = call i64 @vaak.hash.str(ptr %k)\n".to_string(),
                "  %c = call i32 @vaak.strcmp(ptr %ek, ptr %k)\n  %same = icmp eq i32 %c, 0\n"
                    .to_string(),
            )
        } else if is_float(&kt) {
            if matches!(kt, ValueType::F80) {
                return err("STEEL は f80 を hash の鍵にできない", Span::default());
            }
            // **同じ単調な写しを使う**（C-99）。`map` と鍵の同一性が揃う
            let ext = if matches!(kt, ValueType::F32) {
                ("fpext float %k to double", "fpext float %ek to double")
            } else {
                ("fadd double %k, 0.0", "fadd double %ek, 0.0")
            };
            (
                format!("  %kd = {}
  %kw = call i64 @vaak.fkey(double %kd)
                           %hv = call i64 @vaak.hash.i64(i64 %kw)
", ext.0),
                format!("  %ekd = {}
  %eka = call i64 @vaak.fkey(double %ekd)
                           %kd2 = {}
  %ekb = call i64 @vaak.fkey(double %kd2)
                           %same = icmp eq i64 %eka, %ekb
", ext.1, ext.0.replace("%k,", "%k,")),
            )
        } else {
            let w = width(&kt).unwrap_or(64);
            let widen = if w >= 64 {
                "  %kw = bitcast i64 %k to i64\n".to_string()
            } else if signed(&kt) {
                format!("  %kw = sext {ki} %k to i64\n")
            } else {
                format!("  %kw = zext {ki} %k to i64\n")
            };
            (
                format!("{widen}  %hv = call i64 @vaak.hash.i64(i64 %kw)\n"),
                format!("  %same = icmp eq {ki} %ek, %k\n"),
            )
        };

        // 引く：要素の位置か -1
        self.head_global(&format!(
            "define internal i64 {stem}.find(ptr %h, {ki} %k) {{\n\
             entry:\n  \
             %nbg = getelementptr i8, ptr %h, i64 40\n  \
             %nbk = load i64, ptr %nbg\n  \
             %empty = icmp eq i64 %nbk, 0\n  \
             br i1 %empty, label %none, label %go\n\
             go:\n\
             {hash_call}  \
             %mask = sub i64 %nbk, 1\n  \
             %slot = and i64 %hv, %mask\n  \
             %bg = getelementptr i8, ptr %h, i64 32\n  \
             %buk = load ptr, ptr %bg\n  \
             %sp = getelementptr i64, ptr %buk, i64 %slot\n  \
             %first = load i64, ptr %sp\n  \
             %eg = getelementptr i8, ptr %h, i64 24\n  \
             %ents = load ptr, ptr %eg\n  \
             br label %walk\n\
             walk:\n  \
             %i = phi i64 [ %first, %go ], [ %next, %step ]\n  \
             %end = icmp slt i64 %i, 0\n  \
             br i1 %end, label %none, label %look\n\
             look:\n  \
             %ekp = getelementptr {ent}, ptr %ents, i64 %i, i32 0\n  \
             %ek = load {ki}, ptr %ekp\n\
             {eq}  \
             br i1 %same, label %hit, label %step\n\
             step:\n  \
             %np = getelementptr {ent}, ptr %ents, i64 %i, i32 2\n  \
             %next = load i64, ptr %np\n  \
             br label %walk\n\
             hit:\n  ret i64 %i\n\
             none:\n  ret i64 -1\n\
             }}"
        ));

        // 大きくする：並びを倍にし、bucket を組み直す
        self.head_global(&format!(
            "define internal void {stem}.grow(ptr %h) {{\n\
             entry:\n  \
             %cg = getelementptr i8, ptr %h, i64 16\n  \
             %cap = load i64, ptr %cg\n  \
             %tw = shl i64 %cap, 1\n  \
             %sm = icmp slt i64 %tw, 8\n  \
             %cap2 = select i1 %sm, i64 8, i64 %tw\n  \
             %sz = getelementptr {ent}, ptr null, i64 1\n  \
             %esz = ptrtoint ptr %sz to i64\n  \
             %bytes = mul i64 %cap2, %esz\n  \
             %ne = call ptr @vaak.alloc(i64 %bytes)\n  \
             %eg = getelementptr i8, ptr %h, i64 24\n  \
             %oe = load ptr, ptr %eg\n  \
             %ng = getelementptr i8, ptr %h, i64 8\n  \
             %n = load i64, ptr %ng\n  \
             %ob = mul i64 %n, %esz\n  \
             %has = icmp sgt i64 %n, 0\n  \
             br i1 %has, label %cp, label %tab\n\
             cp:\n  \
             call void @llvm.memcpy.p0.p0.i64(ptr %ne, ptr %oe, i64 %ob, i1 false)\n  \
             br label %tab\n\
             tab:\n  \
             store ptr %ne, ptr %eg\n  \
             store i64 %cap2, ptr %cg\n  \
             %nbk = shl i64 %cap2, 1\n  \
             %bb = mul i64 %nbk, 8\n  \
             %nb = call ptr @vaak.alloc(i64 %bb)\n  \
             %bg = getelementptr i8, ptr %h, i64 32\n  \
             store ptr %nb, ptr %bg\n  \
             %kg = getelementptr i8, ptr %h, i64 40\n  \
             store i64 %nbk, ptr %kg\n  \
             br label %clr\n\
             clr:\n  \
             %ci = phi i64 [ 0, %tab ], [ %ci2, %clrb ]\n  \
             %cgo = icmp slt i64 %ci, %nbk\n  \
             br i1 %cgo, label %clrb, label %relink\n\
             clrb:\n  \
             %cp2 = getelementptr i64, ptr %nb, i64 %ci\n  \
             store i64 -1, ptr %cp2\n  \
             %ci2 = add i64 %ci, 1\n  \
             br label %clr\n\
             relink:\n  \
             %ri = phi i64 [ 0, %clr ], [ %ri2, %rnext ]\n  \
             %rgo = icmp slt i64 %ri, %n\n  \
             br i1 %rgo, label %rbody, label %rdone\n\
             rbody:\n  \
             %lp = getelementptr {ent}, ptr %ne, i64 %ri, i32 3\n  \
             %lv = load i8, ptr %lp\n  \
             %alive = icmp ne i8 %lv, 0\n  \
             br i1 %alive, label %rlink, label %rnext\n\
             rlink:\n  \
             %rkp = getelementptr {ent}, ptr %ne, i64 %ri, i32 0\n  \
             %k = load {ki}, ptr %rkp\n\
             {hash_call}  \
             %rmask = sub i64 %nbk, 1\n  \
             %rslot = and i64 %hv, %rmask\n  \
             %rsp = getelementptr i64, ptr %nb, i64 %rslot\n  \
             %head = load i64, ptr %rsp\n  \
             %rnp = getelementptr {ent}, ptr %ne, i64 %ri, i32 2\n  \
             store i64 %head, ptr %rnp\n  \
             store i64 %ri, ptr %rsp\n  \
             br label %rnext\n\
             rnext:\n  \
             %ri2 = add i64 %ri, 1\n  \
             br label %relink\n\
             rdone:\n  ret void\n\
             }}"
        ));

        // 入れる：あれば値を替え、無ければ末尾へ足して鎖に繋ぐ
        self.head_global(&format!(
            "define internal void {stem}.put(ptr %h, {ki} %k, {vi} %v) {{\n\
             entry:\n  \
             %at = call i64 {stem}.find(ptr %h, {ki} %k)\n  \
             %miss = icmp slt i64 %at, 0\n  \
             br i1 %miss, label %new, label %set\n\
             set:\n  \
             %eg0 = getelementptr i8, ptr %h, i64 24\n  \
             %e0 = load ptr, ptr %eg0\n  \
             %vp0 = getelementptr {ent}, ptr %e0, i64 %at, i32 1\n  \
             store {vi} %v, ptr %vp0\n  \
             ret void\n\
             new:\n  \
             %ng = getelementptr i8, ptr %h, i64 8\n  \
             %n = load i64, ptr %ng\n  \
             %cg = getelementptr i8, ptr %h, i64 16\n  \
             %cap = load i64, ptr %cg\n  \
             %full = icmp sge i64 %n, %cap\n  \
             br i1 %full, label %big, label %ok\n\
             big:\n  \
             call void {stem}.grow(ptr %h)\n  \
             br label %ok\n\
             ok:\n  \
             %eg = getelementptr i8, ptr %h, i64 24\n  \
             %ents = load ptr, ptr %eg\n  \
             %kp = getelementptr {ent}, ptr %ents, i64 %n, i32 0\n  \
             store {ki} %k, ptr %kp\n  \
             %vp = getelementptr {ent}, ptr %ents, i64 %n, i32 1\n  \
             store {vi} %v, ptr %vp\n  \
             %lp = getelementptr {ent}, ptr %ents, i64 %n, i32 3\n  \
             store i8 1, ptr %lp\n  \
             %kg = getelementptr i8, ptr %h, i64 40\n  \
             %nbk = load i64, ptr %kg\n\
             {hash_call}  \
             %mask = sub i64 %nbk, 1\n  \
             %slot = and i64 %hv, %mask\n  \
             %bg = getelementptr i8, ptr %h, i64 32\n  \
             %buk = load ptr, ptr %bg\n  \
             %sp = getelementptr i64, ptr %buk, i64 %slot\n  \
             %head = load i64, ptr %sp\n  \
             %np = getelementptr {ent}, ptr %ents, i64 %n, i32 2\n  \
             store i64 %head, ptr %np\n  \
             store i64 %n, ptr %sp\n  \
             %n1 = add i64 %n, 1\n  \
             store i64 %n1, ptr %ng\n  \
             %live = load i64, ptr %h\n  \
             %live1 = add i64 %live, 1\n  \
             store i64 %live1, ptr %h\n  \
             ret void\n\
             }}"
        ));

        // 抜く：鎖から外して穴にする
        self.head_global(&format!(
            "define internal {{ {vi}, i1 }} {stem}.del(ptr %h, {ki} %k) {{\n\
             entry:\n  \
             %at = call i64 {stem}.find(ptr %h, {ki} %k)\n  \
             %miss = icmp slt i64 %at, 0\n  \
             br i1 %miss, label %none, label %kill\n\
             kill:\n  \
             %eg = getelementptr i8, ptr %h, i64 24\n  \
             %ents = load ptr, ptr %eg\n  \
             %vp = getelementptr {ent}, ptr %ents, i64 %at, i32 1\n  \
             %v = load {vi}, ptr %vp\n  \
             %lp = getelementptr {ent}, ptr %ents, i64 %at, i32 3\n  \
             store i8 0, ptr %lp\n  \
             %kg = getelementptr i8, ptr %h, i64 40\n  \
             %nbk = load i64, ptr %kg\n\
             {hash_call}  \
             %mask = sub i64 %nbk, 1\n  \
             %slot = and i64 %hv, %mask\n  \
             %bg = getelementptr i8, ptr %h, i64 32\n  \
             %buk = load ptr, ptr %bg\n  \
             %sp = getelementptr i64, ptr %buk, i64 %slot\n  \
             %first = load i64, ptr %sp\n  \
             %isfirst = icmp eq i64 %first, %at\n  \
             br i1 %isfirst, label %unhead, label %walk\n\
             unhead:\n  \
             %mynp = getelementptr {ent}, ptr %ents, i64 %at, i32 2\n  \
             %mynext = load i64, ptr %mynp\n  \
             store i64 %mynext, ptr %sp\n  \
             br label %out\n\
             walk:\n  \
             %p = phi i64 [ %first, %kill ], [ %pn, %pstep ]\n  \
             %pend = icmp slt i64 %p, 0\n  \
             br i1 %pend, label %out, label %pcheck\n\
             pcheck:\n  \
             %pnp = getelementptr {ent}, ptr %ents, i64 %p, i32 2\n  \
             %pn = load i64, ptr %pnp\n  \
             %found = icmp eq i64 %pn, %at\n  \
             br i1 %found, label %unlink, label %pstep\n\
             unlink:\n  \
             %anp = getelementptr {ent}, ptr %ents, i64 %at, i32 2\n  \
             %an = load i64, ptr %anp\n  \
             store i64 %an, ptr %pnp\n  \
             br label %out\n\
             pstep:\n  \
             br label %walk\n\
             out:\n  \
             %live = load i64, ptr %h\n  \
             %live1 = sub i64 %live, 1\n  \
             store i64 %live1, ptr %h\n  \
             %r1 = insertvalue {{ {vi}, i1 }} undef, {vi} %v, 0\n  \
             %r2 = insertvalue {{ {vi}, i1 }} %r1, i1 true, 1\n  \
             ret {{ {vi}, i1 }} %r2\n\
             none:\n  \
             %z1 = insertvalue {{ {vi}, i1 }} undef, {vi} {zero}, 0\n  \
             %z2 = insertvalue {{ {vi}, i1 }} %z1, i1 false, 1\n  \
             ret {{ {vi}, i1 }} %z2\n\
             }}",
            zero = if is_heap(&vt) {
                "null".to_string()
            } else if is_float(&vt) {
                if matches!(vt, ValueType::F80) {
                    "0xK00000000000000000000".into()
                } else {
                    "0.0".into()
                }
            } else {
                "0".to_string()
            }
        ));
        Ok(stem)
    }

    /// 空の写像を作る。
    fn map_new(&mut self) -> String {
        let p = self.tmp();
        self.emit(&format!("{p} = call ptr @vaak.map.new()"));
        p
    }

    /// `m[k] := v` の中身。**引いた場所へ書くか、無ければそこへ挿す。**
    ///
    /// 探索は一度で済む——二分探索が「無いならどこへ挿すか」も返すからである。
    fn map_put(&mut self, m: &str, mt: &ValueType, k: Val, v: Val, span: Span) -> R<()> {
        let ValueType::Map(kt, vt) = mt else {
            return err("写像ではない", span);
        };
        let (kt, vt) = ((**kt).clone(), (**vt).clone());
        let (ks, vs) = (elem_size(&kt), elem_size(&vt));
        let kc = if is_heap(&kt) {
            self.deep_copy(&k.v.clone(), &kt)
        } else {
            self.conv(&k.v.clone(), &k.ty.clone(), &kt)
        };
        let vc = if is_heap(&vt) {
            self.deep_copy(&v.v.clone(), &vt)
        } else {
            self.conv(&v.v.clone(), &v.ty.clone(), &vt)
        };
        let f = self.map_find_fn(&kt)?;
        let r = self.tmp();
        self.emit(&format!("{r} = call {{ i64, i1 }} {f}(ptr {m}, {} {kc})", ity(&kt)));
        let at = self.tmp();
        self.emit(&format!("{at} = extractvalue {{ i64, i1 }} {r}, 0"));
        let hit = self.tmp();
        self.emit(&format!("{hit} = extractvalue {{ i64, i1 }} {r}, 1"));
        let ins = self.label("put.new");
        let after = self.label("put.set");
        self.cbr(&hit, &after, &ins);
        self.place(&ins);
        self.emit(&format!("call void @vaak.map.reserve(ptr {m}, i64 {ks}, i64 {vs})"));
        self.emit(&format!(
            "call void @vaak.map.open(ptr {m}, i64 {at}, i64 {ks}, i64 {vs})"
        ));
        // **鍵は挿すときだけ書く。** 既にあるなら等しいので上書きは無駄である
        let keys = self.tmp();
        self.emit(&format!("{keys} = call ptr @vaak.map.keys(ptr {m})"));
        let kp = self.tmp();
        self.emit(&format!("{kp} = getelementptr {}, ptr {keys}, i64 {at}", ity(&kt)));
        self.emit(&format!("store {} {kc}, ptr {kp}", ity(&kt)));
        self.br(&after);
        self.place(&after);
        let vals = self.tmp();
        self.emit(&format!("{vals} = call ptr @vaak.map.vals(ptr {m})"));
        let vp = self.tmp();
        self.emit(&format!("{vp} = getelementptr {}, ptr {vals}, i64 {at}", ity(&vt)));
        self.emit(&format!("store {} {vc}, ptr {vp}", ity(&vt)));
        Ok(())
    }

    /// `m[k]` — **無ければ paradox。**
    fn map_get(&mut self, m: &str, mt: &ValueType, k: Val, span: Span) -> R<Val> {
        let ValueType::Map(kt, vt) = mt else {
            return err("写像ではない", span);
        };
        let (kt, vt) = ((**kt).clone(), (**vt).clone());
        let kc = self.conv(&k.v.clone(), &k.ty.clone(), &kt);
        let f = self.map_find_fn(&kt)?;
        let r = self.tmp();
        self.emit(&format!("{r} = call {{ i64, i1 }} {f}(ptr {m}, {} {kc})", ity(&kt)));
        let at = self.tmp();
        self.emit(&format!("{at} = extractvalue {{ i64, i1 }} {r}, 0"));
        let hit = self.tmp();
        self.emit(&format!("{hit} = extractvalue {{ i64, i1 }} {r}, 1"));
        // **見つからなければ読まない。** 空の写像では並びがまだ無い（null）ので、
        // 添字を丸めて読む手は使えない。枝を分ける
        let vty = ity(&vt);
        let slot = self.alloca(&vty);
        let zero = if is_float(&vt) {
            self.conv("0", &ValueType::I64, &vt)
        } else if is_heap(&vt) {
            "null".into()
        } else {
            "0".into()
        };
        self.emit(&format!("store {vty} {zero}, ptr {slot}"));
        let take = self.label("get.hit");
        let after = self.label("get.after");
        self.cbr(&hit, &take, &after);
        self.place(&take);
        let vals = self.tmp();
        self.emit(&format!("{vals} = call ptr @vaak.map.vals(ptr {m})"));
        let vp = self.tmp();
        self.emit(&format!("{vp} = getelementptr {vty}, ptr {vals}, i64 {at}"));
        let got = self.tmp();
        self.emit(&format!("{got} = load {vty}, ptr {vp}"));
        self.emit(&format!("store {vty} {got}, ptr {slot}"));
        self.br(&after);
        self.place(&after);
        let out = self.tmp();
        self.emit(&format!("{out} = load {vty}, ptr {slot}"));
        Ok(Val { ok: hit, v: out, ty: vt })
    }

    /// 場所への**読み・演算・書き**。`:=` なら読まずに書く。
    ///
    /// **場所は呼び手が一度だけ数えてから渡す。** 添字も欄も、
    /// 読みと書きで別々に数えると**二度目に違う場所を指しうる**。
    fn rmw(&mut self, op: AssignOp, ptr: &str, ty: &ValueType, r: Val, span: Span) -> R<String> {
        if op == AssignOp::Set {
            // **`:=` は深い複製である**（C-33）
            return Ok(if is_heap(ty) {
                self.deep_copy(&r.v.clone(), ty)
            } else {
                self.conv(&r.v.clone(), &r.ty.clone(), ty)
            });
        }
        let Some(b) = assign_binop(op) else {
            return err("扱えない代入", span);
        };
        if is_heap(ty) {
            return err("集合体には複合代入できない", span);
        }
        let cur = self.tmp();
        self.emit(&format!("{cur} = load {}, ptr {ptr}", ity(ty)));
        let lv = Val { ok: "true".into(), v: cur, ty: ty.clone() };
        Ok(self.binary(b, lv, r, span)?.v)
    }

    /// `??` — **左が値なら右は走らない。**
    fn coalesce(&mut self, lhs: &Expr, rhs: &Expr, span: Span) -> R<Val> {
        let l = self.expr(lhs)?;
        let Some(l) = l else { return err("`??` の左に領域が無い", span) };
        let slot = self.alloca("i64");
        let ok_slot = self.alloca("i1");
        let use_r = self.label("qq.right");
        let done = self.label("qq.done");

        let w = self.to_slot(&l);
        self.emit(&format!("store i64 {w}, ptr {slot}"));
        self.emit(&format!("store i1 {}, ptr {ok_slot}", l.ok));
        self.cbr(&l.ok.clone(), &done, &use_r);

        self.place(&use_r);
        let r = self.expr(rhs)?;
        let ty = match r {
            Some(r) => {
                let w = self.to_slot(&r);
                self.emit(&format!("store i64 {w}, ptr {slot}"));
                self.emit(&format!("store i1 {}, ptr {ok_slot}", r.ok));
                r.ty
            }
            None => {
                self.emit(&format!("store i1 false, ptr {ok_slot}"));
                l.ty.clone()
            }
        };
        self.br(&done);

        self.place(&done);
        let ok = self.tmp();
        self.emit(&format!("{ok} = load i1, ptr {ok_slot}"));
        let raw = self.tmp();
        self.emit(&format!("{raw} = load i64, ptr {slot}"));
        let v = self.from_slot(&raw, &ty.clone());
        Ok(Val { ok, v, ty })
    }
}

// ================= 制御構造 =================

impl Steel {
    /// `if` — **分岐は被演算子位置なので領域だが、スコープでも脱出段でもない**（C-20）。
    fn if_expr(&mut self, i: &If, _span: Span) -> R<Val> {
        let slot = self.alloca("i64");
        let ok_slot = self.alloca("i1");
        let done = self.label("if.done");
        self.emit(&format!("store i1 false, ptr {ok_slot}"));
        self.emit(&format!("store i64 0, ptr {slot}"));
        let mut ty = ValueType::I64;

        for (cond, body) in &i.arms {
            let c = self.expr(cond)?;
            let Some(c) = c else { return err("`if` の条件に値が無い", cond.span) };
            let b = self.nonzero(&c);
            let yes = self.label("if.yes");
            let no = self.label("if.no");
            self.cbr(&b, &yes, &no);
            self.place(&yes);
            if let Some(v) = self.expr(body)? {
                ty = v.ty.clone();
                let w = self.to_slot(&v);
                self.emit(&format!("store i64 {w}, ptr {slot}"));
                self.emit(&format!("store i1 {}, ptr {ok_slot}", v.ok));
            }
            self.br(&done);
            self.place(&no);
        }

        // **`else` が無ければ paradox**（C-14 規則2）
        if let Some(els) = &i.els {
            if let Some(v) = self.expr(els)? {
                ty = v.ty.clone();
                let w = self.to_slot(&v);
                self.emit(&format!("store i64 {w}, ptr {slot}"));
                self.emit(&format!("store i1 {}, ptr {ok_slot}", v.ok));
            }
        }
        self.br(&done);

        self.place(&done);
        let ok = self.tmp();
        self.emit(&format!("{ok} = load i1, ptr {ok_slot}"));
        let raw = self.tmp();
        self.emit(&format!("{raw} = load i64, ptr {slot}"));
        let v = self.from_slot(&raw, &ty.clone());
        Ok(Val { ok, v, ty })
    }

    /// `loop` / `while` — **値は反復回数**（C-3）。
    /// **本体はその構文の段**であって、二重にはならない（C-64）。
    fn loop_expr(&mut self, cond: Option<&Expr>, body: &Expr, span: Span) -> R<Val> {
        let head = self.label("loop.head");
        let cont = self.label("loop.cont");
        self.br(&head);
        self.place(&head);

        let idx = self.open_stage(Some(cont.clone()), true);
        let count_slot = self.stages[idx].count_slot.clone().unwrap();
        // **段の入れ物は毎周作らない。** 入口で一度だけ初期化してある
        let top = self.label("loop.top");
        self.br(&top);
        self.place(&top);

        if let Some(c) = cond {
            let cv = self.expr(c)?;
            let Some(cv) = cv else { return err("`while` の条件に値が無い", c.span) };
            let b = self.nonzero(&cv);
            let go = self.label("loop.go");
            let out = self.label("loop.out");
            self.cbr(&b, &go, &out);
            self.place(&out);
            // 条件で終わったときは**回数が値**
            let n = self.tmp();
            self.emit(&format!("{n} = load i64, ptr {count_slot}"));
            self.leave(1, Some(Val { ok: "true".into(), v: n, ty: ValueType::I64 }))?;
            self.place(&go);
        }

        self.push_scope();
        let ExprKind::Block(items) = &body.kind else {
            return err("ループの本体はブロックでなければならない", span);
        };
        let r = self.region(items)?;
        // **本体に値が残ってはいけない**（C-3）。検査器が捕らえているので、ここでは捨てる
        let _ = r;
        self.pop_scope();

        self.br(&cont);
        self.place(&cont.clone());
        let n = self.tmp();
        self.emit(&format!("{n} = load i64, ptr {count_slot}"));
        let n2 = self.tmp();
        self.emit(&format!("{n2} = add i64 {n}, 1"));
        self.emit(&format!("store i64 {n2}, ptr {count_slot}"));
        self.br(&top);

        Ok(self.close_stage())
    }

    /// `nfor (名前, 開始, 回数) { … }` — **値は反復回数。**
    fn nfor(&mut self, name: &str, start: &Expr, count: &Expr, body: &Expr, span: Span) -> R<Val> {
        let s = self.expr(start)?;
        let c = self.expr(count)?;
        let (Some(s), Some(c)) = (s, c) else {
            return err("`nfor` の開始と回数に値が要る", span);
        };
        let sv = self.widen64(&s);
        let cv = self.widen64(&c);
        let iv = self.alloca("i64");
        self.emit(&format!("store i64 {sv}, ptr {iv}"));
        let limit = self.alloca("i64");
        self.emit(&format!("store i64 {cv}, ptr {limit}"));

        let cont = self.label("nfor.cont");
        let idx = self.open_stage(Some(cont.clone()), true);
        let count_slot = self.stages[idx].count_slot.clone().unwrap();
        let top = self.label("nfor.top");
        self.br(&top);
        self.place(&top);

        let n = self.tmp();
        self.emit(&format!("{n} = load i64, ptr {count_slot}"));
        let lm = self.tmp();
        self.emit(&format!("{lm} = load i64, ptr {limit}"));
        let go = self.tmp();
        self.emit(&format!("{go} = icmp slt i64 {n}, {lm}"));
        let run = self.label("nfor.run");
        let out = self.label("nfor.out");
        self.cbr(&go, &run, &out);

        self.place(&out);
        let fin = self.tmp();
        self.emit(&format!("{fin} = load i64, ptr {count_slot}"));
        self.leave(1, Some(Val { ok: "true".into(), v: fin, ty: ValueType::I64 }))?;

        self.place(&run);
        self.push_scope();
        // **束縛は本体のスコープに入る。** 一周ごとに新しい値
        let p = self.declare(name, ValueType::I64);
        let cur = self.tmp();
        self.emit(&format!("{cur} = load i64, ptr {iv}"));
        self.emit(&format!("store i64 {cur}, ptr {p}"));
        let ExprKind::Block(items) = &body.kind else {
            return err("`nfor` の本体はブロックでなければならない", span);
        };
        let _ = self.region(items)?;
        self.pop_scope();

        self.br(&cont);
        self.place(&cont.clone());
        let n2 = self.tmp();
        self.emit(&format!("{n2} = load i64, ptr {count_slot}"));
        let n3 = self.tmp();
        self.emit(&format!("{n3} = add i64 {n2}, 1"));
        self.emit(&format!("store i64 {n3}, ptr {count_slot}"));
        let i2 = self.tmp();
        self.emit(&format!("{i2} = load i64, ptr {iv}"));
        let i3 = self.tmp();
        self.emit(&format!("{i3} = add i64 {i2}, 1"));
        self.emit(&format!("store i64 {i3}, ptr {iv}"));
        self.br(&top);

        Ok(self.close_stage())
    }

    /// `switch` — **どの腕にも当たらなければ paradox**（C-39）。
    fn switch(&mut self, subject: &Expr, arms: &[Arm], span: Span) -> R<Val> {
        let s = self.expr(subject)?;
        let Some(s) = s else { return err("`switch` の主題に値が無い", span) };
        let slot = self.alloca("i64");
        let ok_slot = self.alloca("i1");
        self.emit(&format!("store i1 false, ptr {ok_slot}"));
        self.emit(&format!("store i64 0, ptr {slot}"));
        let done = self.label("sw.done");
        let mut ty = ValueType::I64;

        for a in arms {
            let p = self.expr(&a.pattern)?;
            let Some(p) = p else { return err("`case` に値が無い", a.span) };
            let eq = self.binary(BinOp::Eq, s.clone(), p, a.span)?;
            let hit = self.label("sw.hit");
            let next = self.label("sw.next");
            self.cbr(&eq.v.clone(), &hit, &next);
            self.place(&hit);
            if let Some(v) = self.expr(&a.value)? {
                ty = v.ty.clone();
                let w = self.to_slot(&v);
                self.emit(&format!("store i64 {w}, ptr {slot}"));
                self.emit(&format!("store i1 {}, ptr {ok_slot}", v.ok));
            }
            self.br(&done);
            self.place(&next);
        }
        self.br(&done);

        self.place(&done);
        let ok = self.tmp();
        self.emit(&format!("{ok} = load i1, ptr {ok_slot}"));
        let raw = self.tmp();
        self.emit(&format!("{raw} = load i64, ptr {slot}"));
        let v = self.from_slot(&raw, &ty.clone());
        Ok(Val { ok, v, ty })
    }

    /// 脱出。**段数が静的に分かっていれば `br` 一つになる。**
    fn escape(&mut self, x: &Escape) -> R<()> {
        let (depth, is_continue, payload) = self.plan(x)?;
        if is_continue {
            // **段を再開させる。** `break` の連なりの分だけ外へ出てから
            let idx = self
                .stages
                .len()
                .checked_sub(depth.max(1))
                .ok_or(SteelError { msg: "段が足りない".into(), span: x.span })?;
            let Some(cont) = self.stages[idx].cont.clone() else {
                return err("`continue` の抜けた先がループではない", x.span);
            };
            self.br(&cont);
            return Ok(());
        }
        let p = match payload {
            Some(e) => self.expr(&e)?,
            None => None,
        };
        self.leave(depth, p)
    }

    /// 脱出の**段数と積み荷を、組み立て時に決める。**
    ///
    /// `flow` の本体は**使用位置で読み直される**（C-15）ので、
    /// ここで展開する——`getdepth()` が使用位置の深さになるのはそのためである。
    ///
    /// 返すのは（段数, 再開か, 積み荷）。
    fn plan(&mut self, x: &Escape) -> R<(usize, bool, Option<Expr>)> {
        let inner = |me: &mut Self, x: &Escape| -> R<(usize, bool, Option<Expr>)> {
            match &x.operand {
                Some(Operand::Escape(i)) => me.plan(i),
                Some(Operand::Value(v)) => Ok((0, false, Some(v.clone()))),
                None => Ok((0, false, None)),
            }
        };
        match &x.kind {
            EscapeKind::Break { outward } => {
                if *outward {
                    return err("`outward` は STEEL がまだ扱えない", x.span);
                }
                let (d, c, p) = inner(self, x)?;
                Ok((d + 1, c, p))
            }
            EscapeKind::Continue => {
                let (d, c, p) = inner(self, x)?;
                if p.is_some() {
                    // `continue` は作用素式か虚無しか取らない（C-71）
                    return err("`continue` の被演算子は作用素式か虚無だけ", x.span);
                }
                let _ = c;
                Ok((d, true, None))
            }
            EscapeKind::Flow { name, args } => {
                if name == "$repeat" {
                    return self.plan_repeat(args, x);
                }
                let Some(body) = self.flows.get(name).cloned() else {
                    return err(format!("知らない作用素式 `{name}`"), x.span);
                };
                // **本体を使用位置で読み直す。** 積み荷は使用位置のもの
                let (d, c, _) = self.plan(&body)?;
                let (_, _, p) = inner(self, x)?;
                Ok((d, c, p))
            }
        }
    }

    /// `$repeat(作用素, 回数)` — **回数が組み立て時に決まれば畳む。**
    fn plan_repeat(
        &mut self,
        args: &[FlowArg],
        x: &Escape,
    ) -> R<(usize, bool, Option<Expr>)> {
        let [FlowArg::Escape(op), FlowArg::Value(n)] = args else {
            return err("`$repeat` は作用素と回数を取る", x.span);
        };
        let Some(times) = self.const_int(n) else {
            // **動く段数は実行時に決まる**（C-34 の A）。STEEL はまだ持たない
            return err("`$repeat` の回数が組み立て時に決まらない", n.span);
        };
        if times < 0 {
            return err("`$repeat` の回数が負", n.span);
        }
        let (d, c, _) = self.plan(op)?;
        let (_, _, p) = match &x.operand {
            Some(Operand::Escape(i)) => self.plan(i)?,
            Some(Operand::Value(v)) => (0, false, Some(v.clone())),
            None => (0, false, None),
        };
        Ok((d * times as usize, c, p))
    }

    /// 式の指す**枠**（値そのものではない）。書き戻しに要る。
    fn slot(&mut self, e: &Expr) -> R<(String, ValueType)> {
        match &e.kind {
            ExprKind::Name(n) => self
                .addr(n)
                .ok_or(SteelError { msg: format!("知らない名前 `{n}`"), span: e.span }),
            ExprKind::Paren(v) if v.len() == 1 => self.slot(&v[0]),
            ExprKind::Field { base, name } => {
                let b = self.expr(base)?;
                let Some(b) = b else { return err("受け手に値が無い", base.span) };
                let ValueType::Named(sname) = &b.ty else {
                    return err("欄を持つのは構造体だけ", base.span);
                };
                let sname = sname.clone();
                let Some(fields) = self.fields_of(&sname) else {
                    return err(format!("知らない構造体 `{sname}`"), base.span);
                };
                let Some(i) = fields.iter().position(|(f, _)| f == name) else {
                    return err(format!("`{sname}` に欄 `{name}` は無い"), e.span);
                };
                let fty = fields[i].1.clone();
                let g = self.field_ptr(&b.v.clone(), &sname, i)?;
                Ok((g, fty))
            }
            _ => err("ここには名前か欄が要る", e.span),
        }
    }

    /// `a.push(v)` `a.pop()` `a.clear()` — **場所を書き換える。**
    fn mutating_method(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> R<Region> {
        let (slot, ty) = self.slot(base)?;
        if !is_heap(&ty) {
            return err("押し引きできるのは集合体だけ", base.span);
        }
        // **hash の `remove` も鍵で抜く**
        if is_hash(&ty) {
            let cur = self.tmp();
            self.emit(&format!("{cur} = load ptr, ptr {slot}"));
            if name == "clear" {
                // **生きている数と並びの長さを零に。** bucket は繋ぎ替えなくてよい
                self.emit(&format!("store i64 0, ptr {cur}"));
                let ng = self.tmp();
                self.emit(&format!("{ng} = getelementptr i8, ptr {cur}, i64 8"));
                self.emit(&format!("store i64 0, ptr {ng}"));
                let bg = self.tmp();
                self.emit(&format!("{bg} = getelementptr i8, ptr {cur}, i64 40"));
                self.emit(&format!("store i64 0, ptr {bg}"));
                return Ok(None);
            }
            if name != "remove" {
                return err(format!("`{name}` は hash に使えない"), span);
            }
            let ValueType::Hash(kt, vt) = &ty else { unreachable!() };
            let (kt, vt) = ((**kt).clone(), (**vt).clone());
            let Some(a) = args.first() else {
                return err("`remove` は鍵を一つ取る", span);
            };
            let k = self.expr(a)?;
            let Some(k) = k else { return err("鍵に値が無い", a.span) };
            let kc = self.conv(&k.v.clone(), &k.ty.clone(), &kt);
            let stem = self.hash_fns(&ty)?;
            let r = self.tmp();
            self.emit(&format!(
                "{r} = call {{ {}, i1 }} {stem}.del(ptr {cur}, {} {kc})",
                ity(&vt),
                ity(&kt)
            ));
            let v = self.tmp();
            self.emit(&format!("{v} = extractvalue {{ {}, i1 }} {r}, 0", ity(&vt)));
            let ok = self.tmp();
            self.emit(&format!("{ok} = extractvalue {{ {}, i1 }} {r}, 1", ity(&vt)));
            return Ok(Some(Val { ok, v, ty: vt }));
        }
        // **写像の `remove` は鍵で抜く。** 添字ではない
        if is_map(&ty) {
            let cur = self.tmp();
            self.emit(&format!("{cur} = load ptr, ptr {slot}"));
            if name == "clear" {
                self.emit(&format!("store i64 0, ptr {cur}"));
                return Ok(None);
            }
            if name != "remove" {
                return err(format!("`{name}` は写像に使えない"), span);
            }
            let ValueType::Map(kt, vt) = &ty else { unreachable!() };
            let (kt, vt) = ((**kt).clone(), (**vt).clone());
            let Some(a) = args.first() else {
                return err("`remove` は鍵を一つ取る", span);
            };
            let k = self.expr(a)?;
            let Some(k) = k else { return err("鍵に値が無い", a.span) };
            let kc = self.conv(&k.v.clone(), &k.ty.clone(), &kt);
            let f = self.map_find_fn(&kt)?;
            let r = self.tmp();
            self.emit(&format!(
                "{r} = call {{ i64, i1 }} {f}(ptr {cur}, {} {kc})",
                ity(&kt)
            ));
            let at = self.tmp();
            self.emit(&format!("{at} = extractvalue {{ i64, i1 }} {r}, 0"));
            let hit = self.tmp();
            self.emit(&format!("{hit} = extractvalue {{ i64, i1 }} {r}, 1"));
            let vty = ity(&vt);
            let out = self.alloca(&vty);
            let zero = if is_float(&vt) {
                self.conv("0", &ValueType::I64, &vt)
            } else if is_heap(&vt) {
                "null".into()
            } else {
                "0".into()
            };
            self.emit(&format!("store {vty} {zero}, ptr {out}"));
            let take = self.label("del.hit");
            let after = self.label("del.after");
            self.cbr(&hit, &take, &after);
            self.place(&take);
            // **詰める前に読む。** 後では消えている
            let vals = self.tmp();
            self.emit(&format!("{vals} = call ptr @vaak.map.vals(ptr {cur})"));
            let vp = self.tmp();
            self.emit(&format!("{vp} = getelementptr {vty}, ptr {vals}, i64 {at}"));
            let got = self.tmp();
            self.emit(&format!("{got} = load {vty}, ptr {vp}"));
            self.emit(&format!("store {vty} {got}, ptr {out}"));
            self.emit(&format!(
                "call void @vaak.map.close(ptr {cur}, i64 {at}, i64 {}, i64 {})",
                elem_size(&kt),
                elem_size(&vt)
            ));
            self.br(&after);
            self.place(&after);
            let v = self.tmp();
            self.emit(&format!("{v} = load {vty}, ptr {out}"));
            return Ok(Some(Val { ok: hit, v, ty: vt }));
        }
        let el = elem_of(&ty).unwrap_or(ValueType::I64);
        let es = elem_size(&el);
        let cur = self.tmp();
        self.emit(&format!("{cur} = load ptr, ptr {slot}"));
        match name {
            "clear" => {
                // **個数を零にするだけ。** 容量も置き場もそのまま
                self.emit(&format!("store i64 0, ptr {cur}"));
                Ok(None)
            }
            "push" => {
                let Some(a) = args.first() else {
                    return err("`push` は値を一つ取る", span);
                };
                let v = self.expr(a)?;
                let Some(v) = v else { return err("押す値が無い", a.span) };
                // **集合体は自分の要素の型を知っている**（C-94）
                let c = if is_heap(&el) {
                    self.deep_copy(&v.v.clone(), &el)
                } else {
                    self.conv(&v.v.clone(), &v.ty.clone(), &el)
                };
                let q = self.tmp();
                self.emit(&format!("{q} = call ptr @vaak.grow(ptr {cur}, i64 {es})"));
                self.emit(&format!("store ptr {q}, ptr {slot}"));
                let n = self.coll_len(&q);
                let last = self.tmp();
                self.emit(&format!("{last} = sub i64 {n}, 1"));
                let g = self.elem_ptr(&q, &last, &ty);
                self.emit(&format!("store {} {c}, ptr {g}", ity(&el)));
                // **押した結果は paradox**（C-33 と同じく、書きは値を置かない）
                Ok(None)
            }
            "pop" => {
                let n = self.coll_len(&cur);
                let some = self.tmp();
                self.emit(&format!("{some} = icmp sgt i64 {n}, 0"));
                let last = self.tmp();
                self.emit(&format!("{last} = sub i64 {n}, 1"));
                // **空なら触らない。** 添字も個数も零のまま
                let safe = self.tmp();
                self.emit(&format!("{safe} = select i1 {some}, i64 {last}, i64 0"));
                let g = self.elem_ptr(&cur, &safe, &ty);
                let out = self.tmp();
                self.emit(&format!("{out} = load {}, ptr {g}", ity(&el)));
                self.emit(&format!("store i64 {safe}, ptr {cur}"));
                Ok(Some(Val { ok: some, v: out, ty: el }))
            }
            // `a.insert(i, v)` — **枠の外なら何も起きない**（paradox）
            "insert" => {
                let (Some(ia), Some(va)) = (args.first(), args.get(1)) else {
                    return err("`insert` は添字と値を取る", span);
                };
                let i = self.expr(ia)?;
                let v = self.expr(va)?;
                let (Some(i), Some(v)) = (i, v) else {
                    return err("`insert` の引数に値が無い", span);
                };
                let idx = self.widen64(&i);
                let c = if is_heap(&el) {
                    self.deep_copy(&v.v.clone(), &el)
                } else {
                    self.conv(&v.v.clone(), &v.ty.clone(), &el)
                };
                let n = self.coll_len(&cur);
                // **末尾へ挿すのは許す**（`i == n`）。押すのと同じである
                let lo = self.tmp();
                self.emit(&format!("{lo} = icmp sge i64 {idx}, 0"));
                let hi = self.tmp();
                self.emit(&format!("{hi} = icmp sle i64 {idx}, {n}"));
                let ok = self.tmp();
                self.emit(&format!("{ok} = and i1 {lo}, {hi}"));
                let dothis = self.label("ins.do");
                let after = self.label("ins.after");
                self.cbr(&ok, &dothis, &after);
                self.place(&dothis);
                // **枠の中と分かってから伸ばす。** 外なら個数も置き場も動かさない
                let q = self.tmp();
                self.emit(&format!("{q} = call ptr @vaak.grow(ptr {cur}, i64 {es})"));
                self.emit(&format!("store ptr {q}, ptr {slot}"));
                let dst = self.tmp();
                self.emit(&format!("{dst} = add i64 {idx}, 1"));
                let from = self.elem_ptr(&q, &idx, &ty);
                let to = self.elem_ptr(&q, &dst, &ty);
                let cnt = self.tmp();
                self.emit(&format!("{cnt} = sub i64 {n}, {idx}"));
                let bytes = self.tmp();
                self.emit(&format!("{bytes} = mul i64 {cnt}, {es}"));
                // **重なるので memmove である。** memcpy では壊れる
                self.emit(&format!(
                    "call void @llvm.memmove.p0.p0.i64(ptr {to}, ptr {from}, i64 {bytes}, i1 false)"
                ));
                self.emit(&format!("store {} {c}, ptr {from}", ity(&el)));
                self.br(&after);
                self.place(&after);
                Ok(None)
            }

            // `a.remove(i)` — **抜いた値を返す。** 枠の外なら paradox
            "remove" => {
                let Some(ia) = args.first() else {
                    return err("`remove` は添字を一つ取る", span);
                };
                let i = self.expr(ia)?;
                let Some(i) = i else { return err("`remove` の添字に値が無い", ia.span) };
                let idx = self.widen64(&i);
                let n = self.coll_len(&cur);
                let (ok, safe) = self.bounds(&cur, &idx);
                // **ずらす前に読む。** 後では消えている
                let g = self.elem_ptr(&cur, &safe, &ty);
                let out = self.tmp();
                self.emit(&format!("{out} = load {}, ptr {g}", ity(&el)));
                let dothis = self.label("rm.do");
                let after = self.label("rm.after");
                self.cbr(&ok, &dothis, &after);
                self.place(&dothis);
                let src = self.tmp();
                self.emit(&format!("{src} = add i64 {safe}, 1"));
                let from = self.elem_ptr(&cur, &src, &ty);
                let cnt = self.tmp();
                self.emit(&format!("{cnt} = sub i64 {n}, {src}"));
                let bytes = self.tmp();
                self.emit(&format!("{bytes} = mul i64 {cnt}, {es}"));
                self.emit(&format!(
                    "call void @llvm.memmove.p0.p0.i64(ptr {g}, ptr {from}, i64 {bytes}, i1 false)"
                ));
                let less = self.tmp();
                self.emit(&format!("{less} = sub i64 {n}, 1"));
                self.emit(&format!("store i64 {less}, ptr {cur}"));
                self.br(&after);
                self.place(&after);
                Ok(Some(Val { ok, v: out, ty: el }))
            }

            _ => err(format!("`{name}` は STEEL がまだ扱えない"), span),
        }
    }

    /// 組み立て時に決まる整数か。**`getdepth()` はここで決まる**（C-23）。
    fn const_int(&self, e: &Expr) -> Option<i128> {
        match &e.kind {
            ExprKind::Int(t) => t.parse().ok(),
            ExprKind::Paren(v) if v.len() == 1 => self.const_int(&v[0]),
            ExprKind::Call { callee, args } if args.is_empty() => {
                match &callee.kind {
                    ExprKind::Name(n) if n == "getdepth" => Some(self.stages.len() as i128),
                    _ => None,
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let (a, b) = (self.const_int(lhs)?, self.const_int(rhs)?);
                match op {
                    BinOp::Add => Some(a + b),
                    BinOp::Sub => Some(a - b),
                    BinOp::Mul => Some(a * b),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

// ================= 関数とモジュール =================

impl Steel {
    fn call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> R<Region> {
        // **書き換えるメンバ関数は場所を要る。** 伸ばすと置き場が変わりうるので、
        // 新しい置き場を**元の枠へ書き戻さねばならない**
        if let ExprKind::Field { base, name } = &callee.kind {
            if matches!(name.as_str(), "push" | "pop" | "clear" | "insert" | "remove") {
                return self.mutating_method(base, name, args, span);
            }
        }
        // 集合体のメンバ関数
        if let ExprKind::Field { base, name } = &callee.kind {
            let b = self.expr(base)?;
            let Some(b) = b else { return err("受け手に値が無い", base.span) };
            if !is_heap(&b.ty) {
                return err("STEEL はまだ集合体のメンバ関数しか扱えない", span);
            }
            return match name.as_str() {
                "len" => {
                    let n = self.coll_len(&b.v.clone());
                    Ok(Some(Val { ok: b.ok, v: n, ty: ValueType::I64 }))
                }
                "has" if is_hash(&b.ty) => {
                    let ValueType::Hash(kt, _) = &b.ty else { unreachable!() };
                    let kt = (**kt).clone();
                    let Some(a) = args.first() else {
                        return err("`has` は鍵を一つ取る", span);
                    };
                    let k = self.expr(a)?;
                    let Some(k) = k else { return err("鍵に値が無い", a.span) };
                    let kc = self.conv(&k.v.clone(), &k.ty.clone(), &kt);
                    let stem = self.hash_fns(&b.ty.clone())?;
                    let at = self.tmp();
                    self.emit(&format!(
                        "{at} = call i64 {stem}.find(ptr {}, {} {kc})",
                        b.v,
                        ity(&kt)
                    ));
                    let hit = self.tmp();
                    self.emit(&format!("{hit} = icmp sge i64 {at}, 0"));
                    Ok(Some(Val { ok: b.ok, v: hit, ty: ValueType::U1 }))
                }
                // `h.keys()` — **入れた順**（C-98）。穴は飛ばす
                "keys" if is_hash(&b.ty) => {
                    let ValueType::Hash(kt, _) = &b.ty else { unreachable!() };
                    let kt = (**kt).clone();
                    let at_ty = ValueType::Array(Box::new(kt.clone()));
                    let live = self.coll_len(&b.v.clone());
                    let arr = self.new_collection(&live, &at_ty);
                    let dst = self.tmp();
                    self.emit(&format!("{dst} = call ptr @vaak.data(ptr {arr})"));
                    let ng = self.tmp();
                    self.emit(&format!("{ng} = getelementptr i8, ptr {}, i64 8", b.v));
                    let n = self.tmp();
                    self.emit(&format!("{n} = load i64, ptr {ng}"));
                    let ents = self.hash_entries(&b.v.clone());
                    let ent = self.hash_ent_ty(&b.ty.clone());
                    let ki = ity(&kt);
                    let ix = self.alloca("i64");
                    let ox = self.alloca("i64");
                    self.emit(&format!("store i64 0, ptr {ix}"));
                    self.emit(&format!("store i64 0, ptr {ox}"));
                    let head = self.label("hk.head");
                    let body = self.label("hk.body");
                    let take = self.label("hk.take");
                    let next = self.label("hk.next");
                    let done = self.label("hk.done");
                    self.br(&head);
                    self.place(&head);
                    let i = self.tmp();
                    self.emit(&format!("{i} = load i64, ptr {ix}"));
                    let go = self.tmp();
                    self.emit(&format!("{go} = icmp slt i64 {i}, {n}"));
                    self.cbr(&go, &body, &done);
                    self.place(&body);
                    let lp = self.tmp();
                    self.emit(&format!(
                        "{lp} = getelementptr {ent}, ptr {ents}, i64 {i}, i32 3"
                    ));
                    let lv = self.tmp();
                    self.emit(&format!("{lv} = load i8, ptr {lp}"));
                    let alive = self.tmp();
                    self.emit(&format!("{alive} = icmp ne i8 {lv}, 0"));
                    self.cbr(&alive, &take, &next);
                    self.place(&take);
                    let kp = self.tmp();
                    self.emit(&format!(
                        "{kp} = getelementptr {ent}, ptr {ents}, i64 {i}, i32 0"
                    ));
                    let kv = self.tmp();
                    self.emit(&format!("{kv} = load {ki}, ptr {kp}"));
                    let kv2 = if is_heap(&kt) {
                        // **同じ場所を指させない**（C-48）
                        let f = self.copy_fn(&kt);
                        let c = self.tmp();
                        self.emit(&format!("{c} = call ptr {f}(ptr {kv})"));
                        c
                    } else {
                        kv
                    };
                    let o = self.tmp();
                    self.emit(&format!("{o} = load i64, ptr {ox}"));
                    let op = self.tmp();
                    self.emit(&format!("{op} = getelementptr {ki}, ptr {dst}, i64 {o}"));
                    self.emit(&format!("store {ki} {kv2}, ptr {op}"));
                    let o2 = self.tmp();
                    self.emit(&format!("{o2} = add i64 {o}, 1"));
                    self.emit(&format!("store i64 {o2}, ptr {ox}"));
                    self.br(&next);
                    self.place(&next);
                    let i2 = self.tmp();
                    self.emit(&format!("{i2} = add i64 {i}, 1"));
                    self.emit(&format!("store i64 {i2}, ptr {ix}"));
                    self.br(&head);
                    self.place(&done);
                    Ok(Some(Val { ok: b.ok, v: arr, ty: at_ty }))
                }
                // `m.has(k)` — **引くだけ。値は取り出さない**
                "has" if is_map(&b.ty) => {
                    let ValueType::Map(kt, _) = &b.ty else { unreachable!() };
                    let kt = (**kt).clone();
                    let Some(a) = args.first() else {
                        return err("`has` は鍵を一つ取る", span);
                    };
                    let k = self.expr(a)?;
                    let Some(k) = k else { return err("鍵に値が無い", a.span) };
                    let kc = self.conv(&k.v.clone(), &k.ty.clone(), &kt);
                    let f = self.map_find_fn(&kt)?;
                    let r = self.tmp();
                    self.emit(&format!(
                        "{r} = call {{ i64, i1 }} {f}(ptr {}, {} {kc})",
                        b.v,
                        ity(&kt)
                    ));
                    let hit = self.tmp();
                    self.emit(&format!("{hit} = extractvalue {{ i64, i1 }} {r}, 1"));
                    // **`u1` は `i1` である。** 広げない
                    Ok(Some(Val { ok: b.ok, v: hit, ty: ValueType::U1 }))
                }
                // `m.keys()` — **鍵の配列を作る。** 並びは鍵の順（既にそうなっている）
                //
                // **確保が呼び出しとして書かれている**（C-6）。高いことが見える
                "keys" if is_map(&b.ty) => {
                    let ValueType::Map(kt, _) = &b.ty else { unreachable!() };
                    let kt = (**kt).clone();
                    let at = ValueType::Array(Box::new(kt.clone()));
                    let n = self.coll_len(&b.v.clone());
                    let arr = self.new_collection(&n, &at);
                    let src = self.tmp();
                    self.emit(&format!("{src} = call ptr @vaak.map.keys(ptr {})", b.v));
                    let dst = self.tmp();
                    self.emit(&format!("{dst} = call ptr @vaak.data(ptr {arr})"));
                    let bytes = self.tmp();
                    self.emit(&format!("{bytes} = mul i64 {n}, {}", elem_size(&kt)));
                    self.emit(&format!(
                        "call void @llvm.memcpy.p0.p0.i64(ptr {dst}, ptr {src}, i64 {bytes}, i1 false)"
                    ));
                    // **場を持つ鍵は写す。** 配列と写像が同じ場所を指してはいけない（C-48）
                    if is_heap(&kt) {
                        let f = self.copy_fn(&kt);
                        let head = self.label("keys.head");
                        let body = self.label("keys.body");
                        let done = self.label("keys.done");
                        let ix = self.alloca("i64");
                        self.emit(&format!("store i64 0, ptr {ix}"));
                        self.br(&head);
                        self.place(&head);
                        let i = self.tmp();
                        self.emit(&format!("{i} = load i64, ptr {ix}"));
                        let go = self.tmp();
                        self.emit(&format!("{go} = icmp slt i64 {i}, {n}"));
                        self.cbr(&go, &body, &done);
                        self.place(&body);
                        let ep = self.tmp();
                        self.emit(&format!("{ep} = getelementptr ptr, ptr {dst}, i64 {i}"));
                        let ev = self.tmp();
                        self.emit(&format!("{ev} = load ptr, ptr {ep}"));
                        let cv = self.tmp();
                        self.emit(&format!("{cv} = call ptr {f}(ptr {ev})"));
                        self.emit(&format!("store ptr {cv}, ptr {ep}"));
                        let i2 = self.tmp();
                        self.emit(&format!("{i2} = add i64 {i}, 1"));
                        self.emit(&format!("store i64 {i2}, ptr {ix}"));
                        self.br(&head);
                        self.place(&done);
                    }
                    Ok(Some(Val { ok: b.ok, v: arr, ty: at }))
                }
                _ => err(format!("`{name}` は STEEL がまだ扱えない"), span),
            };
        }
        let ExprKind::Name(name) = &callee.kind else {
            return err("STEEL はまだ自由関数の呼び出ししか扱えない", span);
        };
        if name == "getdepth" {
            // **フレーム基準からの段数**（C-23）。組み立て時に分かる
            let d = self.stages.len() as i128;
            return Ok(Some(self.konst(&ValueType::I64, d)));
        }
        let Some(f) = self.fns.get(name).cloned() else {
            return err(format!("知らない関数 `{name}`"), callee.span);
        };
        let mut vals = Vec::new();
        for (p, a) in f.params.iter().zip(args) {
            let ty = self.resolve(&p.ty.value);
            if width(&ty).is_none() && !is_float(&ty) && !is_heap(&ty) {
                return err("STEEL はまだ数と集合体しか扱えない", a.span);
            }

            // **`alias` は値ではなく、呼び出し元と同じセルを渡す**（C-20、S-21）。
            // 名前が既に別名なら `addr` が一段辿るので、常に値そのものの枠へ届く。
            if p.ty.is_alias {
                let ExprKind::Name(n) = &a.kind else {
                    return err("`alias` 引数に渡せるのは名前だけ", a.span);
                };
                let Some((cell, _)) = self.addr(n) else {
                    return err(format!("知らない名前 `{n}`"), a.span);
                };
                vals.push(("ptr".to_string(), cell, "true".to_string()));
                continue;
            }

            let v = self.expr(a)?;
            let Some(v) = v else { return err("引数に値が無い", a.span) };
            // **値引数は深く複製する**（C-20）。
            let c = if is_heap(&ty) {
                self.deep_copy(&v.v.clone(), &ty)
            } else {
                self.conv(&v.v.clone(), &v.ty.clone(), &ty)
            };
            // **paradox を引数に渡せる。** 型は paradox との直和である
            vals.push((ity(&ty), c, v.ok));
        }
        let ret = f.ret.as_ref().map(|t| t.value.clone()).unwrap_or(ValueType::I64);
        let sig: Vec<String> =
            vals.iter().map(|(t, v, ok)| format!("{t} {v}, i1 {ok}")).collect();
        let t = self.tmp();
        self.emit(&format!(
            "{t} = call {{ i1, {} }} @vaak_{name}({})",
            ity(&ret),
            sig.join(", ")
        ));
        let ok = self.tmp();
        self.emit(&format!("{ok} = extractvalue {{ i1, {} }} {t}, 0", ity(&ret)));
        let v = self.tmp();
        self.emit(&format!("{v} = extractvalue {{ i1, {} }} {t}, 1", ity(&ret)));
        Ok(Some(Val { ok, v, ty: ret }))
    }

    /// 一つの関数を書き出す。**本体の領域がフレームである**（C-23）。
    fn function(&mut self, f: &FnDecl) -> R<String> {
        self.head.clear();
        self.body.clear();
        self.n = 0;
        self.done = false;
        self.scopes.clear();
        self.stages.clear();
        self.push_scope();

        let ret = match f.ret.as_ref() {
            Some(t) => self.resolve(&t.value),
            None => ValueType::I64,
        };
        let mut params = Vec::new();
        for (i, p) in f.params.iter().enumerate() {
            if width(&p.ty.value).is_none()
                && !is_float(&p.ty.value)
                && !is_heap(&p.ty.value)
            {
                return err("STEEL はまだ数と集合体の引数しか扱えない", p.span);
            }
            let pt = self.resolve(&p.ty.value);
            let abi = if p.ty.is_alias { "ptr".to_string() } else { ity(&pt) };
            params.push(format!("{abi} %p{i}, i1 %pok{i}"));
        }

        // 可変な集合体の別名からは、grow や欄への代入で確保が呼び出し元へ逃げる。
        // その関数だけは場を戻さない（S-21）。読み取り専用の別名と数の別名は戻せる。
        let keeps_arena = f.params.iter().any(|p| {
            p.ty.is_alias
                && p.kind == BindKind::Var
                && is_heap(&self.resolve(&p.ty.value))
        });
        let mark = if keeps_arena {
            None
        } else {
            let mark = self.tmp();
            self.emit(&format!("{mark} = call i64 @vaak.mark()"));
            Some(mark)
        };

        // **フレームは段でもある**（C-23）——`break` の上限
        self.open_stage(None, false);
        for (i, p) in f.params.iter().enumerate() {
            let pt = self.resolve(&p.ty.value);
            if p.ty.is_alias {
                // 局所の別名枠を一つ持つので、`&=` で指し直しても caller の名前は動かない。
                self.declare_alias(&p.name, &format!("%p{i}"), pt);
            } else {
                let ptr = self.declare(&p.name, pt.clone());
                self.emit(&format!("store {} %p{i}, ptr {ptr}", ity(&pt)));
            }
        }
        let ExprKind::Block(items) = &f.body.kind else {
            return err("関数の本体はブロックでなければならない", f.span);
        };
        let r = self.region(items)?;
        match r {
            Some(v) => self.leave(1, Some(v))?,
            None => self.leave(1, None)?,
        }
        let out = self.close_stage();
        // **返り値は呼び出し側の領域へ移る**（C-90 の表）。
        // 印の下へ写してから、印を戻す——**領域は高々一つの値**（C-14）なので一つだけ
        let conv = if keeps_arena && is_heap(&ret) && is_heap(&out.ty) {
            // 場を保つ場合も、返り値は別の自己完結した値である（C-20）。
            // 解放を挟まないので、一度の深い複製で足りる。
            self.deep_copy(&out.v.clone(), &ret)
        } else if is_heap(&ret) && is_heap(&out.ty) {
            // **二段で写す**（C-90 の表：「返り値は呼び出し側の領域へ移る」）。
            //
            // 1. 印より上へ深く写す（**逃がす**）
            // 2. 印まで戻す
            // 3. そこから印の下へ深く写す
            //
            // 一段では足りない。**入れ子なら中身が印の上に残る**からである——
            // 外側の塊だけを下へ動かしても、指している先が消える。
            //
            // 重ならないことは数えれば分かる：
            // 逃がした先は元の頂より上、書き込む先は印から深さの分だけ。
            // **深さは元の頂と印の差を越えない**ので、届かない。
            let f = self.copy_fn(&ret);
            let up = self.tmp();
            self.emit(&format!("{up} = call ptr {f}(ptr {})", out.v));
            self.emit(&format!(
                "call void @vaak.release(i64 {})",
                mark.as_ref().expect("場を戻せる関数")
            ));
            let down = self.tmp();
            self.emit(&format!("{down} = call ptr {f}(ptr {up})"));
            down
        } else {
            let c = self.conv(&out.v.clone(), &out.ty.clone(), &ret);
            if let Some(mark) = &mark {
                self.emit(&format!("call void @vaak.release(i64 {mark})"));
            }
            c
        };
        let a = self.tmp();
        self.emit(&format!("{a} = insertvalue {{ i1, {} }} undef, i1 {}, 0", ity(&ret), out.ok));
        let b = self.tmp();
        self.emit(&format!("{b} = insertvalue {{ i1, {} }} {a}, {} {conv}, 1", ity(&ret), ity(&ret)));
        self.emit(&format!("ret {{ i1, {} }} {b}", ity(&ret)));

        Ok(format!(
            "define internal {{ i1, {} }} @vaak_{}({}) {{\nentry:\n{}{}}}\n",
            ity(&ret),
            f.name,
            params.join(", "),
            self.head,
            self.body
        ))
    }
}

/// 場（アリーナ）の前口上。
///
/// # なぜバンプ確保でよいか
///
/// **C-90 の表がそのまま設計になっている。**
///
/// | | |
/// |---|---|
/// | 値は自己完結（C-48） | **参照を追う必要が無い** |
/// | 別名は外へ返せない（C-19） | 内側を指したまま外へ出ない |
/// | 再帰的データ構造は作れない（C-63） | **循環しない** |
/// | 返り値は複製。実装は移動してよい | **呼び出し側の領域へ移る** |
///
/// ただし**可変な集合体の `alias` 引数は呼び出し元のセルそのもの**なので、
/// grow 等で作った値が関数を越える（S-21）。その関数は印を戻さない。
///
/// # 領域を抜けるときに何をするか
///
/// **印を戻すだけ。** ただし外へ出る値を先に印の下へ写す——
/// **領域は高々一つの値しか持たない**（C-14）ので、通常は写すのは一つだけである。
///
/// これは C-33（`:=` は深い複製）が既に言っていることを、そのまま実装したものである。
///
/// # 解放しない場合との違い
///
/// **観測できない。** 破棄処理が走らないので（C-90）、
/// 遅く捨てても早く捨てても同じ結果になる。だから**粗くしてよい。**
const PRELUDE: &str = r#"
; ==== 場（バンプ確保器）====
; **領域ごとのアリーナ**（C-90）。抜けたら印を戻すだけ
@vaak.heap = internal global [16777216 x i8] zeroinitializer
@vaak.bump = internal global i64 0

define internal ptr @vaak.alloc(i64 %n) {
entry:
  %b = load i64, ptr @vaak.bump
  ; 八バイト境界に揃える
  %a = add i64 %n, 7
  %r = and i64 %a, -8
  %e = add i64 %b, %r
  ; **尽きたら落とす。** 黙って踏み外すよりよい
  %over = icmp sgt i64 %e, 16777216
  br i1 %over, label %fail, label %ok
fail:
  call void @vaak.fail()
  unreachable
ok:
  store i64 %e, ptr @vaak.bump
  %p = getelementptr [16777216 x i8], ptr @vaak.heap, i64 0, i64 %b
  ret ptr %p
}

define internal i64 @vaak.mark() {
entry:
  %b = load i64, ptr @vaak.bump
  ret i64 %b
}

define internal void @vaak.release(i64 %m) {
entry:
  store i64 %m, ptr @vaak.bump
  ret void
}

; 集合体：{ i64 長さ, i64 容量, 要素… }
define internal ptr @vaak.new(i64 %len, i64 %esize) {
entry:
  %bytes = mul i64 %len, %esize
  %total = add i64 %bytes, 16
  %p = call ptr @vaak.alloc(i64 %total)
  store i64 %len, ptr %p
  %cp = getelementptr i8, ptr %p, i64 8
  store i64 %len, ptr %cp
  ret ptr %p
}

; **一つ分伸ばす。** 容量が足りれば同じ場所、足りなければ写して新しい場所を返す。
;
; 頭は十六バイト——前の八つが**個数**、後の八つが**容量**である。
; 容量は最初から持っていた（`@vaak.new` が個数と同じ値を入れている）ので、
; **表現を変えずに伸ばせる。**
define internal ptr @vaak.grow(ptr %p, i64 %esize) {
entry:
  %n = load i64, ptr %p
  %cp = getelementptr i8, ptr %p, i64 8
  %c = load i64, ptr %cp
  %fits = icmp slt i64 %n, %c
  %n1 = add i64 %n, 1
  br i1 %fits, label %inplace, label %move
inplace:
  store i64 %n1, ptr %p
  ret ptr %p
move:
  ; **倍にする。** 一つずつ伸ばすと押すたびに写すことになる
  %twice = shl i64 %c, 1
  %empty = icmp slt i64 %twice, 4
  %c2 = select i1 %empty, i64 4, i64 %twice
  %q = call ptr @vaak.new(i64 %c2, i64 %esize)
  %bytes = mul i64 %n, %esize
  %src = getelementptr i8, ptr %p, i64 16
  %dst = getelementptr i8, ptr %q, i64 16
  call void @llvm.memcpy.p0.p0.i64(ptr %dst, ptr %src, i64 %bytes, i1 false)
  store i64 %n1, ptr %q
  %qc = getelementptr i8, ptr %q, i64 8
  store i64 %c2, ptr %qc
  ret ptr %q
}

define internal i64 @vaak.len(ptr %p) {
entry:
  %n = load i64, ptr %p
  ret i64 %n
}

define internal ptr @vaak.data(ptr %p) {
entry:
  %d = getelementptr i8, ptr %p, i64 16
  ret ptr %d
}

; ================= 写像 =================
;
; **並べた配列である。** `.keys()` の並びは鍵の順と決まっている（全順序なので決まる）
; ので、順序を保つ表でなければならない。二分探索で引く。
;
; 頭は三十二バイト。
;
;   [ 0.. 8)  個数
;   [ 8..16)  容量
;   [16..24)  鍵の並び
;   [24..32)  値の並び
;
; 先頭が個数なので `@vaak.len` がそのまま効く。

define internal ptr @vaak.map.new() {
entry:
  %p = call ptr @vaak.alloc(i64 32)
  ; **場は使い回されるので、零で埋め直す。** 前の中身が残っている
  store i64 0, ptr %p
  %c = getelementptr i8, ptr %p, i64 8
  store i64 0, ptr %c
  %k = getelementptr i8, ptr %p, i64 16
  store ptr null, ptr %k
  %v = getelementptr i8, ptr %p, i64 24
  store ptr null, ptr %v
  ret ptr %p
}

define internal ptr @vaak.map.keys(ptr %m) {
entry:
  %g = getelementptr i8, ptr %m, i64 16
  %k = load ptr, ptr %g
  ret ptr %k
}

define internal ptr @vaak.map.vals(ptr %m) {
entry:
  %g = getelementptr i8, ptr %m, i64 24
  %v = load ptr, ptr %g
  ret ptr %v
}

; 一つ分の空きを作る。足りなければ倍にして写す
define internal void @vaak.map.reserve(ptr %m, i64 %ks, i64 %vs) {
entry:
  %n = load i64, ptr %m
  %cg = getelementptr i8, ptr %m, i64 8
  %c = load i64, ptr %cg
  %fits = icmp slt i64 %n, %c
  br i1 %fits, label %done, label %grow
grow:
  %twice = shl i64 %c, 1
  %small = icmp slt i64 %twice, 4
  %c2 = select i1 %small, i64 4, i64 %twice
  %kb = mul i64 %c2, %ks
  %vb = mul i64 %c2, %vs
  %nk = call ptr @vaak.alloc(i64 %kb)
  %nv = call ptr @vaak.alloc(i64 %vb)
  %kg = getelementptr i8, ptr %m, i64 16
  %vg = getelementptr i8, ptr %m, i64 24
  %ok = load ptr, ptr %kg
  %ov = load ptr, ptr %vg
  %hasold = icmp sgt i64 %n, 0
  br i1 %hasold, label %copy, label %put
copy:
  %okb = mul i64 %n, %ks
  %ovb = mul i64 %n, %vs
  call void @llvm.memcpy.p0.p0.i64(ptr %nk, ptr %ok, i64 %okb, i1 false)
  call void @llvm.memcpy.p0.p0.i64(ptr %nv, ptr %ov, i64 %ovb, i1 false)
  br label %put
put:
  store ptr %nk, ptr %kg
  store ptr %nv, ptr %vg
  store i64 %c2, ptr %cg
  br label %done
done:
  ret void
}

; `at` に一つ分の隙間を開ける。**空きは呼び手が先に作っておく**
define internal void @vaak.map.open(ptr %m, i64 %at, i64 %ks, i64 %vs) {
entry:
  %n = load i64, ptr %m
  %keys = call ptr @vaak.map.keys(ptr %m)
  %vals = call ptr @vaak.map.vals(ptr %m)
  %cnt = sub i64 %n, %at
  %ko = mul i64 %at, %ks
  %vo = mul i64 %at, %vs
  %ksrc = getelementptr i8, ptr %keys, i64 %ko
  %vsrc = getelementptr i8, ptr %vals, i64 %vo
  %kdst = getelementptr i8, ptr %ksrc, i64 %ks
  %vdst = getelementptr i8, ptr %vsrc, i64 %vs
  %kb = mul i64 %cnt, %ks
  %vb = mul i64 %cnt, %vs
  call void @llvm.memmove.p0.p0.i64(ptr %kdst, ptr %ksrc, i64 %kb, i1 false)
  call void @llvm.memmove.p0.p0.i64(ptr %vdst, ptr %vsrc, i64 %vb, i1 false)
  %n1 = add i64 %n, 1
  store i64 %n1, ptr %m
  ret void
}

; `at` の一つを抜いて詰める
define internal void @vaak.map.close(ptr %m, i64 %at, i64 %ks, i64 %vs) {
entry:
  %n = load i64, ptr %m
  %keys = call ptr @vaak.map.keys(ptr %m)
  %vals = call ptr @vaak.map.vals(ptr %m)
  %at1 = add i64 %at, 1
  %cnt = sub i64 %n, %at1
  %ko = mul i64 %at, %ks
  %vo = mul i64 %at, %vs
  %kdst = getelementptr i8, ptr %keys, i64 %ko
  %vdst = getelementptr i8, ptr %vals, i64 %vo
  %ksrc = getelementptr i8, ptr %kdst, i64 %ks
  %vsrc = getelementptr i8, ptr %vdst, i64 %vs
  %kb = mul i64 %cnt, %ks
  %vb = mul i64 %cnt, %vs
  call void @llvm.memmove.p0.p0.i64(ptr %kdst, ptr %ksrc, i64 %kb, i1 false)
  call void @llvm.memmove.p0.p0.i64(ptr %vdst, ptr %vsrc, i64 %vb, i1 false)
  %n1 = sub i64 %n, 1
  store i64 %n1, ptr %m
  ret void
}

; 並びごと写す。**深く写すのは呼び手の仕事**（鍵や値が場を持つときだけ）
define internal ptr @vaak.map.clone(ptr %m, i64 %ks, i64 %vs) {
entry:
  %n = load i64, ptr %m
  %q = call ptr @vaak.map.new()
  %none = icmp eq i64 %n, 0
  br i1 %none, label %out, label %work
work:
  %kb = mul i64 %n, %ks
  %vb = mul i64 %n, %vs
  %nk = call ptr @vaak.alloc(i64 %kb)
  %nv = call ptr @vaak.alloc(i64 %vb)
  %ok = call ptr @vaak.map.keys(ptr %m)
  %ov = call ptr @vaak.map.vals(ptr %m)
  call void @llvm.memcpy.p0.p0.i64(ptr %nk, ptr %ok, i64 %kb, i1 false)
  call void @llvm.memcpy.p0.p0.i64(ptr %nv, ptr %ov, i64 %vb, i1 false)
  store i64 %n, ptr %q
  %qc = getelementptr i8, ptr %q, i64 8
  store i64 %n, ptr %qc
  %qk = getelementptr i8, ptr %q, i64 16
  store ptr %nk, ptr %qk
  %qv = getelementptr i8, ptr %q, i64 24
  store ptr %nv, ptr %qv
  br label %out
out:
  ret ptr %q
}

; ================= hash =================
;
; **鍵の値で飛ぶ連想**（C-98）。`map` と違い `.keys()` は**入れた順**である。
;
; 連鎖法。並びは密で、抜くと穴が空く。
;
;   [ 0.. 8)  生きている数     ← 先頭なので `@vaak.len` が効く
;   [ 8..16)  並びの長さ（穴込み）
;   [16..24)  並びの容量
;   [24..32)  並び
;   [32..40)  bucket（要素の位置。無ければ -1）
;   [40..48)  bucket の数（2 の冪）
;
; 要素は `{ 鍵, 値, 次, 生きているか }`。**次**が同じ bucket の鎖である。

define internal ptr @vaak.hash.new() {
entry:
  %p = call ptr @vaak.alloc(i64 48)
  store i64 0, ptr %p
  %a = getelementptr i8, ptr %p, i64 8
  store i64 0, ptr %a
  %b = getelementptr i8, ptr %p, i64 16
  store i64 0, ptr %b
  %c = getelementptr i8, ptr %p, i64 24
  store ptr null, ptr %c
  %d = getelementptr i8, ptr %p, i64 32
  store ptr null, ptr %d
  %e = getelementptr i8, ptr %p, i64 40
  store i64 0, ptr %e
  ret ptr %p
}

; 浮動小数を**単調な i64 へ写す。**
;
; 生のビット列は数の順と一致しない（負ほど大きくなる）ので並べ替える。
; **順序と等しさを同時に直す**ので、`map` も `hash` も同じ写しを使える。
;
; `-0.0` は `0.0` へ潰す。**`-0.0 == 0.0` が真である以上、鍵も同じでなければならない。**
define internal i64 @vaak.fkey(double %x) {
entry:
  %z = fcmp oeq double %x, 0.0
  %v = select i1 %z, double 0.0, double %x
  %b = bitcast double %v to i64
  %neg = icmp slt i64 %b, 0
  %inv = xor i64 %b, -1
  %pos = or i64 %b, -9223372036854775808
  %k = select i1 %neg, i64 %inv, i64 %pos
  ret i64 %k
}

; 整数の混ぜ方（splitmix64 の仕上げ）。**下位だけ見ても散る**ようにする
define internal i64 @vaak.hash.i64(i64 %x0) {
entry:
  %s1 = lshr i64 %x0, 30
  %x1 = xor i64 %x0, %s1
  %m1 = mul i64 %x1, -4658895280553007687
  %s2 = lshr i64 %m1, 27
  %x2 = xor i64 %m1, %s2
  %m2 = mul i64 %x2, -7723592293110705685
  %s3 = lshr i64 %m2, 31
  %x3 = xor i64 %m2, %s3
  ret i64 %x3
}

; 文字列は FNV-1a
define internal i64 @vaak.hash.str(ptr %p) {
entry:
  %n = load i64, ptr %p
  %d = getelementptr i8, ptr %p, i64 16
  br label %head
head:
  %i = phi i64 [ 0, %entry ], [ %i2, %body ]
  %h = phi i64 [ -3750763034362895579, %entry ], [ %h2, %body ]
  %go = icmp slt i64 %i, %n
  br i1 %go, label %body, label %done
body:
  %bp = getelementptr i8, ptr %d, i64 %i
  %b = load i8, ptr %bp
  %bz = zext i8 %b to i64
  %hx = xor i64 %h, %bz
  %h2 = mul i64 %hx, 1099511628211
  %i2 = add i64 %i, 1
  br label %head
done:
  ret i64 %h
}

; 文字列の順序。**短い方が前、同じ長さなら中身のバイト順**
define internal i32 @vaak.strcmp(ptr %a, ptr %b) {
entry:
  %na = load i64, ptr %a
  %nb = load i64, ptr %b
  %da = getelementptr i8, ptr %a, i64 16
  %db = getelementptr i8, ptr %b, i64 16
  %alt = icmp slt i64 %na, %nb
  %min = select i1 %alt, i64 %na, i64 %nb
  %c = call i32 @memcmp(ptr %da, ptr %db, i64 %min)
  %ne = icmp ne i32 %c, 0
  br i1 %ne, label %out, label %bylen
bylen:
  %agt = icmp sgt i64 %na, %nb
  %x = select i1 %alt, i32 -1, i32 0
  %y = select i1 %agt, i32 1, i32 %x
  br label %out
out:
  %r = phi i32 [ %c, %entry ], [ %y, %bylen ]
  ret i32 %r
}

; **写す。** 要素が場を持たないときはこれで足りる
define internal ptr @vaak.copy(ptr %p, i64 %esize) {
entry:
  %n = load i64, ptr %p
  %q = call ptr @vaak.new(i64 %n, i64 %esize)
  %bytes = mul i64 %n, %esize
  %src = getelementptr i8, ptr %p, i64 16
  %dst = getelementptr i8, ptr %q, i64 16
  call void @llvm.memcpy.p0.p0.i64(ptr %dst, ptr %src, i64 %bytes, i1 false)
  ret ptr %q
}

; **印の下へ写す。** 領域を抜けるときに、外へ出る一つの値だけを移す（C-14）
define internal ptr @vaak.carry(ptr %p, i64 %esize, i64 %mark) {
entry:
  %n = load i64, ptr %p
  %bytes = mul i64 %n, %esize
  %total = add i64 %bytes, 16
  store i64 %mark, ptr @vaak.bump
  %q = call ptr @vaak.alloc(i64 %total)
  ; **重なりうる。** memmove で写す
  call void @llvm.memmove.p0.p0.i64(ptr %q, ptr %p, i64 %total, i1 false)
  ret ptr %q
}

; **実行時の誤り。** 書けない場所に書こうとしたときなど
declare void @exit(i32)
define internal void @vaak.fail() {
entry:
  call void @exit(i32 70)
  unreachable
}

declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
declare void @llvm.memmove.p0.p0.i64(ptr, ptr, i64, i1)
declare i32 @memcmp(ptr, ptr, i64)
"#;

/// プログラム全体を LLVM IR に。
///
/// **最上位の外界面は言語の意味論ではない**（C-31）。
/// スタンドアロンでは**終了コード**にする——値ならその下位 8 ビット、
/// **中身が空なら 0。**
pub fn compile(prog: &Program) -> R<String> {
    let mut s = Steel::new();

    // 関数は**スコープ全体で見える**（C-36）ので、先に集める
    let (mut fns, mut flows) = (HashMap::new(), HashMap::new());
    let (mut structs, mut wraps) = (HashMap::new(), HashMap::new());
    collect(&prog.body, &mut fns, &mut flows, &mut structs, &mut wraps);
    s.fns = fns;
    s.flows = flows;
    s.structs = structs;
    s.wraps = wraps;

    let mut out = String::new();

    let mut fns: Vec<FnDecl> = s.fns.values().cloned().collect();
    fns.sort_by(|a, b| a.name.cmp(&b.name));
    for f in &fns {
        let t = s.function(f)?;
        out.push_str(&t);
        out.push('\n');
    }

    // 最上位
    s.head.clear();
    s.body.clear();
    s.n = 0;
    s.done = false;
    s.scopes.clear();
    s.stages.clear();
    s.push_scope();
    s.open_stage(None, false);
    let r = s.region(&prog.body)?;
    match r {
        Some(v) => s.leave(1, Some(v))?,
        None => s.leave(1, None)?,
    }
    let top = s.close_stage();
    let w = s.conv(&top.v.clone(), &top.ty.clone(), &ValueType::I64);
    let t = s.tmp();
    s.emit(&format!("{t} = trunc i64 {w} to i32"));
    // **中身が空なら 0。** エラーではない（C-31）
    let code = s.tmp();
    s.emit(&format!("{code} = select i1 {}, i32 {t}, i32 0", top.ok));
    s.emit(&format!("ret i32 {code}"));
    out.push_str(&format!("define i32 @main() {{\nentry:\n{}{}}}\n", s.head, s.body));


    // **宣言と定数は最後に集める。** 組み立ての途中で増えるので
    let mut head = String::from("; Vaak — STEEL（LLVM IR）\n");
    head.push_str("target triple = \"x86_64-pc-linux-gnu\"\n");
    head.push_str(PRELUDE);
    head.push('\n');
    for d in &s.decls {
        head.push_str(d);
    }
    head.push('\n');
    head.push_str(&out);
    let out = head;
    Ok(out)
}

/// **宣言はスコープ全体で見える**（C-36）ので、先に集める。
///
/// 作用素式も同じである——関数の本体から使えなければ `$return` が書けない。
fn collect(
    items: &[Expr],
    fns: &mut HashMap<String, FnDecl>,
    flows: &mut HashMap<String, Escape>,
    structs: &mut HashMap<String, StructDecl>,
    wraps: &mut HashMap<String, ValueType>,
) {
    for e in items {
        match &e.kind {
            ExprKind::Discard(Some(inner)) => {
                collect(std::slice::from_ref(inner), fns, flows, structs, wraps)
            }
            ExprKind::FnDecl(f) if f.owner.is_none() => {
                fns.insert(f.name.clone(), f.clone());
            }
            ExprKind::FlowDecl(d) => {
                flows.insert(d.name.clone(), (*d.body).clone());
            }
            ExprKind::StructDecl(d) => {
                structs.insert(d.name.clone(), d.clone());
            }
            ExprKind::WrapDecl(d) => {
                wraps.insert(d.name.clone(), d.base.value.clone());
            }
            _ => {}
        }
    }
}

/// 代入演算子を二項演算子へ写す。**`:=` と `&=` は演算ではない**ので持たない。
fn assign_binop(op: AssignOp) -> Option<BinOp> {
    Some(match op {
        AssignOp::Add => BinOp::Add,
        AssignOp::Sub => BinOp::Sub,
        AssignOp::Mul => BinOp::Mul,
        AssignOp::Div => BinOp::Div,
        AssignOp::Mod => BinOp::Mod,
        AssignOp::Shl => BinOp::Shl,
        AssignOp::Shr => BinOp::Shr,
        AssignOp::BitXor => BinOp::BitXor,
        AssignOp::BitOr => BinOp::BitOr,
        _ => return None,
    })
}

/// 型ごとの名札。**同じ型に同じ関数を二度出さないため**
fn type_tag(t: &ValueType) -> String {
    match t {
        ValueType::Str => "str".into(),
        ValueType::Array(e) => format!("a{}", type_tag(e)),
        ValueType::Map(k, v) => format!("m{}_{}", type_tag(k), type_tag(v)),
        ValueType::Hash(k, v) => format!("h{}_{}", type_tag(k), type_tag(v)),
        ValueType::Named(n) => format!("n{n}"),
        other => ity(other).replace('*', "p"),
    }
}
