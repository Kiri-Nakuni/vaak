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
        ValueType::Array(_) | ValueType::Str | ValueType::Named(_) => "ptr".into(),
        _ => format!("i{}", width(t).unwrap_or(64)),
    }
}

fn is_float(t: &ValueType) -> bool {
    matches!(t, ValueType::F32 | ValueType::F64 | ValueType::F80)
}

/// **場に置かれるもの。** 値そのものではなく、場所を指す。
fn is_heap(t: &ValueType) -> bool {
    // **`Named` はここに来る時点で構造体である**——包みは `resolve` で剥がしてある
    matches!(t, ValueType::Array(_) | ValueType::Str | ValueType::Named(_))
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
        let el = elem_of(&t).unwrap_or(ValueType::I64);
        // `new T array ( )` — 空
        let Some(nx) = a.first() else {
            let p = self.new_collection("0", &t);
            return Ok(Some(Val { ok: "true".into(), v: p, ty: t }));
        };
        let n = self.expr(nx)?;
        let Some(n) = n else { return err("`new` の個数に値が無い", nx.span) };
        // **包む／剥がす**（S-2）。既にその型なら、そのまま通す——
        // `new Bytes ( <u8 array> )` は長さではなく**包む**という意味である
        if is_heap(&n.ty) {
            return Ok(Some(Val { ok: n.ok, v: n.v, ty: t }));
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
                    let v = self.expr(init)?;
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

            _ => err("STEEL がまだ扱えない構文", e.span),
        }
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
            let v = self.expr(a)?;
            let Some(v) = v else { return err("引数に値が無い", a.span) };
            let ty = self.resolve(&p.ty.value);
            if width(&ty).is_none() && !is_float(&ty) && !is_heap(&ty) {
                return err("STEEL はまだ数と集合体しか扱えない", a.span);
            }
            // **複製か別名かは型が決める**（C-20）。`alias` なら写さない
            let c = if is_heap(&ty) {
                if p.ty.is_alias {
                    v.v.clone()
                } else {
                    self.deep_copy(&v.v.clone(), &ty)
                }
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
            params.push(format!("{} %p{i}, i1 %pok{i}", ity(&pt)));
        }

        // **場の印を取る。** 関数を出るときに戻す（C-90）
        let mark = self.tmp();
        self.emit(&format!("{mark} = call i64 @vaak.mark()"));

        // **フレームは段でもある**（C-23）——`break` の上限
        self.open_stage(None, false);
        for (i, p) in f.params.iter().enumerate() {
            let pt = self.resolve(&p.ty.value);
            let ptr = self.declare(&p.name, pt.clone());
            self.emit(&format!("store {} %p{i}, ptr {ptr}", ity(&pt)));
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
        let conv = if is_heap(&ret) && is_heap(&out.ty) {
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
            self.emit(&format!("call void @vaak.release(i64 {mark})"));
            let down = self.tmp();
            self.emit(&format!("{down} = call ptr {f}(ptr {up})"));
            down
        } else {
            let c = self.conv(&out.v.clone(), &out.ty.clone(), &ret);
            self.emit(&format!("call void @vaak.release(i64 {mark})"));
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
/// > スコープを越えて生き残るものが無い。だから、抜けた時点での解放は正確である。
///
/// # 領域を抜けるときに何をするか
///
/// **印を戻すだけ。** ただし外へ出る値を先に印の下へ写す——
/// **領域は高々一つの値しか持たない**（C-14）ので、写すのは一つだけである。
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
