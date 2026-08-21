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
    /// 組み込みの宣言（`llvm.fabs` など）。**重ねて出さない**
    decls: Vec<String>,
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
        _ => format!("i{}", width(t).unwrap_or(64)),
    }
}

fn is_float(t: &ValueType) -> bool {
    matches!(t, ValueType::F32 | ValueType::F64)
}

/// LLVM の浮動小数リテラル。**十進では丸めが入る**ので、ビット列で書く。
///
/// `float` も**倍精度のビット列**で書くのが LLVM の流儀である
/// （その値が単精度で表せることを検証してくれる）。
fn fbits(v: f64, ty: &ValueType) -> String {
    let d = if matches!(ty, ValueType::F32) { v as f32 as f64 } else { v };
    format!("0x{:016X}", d.to_bits())
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
            decls: Vec::new(),
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

// ================= 式 =================

impl Steel {
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
                    if matches!(from, ValueType::F32) { "fpext" } else { "fptrunc" }
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
        let bits = if matches!(ty, ValueType::F32) { "f32" } else { "f64" };
        self.declare_fabs(bits, &t);
        let a = self.tmp();
        self.emit(&format!("{a} = call {t} @llvm.fabs.{bits}({t} {x})"));
        let f = self.tmp();
        let inf = if matches!(ty, ValueType::F32) {
            "0x7FF0000000000000"
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
            self.emit(&format!("{r} = fcmp one {t} {}, 0.0", x.v));
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
        let v = self.conv(&raw, &ValueType::I64, &st.ty);
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
                let w = self.conv(&p.v.clone(), &p.ty.clone(), &ValueType::I64);
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

            E::Name(n) => {
                let Some((p, ty)) = self.lookup(n) else {
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
                    let BindInit::Value(init) = &b.init else {
                        return err("`&=` は STEEL がまだ扱えない", b.span);
                    };
                    let v = self.expr(init)?;
                    let Some(v) = v else {
                        return err("束縛する値が無い", b.span);
                    };
                    let ty = b.ty.as_ref().map(|t| t.value.clone()).unwrap_or(v.ty.clone());
                    if width(&ty).is_none() && !is_float(&ty) {
                        return err("STEEL はまだ数しか扱えない", b.span);
                    }
                    let conv = self.conv(&v.v.clone(), &v.ty.clone(), &ty);
                    let p = self.declare(&b.name, ty.clone());
                    self.emit(&format!("store {} {conv}, ptr {p}", ity(&ty)));
                }
                // **宣言は値を置かない。** 外界面は paradox だが、`;` が潰す
                Ok(None)
            }

            E::Assign { op, lhs, rhs } => {
                let ExprKind::Name(n) = &lhs.kind else {
                    return err("STEEL はまだ名前への代入しか扱えない", e.span);
                };
                if *op == AssignOp::Alias {
                    return err("`&=` は STEEL がまだ扱えない", e.span);
                }
                let Some((p, ty)) = self.lookup(n) else {
                    return err(format!("知らない名前 `{n}`"), lhs.span);
                };
                let r = self.expr(rhs)?;
                let Some(r) = r else { return err("代入する値が無い", e.span) };
                let v = if *op == AssignOp::Set {
                    self.conv(&r.v.clone(), &r.ty.clone(), &ty)
                } else {
                    let cur = self.tmp();
                    self.emit(&format!("{cur} = load {}, ptr {p}", ity(&ty)));
                    let b = match op {
                        AssignOp::Add => BinOp::Add,
                        AssignOp::Sub => BinOp::Sub,
                        AssignOp::Mul => BinOp::Mul,
                        AssignOp::Div => BinOp::Div,
                        AssignOp::Mod => BinOp::Mod,
                        AssignOp::Shl => BinOp::Shl,
                        AssignOp::Shr => BinOp::Shr,
                        AssignOp::BitXor => BinOp::BitXor,
                        AssignOp::BitOr => BinOp::BitOr,
                        _ => return err("扱えない代入", e.span),
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
                let t = ty.value.clone();
                if width(&t).is_none() && !is_float(&t) {
                    return err("STEEL はまだ数しか扱えない", e.span);
                }
                let c = self.conv(&v.v.clone(), &v.ty.clone(), &t);
                Ok(Some(Val { ok: v.ok, v: c, ty: t }))
            }

            E::Call { callee, args } => self.call(callee, args, e.span),

            // **関数の宣言はここでは組まない。** 先に集めてある
            E::FnDecl(_) | E::StructDecl(_) | E::WrapDecl(_) | E::FlowDecl(_) => Ok(None),

            _ => err("STEEL がまだ扱えない構文", e.span),
        }
    }

    /// `??` — **左が値なら右は走らない。**
    fn coalesce(&mut self, lhs: &Expr, rhs: &Expr, span: Span) -> R<Val> {
        let l = self.expr(lhs)?;
        let Some(l) = l else { return err("`??` の左に領域が無い", span) };
        let slot = self.alloca("i64");
        let ok_slot = self.alloca("i1");
        let use_r = self.label("qq.right");
        let done = self.label("qq.done");

        let w = self.conv(&l.v.clone(), &l.ty.clone(), &ValueType::I64);
        self.emit(&format!("store i64 {w}, ptr {slot}"));
        self.emit(&format!("store i1 {}, ptr {ok_slot}", l.ok));
        self.cbr(&l.ok.clone(), &done, &use_r);

        self.place(&use_r);
        let r = self.expr(rhs)?;
        let ty = match r {
            Some(r) => {
                let w = self.conv(&r.v.clone(), &r.ty.clone(), &ValueType::I64);
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
        let v = self.conv(&raw, &ValueType::I64, &ty);
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
                let w = self.conv(&v.v.clone(), &v.ty.clone(), &ValueType::I64);
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
                let w = self.conv(&v.v.clone(), &v.ty.clone(), &ValueType::I64);
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
        let v = self.conv(&raw, &ValueType::I64, &ty);
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
                let w = self.conv(&v.v.clone(), &v.ty.clone(), &ValueType::I64);
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
        let v = self.conv(&raw, &ValueType::I64, &ty);
        Ok(Val { ok, v, ty })
    }

    /// 脱出。**段数が静的に分かっていれば `br` 一つになる。**
    fn escape(&mut self, x: &Escape) -> R<()> {
        // 段数を数える。`break break 5` は二段
        let mut depth = 0usize;
        let mut cur = x;
        let mut payload: Option<&Expr> = None;
        let mut is_continue = false;
        loop {
            match &cur.kind {
                EscapeKind::Break { outward } => {
                    if *outward {
                        return err("`outward` は STEEL がまだ扱えない", cur.span);
                    }
                    depth += 1;
                }
                EscapeKind::Continue => {
                    is_continue = true;
                }
                EscapeKind::Flow { .. } => {
                    return err("作用素式は STEEL がまだ扱えない", cur.span)
                }
            }
            match &cur.operand {
                Some(Operand::Escape(inner)) => {
                    if is_continue {
                        return err("`continue` の遅延した被演算子は STEEL がまだ扱えない", cur.span);
                    }
                    cur = inner;
                }
                Some(Operand::Value(v)) => {
                    payload = Some(v);
                    break;
                }
                None => break,
            }
        }

        if is_continue {
            // **段を再開させる。** `break` の連なりの分だけ外へ出てから
            let idx = self.stages.len().checked_sub(depth.max(1)).ok_or(SteelError {
                msg: "段が足りない".into(),
                span: x.span,
            })?;
            let Some(cont) = self.stages[idx].cont.clone() else {
                return err("`continue` の抜けた先がループではない", x.span);
            };
            self.br(&cont);
            return Ok(());
        }

        let p = match payload {
            Some(e) => self.expr(e)?,
            None => None,
        };
        self.leave(depth, p)
    }
}

// ================= 関数とモジュール =================

impl Steel {
    fn call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> R<Region> {
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
            let ty = p.ty.value.clone();
            if width(&ty).is_none() && !is_float(&ty) {
                return err("STEEL はまだ数しか扱えない", a.span);
            }
            let c = self.conv(&v.v.clone(), &v.ty.clone(), &ty);
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

        let ret = f.ret.as_ref().map(|t| t.value.clone()).unwrap_or(ValueType::I64);
        let mut params = Vec::new();
        for (i, p) in f.params.iter().enumerate() {
            if width(&p.ty.value).is_none() && !is_float(&p.ty.value) {
                return err("STEEL はまだ数の引数しか扱えない", p.span);
            }
            params.push(format!("{} %p{i}, i1 %pok{i}", ity(&p.ty.value)));
        }

        // **フレームは段でもある**（C-23）——`break` の上限
        self.open_stage(None, false);
        for (i, p) in f.params.iter().enumerate() {
            let ptr = self.declare(&p.name, p.ty.value.clone());
            self.emit(&format!("store {} %p{i}, ptr {ptr}", ity(&p.ty.value)));
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
        let conv = self.conv(&out.v.clone(), &out.ty.clone(), &ret);
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

/// プログラム全体を LLVM IR に。
///
/// **最上位の外界面は言語の意味論ではない**（C-31）。
/// スタンドアロンでは**終了コード**にする——値ならその下位 8 ビット、
/// **中身が空なら 0。**
pub fn compile(prog: &Program) -> R<String> {
    let mut s = Steel::new();

    // 関数は**スコープ全体で見える**（C-36）ので、先に集める
    collect(&prog.body, &mut s.fns);

    let mut out = String::new();
    out.push_str("; Vaak — STEEL（LLVM IR）\n");
    out.push_str("target triple = \"x86_64-pc-linux-gnu\"\n\n");

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

    Ok(out)
}

fn collect(items: &[Expr], fns: &mut HashMap<String, FnDecl>) {
    for e in items {
        match &e.kind {
            ExprKind::Discard(Some(inner)) => collect(std::slice::from_ref(inner), fns),
            ExprKind::FnDecl(f) if f.owner.is_none() => {
                fns.insert(f.name.clone(), f.clone());
            }
            _ => {}
        }
    }
}
