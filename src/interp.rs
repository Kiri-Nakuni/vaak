//! 木を辿る評価器。**参照実装である**——後でバイトコード VM を書いたら、
//! 同じテストに掛けて差分を取る。
//!
//! 評価の結果は四つ（C-88 (1) / C-85）：
//! 何も無い（アーカーシャ）／値／paradox／脱出。**paradox は値ではなくセルにも入らない。**

use crate::ast::*;
use crate::span::Span;
use crate::value::{Arena, CellId, MapKey, Value};
use std::collections::HashMap;

// ================= 評価の結果 =================

#[derive(Clone, Debug)]
pub enum Eval {
    /// 何も無い。**値ではない。領域を占めない。**
    Akasha,
    Value(Value),
    /// 「ここに実在する値が無い」ことを表す値。**発生点を持つ**（C-46）。
    Paradox(Span),
    /// 制御の転送。**値でも状態でもない。**
    Escape(EscapeVal),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EKind {
    Break,
    Continue,
}

#[derive(Clone, Debug)]
pub struct EscapeVal {
    pub kind: EKind,
    /// 残りの段数。**段送りは書いた順（左から右）に起きる**（C-70）。
    pub stages: u32,
    /// 越えてよいフレームの数。`outward` が与える。
    pub outward: u32,
    pub payload: Option<Value>,
    /// `continue` の**遅延した被演算子**（C-73）。再開した本体の先頭で実行される。
    pub deferred: Option<Box<Escape>>,
    pub span: Span,
}

/// 実行時エラーは三種だけ（C-72）。
#[derive(Clone, Debug)]
pub struct RuntimeError {
    pub msg: String,
    pub span: Span,
}

pub type R<T> = Result<T, RuntimeError>;

fn rt<T>(msg: impl Into<String>, span: Span) -> R<T> {
    Err(RuntimeError { msg: msg.into(), span })
}

// ================= 環境 =================

#[derive(Clone, Debug)]
struct Binding {
    cell: CellId,
    kind: BindKind,
    /// `&=` で束縛したか。別名は指し直せる（C-62）。
    is_alias: bool,
}

#[derive(Debug)]
struct Scope {
    vars: HashMap<String, Binding>,
    mark: usize,
    /// 脱出段か。**裸のブロック・ループ本体・関数本体だけ**（C-64）。
    is_stage: bool,
    is_loop: bool,
    is_frame: bool,
    /// このスコープで凍っているセル（`const` 別名。C-79 (5) は集合で持つ）。
    frozen: Vec<CellId>,
}

#[derive(Clone)]
struct FnEntry {
    decl: FnDecl,
}

pub struct Interp {
    arena: Arena,
    scopes: Vec<Scope>,
    /// 関数の可視範囲はスコープを越える（C-36）。フレームを跨いでも見える。
    fns: HashMap<String, FnEntry>,
    structs: HashMap<String, StructDecl>,
    flows: HashMap<String, FlowDecl>,
    /// 変数の探索はこの位置より外へ行かない。**関数は局所変数を見ない**（C-86）。
    frame_base: usize,
    /// セルの凍結。`const` 別名は元の名前からの書き込みも禁じる（C-78）。
    frozen: Vec<CellId>,
    depth_guard: u32,
}

const PRELUDE: &str = r#"
flow $return = $repeat(break, getdepth());
"#;

impl Interp {
    pub fn new() -> Self {
        let mut it = Interp {
            arena: Arena::new(),
            scopes: Vec::new(),
            fns: HashMap::new(),
            structs: HashMap::new(),
            flows: HashMap::new(),
            frame_base: 0,
            frozen: Vec::new(),
            depth_guard: 0,
        };
        // 最上位は領域でありスコープでありフレームである
        it.push_scope(true, false, true);
        let prelude = crate::parser::parse(PRELUDE).expect("無名標準ライブラリの解析に失敗");
        it.collect_decls(&prelude.body);
        it
    }

    /// プログラムを走らせ、最上位の外界面を返す。
    pub fn run(&mut self, prog: &Program) -> R<Eval> {
        self.collect_decls(&prog.body);
        self.region(&prog.body, Span::NONE)
    }

    // ---- スコープ ----

    fn push_scope(&mut self, is_stage: bool, is_loop: bool, is_frame: bool) {
        let mark = self.arena.mark();
        self.scopes.push(Scope {
            vars: HashMap::new(),
            mark,
            is_stage,
            is_loop,
            is_frame,
            frozen: Vec::new(),
        });
    }

    /// **領域ごとのアリーナ**（C-90）。抜けたらまとめて捨てる。個々の値は辿らない。
    fn pop_scope(&mut self) {
        if let Some(s) = self.scopes.pop() {
            for c in s.frozen {
                if let Some(i) = self.frozen.iter().rposition(|x| *x == c) {
                    self.frozen.remove(i);
                }
            }
            self.arena.release(s.mark);
        }
    }

    fn lookup(&self, name: &str) -> Option<&Binding> {
        for s in self.scopes[self.frame_base..].iter().rev() {
            if let Some(b) = s.vars.get(name) {
                return Some(b);
            }
        }
        None
    }

    fn declare(&mut self, name: &str, b: Binding) {
        self.scopes.last_mut().unwrap().vars.insert(name.to_string(), b);
    }

    /// フレームからの段数（C-23）。`getdepth()` が返す。
    fn depth(&self) -> i64 {
        let mut n = 0i64;
        for s in self.scopes.iter().rev() {
            if s.is_stage {
                n += 1;
            }
            if s.is_frame {
                break;
            }
        }
        n
    }

    /// **宣言はスコープ全体で見える**（C-36）。中身を評価する前に集める。
    ///
    /// 宣言は paradox を産むので `;` が付く。**`;` が包んだ中身も見る。**
    fn collect_decls(&mut self, body: &[Expr]) {
        for e in body {
            let mut e = e;
            while let ExprKind::Discard(Some(inner)) = &e.kind {
                e = inner;
            }
            match &e.kind {
                ExprKind::FnDecl(f) => {
                    self.fns.insert(f.name.clone(), FnEntry { decl: f.clone() });
                }
                ExprKind::StructDecl(s) => {
                    self.structs.insert(s.name.clone(), s.clone());
                }
                ExprKind::FlowDecl(f) => {
                    self.flows.insert(f.name.clone(), f.clone());
                }
                _ => {}
            }
        }
    }

    // ================= 領域 =================

    /// **一つの領域は値を一つしか持てない。**
    /// 内面に何も残っていなければ、外界面は paradox。
    fn region(&mut self, body: &[Expr], span: Span) -> R<Eval> {
        let mut slot: Option<(Value, Span)> = None;
        let mut inner_paradox: Option<Span> = None;

        for e in body {
            match self.eval(e)? {
                Eval::Akasha => {}
                Eval::Escape(x) => return Ok(Eval::Escape(x)),
                Eval::Value(v) => {
                    if slot.is_some() || inner_paradox.is_some() {
                        return rt("一つの領域に値が二つある", e.span);
                    }
                    slot = Some((v, e.span));
                }
                Eval::Paradox(sp) => {
                    if slot.is_some() || inner_paradox.is_some() {
                        return rt("一つの領域に値が二つある", e.span);
                    }
                    inner_paradox = Some(sp);
                }
            }
        }

        Ok(match (slot, inner_paradox) {
            (Some((v, _)), _) => Eval::Value(v),
            (None, Some(sp)) => Eval::Paradox(sp),
            // 内面に何も残っていない → 外界面は paradox
            (None, None) => Eval::Paradox(span),
        })
    }

    /// 式を**領域の外界面として**読む。アーカーシャは paradox になる。
    fn face(&mut self, e: &Expr) -> R<Eval> {
        Ok(match self.eval(e)? {
            Eval::Akasha => Eval::Paradox(e.span),
            other => other,
        })
    }

    /// 値を要求する。**paradox が消費されずにここへ来たらエラー**（C-88 (1)）。
    fn need_value(&mut self, e: &Expr) -> R<Result<Value, EscapeVal>> {
        match self.face(e)? {
            Eval::Value(v) => Ok(Ok(v)),
            Eval::Escape(x) => Ok(Err(x)),
            Eval::Paradox(sp) => rt("消費されなかった paradox", sp),
            Eval::Akasha => unreachable!(),
        }
    }

    // ================= 式 =================

    fn eval(&mut self, e: &Expr) -> R<Eval> {
        self.depth_guard += 1;
        if self.depth_guard > 4096 {
            self.depth_guard -= 1;
            return rt("評価が深すぎる", e.span);
        }
        let r = self.eval_inner(e);
        self.depth_guard -= 1;
        r
    }

    fn eval_inner(&mut self, e: &Expr) -> R<Eval> {
        match &e.kind {
            ExprKind::Int(s) => Ok(Eval::Value(parse_int(s, e.span)?)),
            ExprKind::Float(s) => Ok(Eval::Value(parse_float(s, e.span)?)),
            ExprKind::Str(s) => Ok(Eval::Value(Value::Str(s.as_bytes().to_vec()))),

            ExprKind::Name(n) => {
                let Some(b) = self.lookup(n).cloned() else {
                    return rt(format!("知らない名前 `{n}`"), e.span);
                };
                match self.arena.get(b.cell) {
                    Some(v) => Ok(Eval::Value(v.clone())),
                    None => rt(format!("`{n}` はまだ束縛されていない"), e.span),
                }
            }

            // `( )` は領域を作るが、スコープでも脱出段でもない
            ExprKind::Paren(body) => self.region(body, e.span),

            // 裸のブロックは領域・スコープ・脱出段の三つを作る
            ExprKind::Block(body) => {
                self.push_scope(true, false, false);
                self.collect_decls(body);
                let r = self.region(body, e.span);
                self.pop_scope();
                self.pass_stage(r?, false, e.span)
            }

            ExprKind::Discard(inner) => {
                // `;` は左辺の**領域**を要求する。空でもよい（C-80）
                if let Some(x) = inner {
                    match self.eval(x)? {
                        Eval::Escape(esc) => return Ok(Eval::Escape(esc)),
                        _ => {}
                    }
                }
                Ok(Eval::Akasha)
            }

            ExprKind::Unary { op, rhs } => {
                let v = match self.need_value(rhs)? {
                    Ok(v) => v,
                    Err(x) => return Ok(Eval::Escape(x)),
                };
                unary(*op, v, e.span).map(Eval::Value)
            }

            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, e.span),

            ExprKind::Escape(esc) => self.make_escape(esc),

            ExprKind::Decl(d) => self.decl(d, e.span),
            ExprKind::Assign { op, lhs, rhs } => self.assign(*op, lhs, rhs, e.span),

            // 宣言は値を置かない。外界面は paradox
            ExprKind::FnDecl(_) | ExprKind::StructDecl(_) | ExprKind::FlowDecl(_) => {
                Ok(Eval::Paradox(e.span))
            }

            ExprKind::If(i) => self.if_expr(i, e.span),
            ExprKind::Loop(body) => self.loop_expr(body, None, e.span),
            ExprKind::While { cond, body } => self.while_expr(cond, body, e.span),
            ExprKind::NFor { name, start, count, body } => {
                self.nfor(name, start, count, body, e.span)
            }
            ExprKind::Switch { subject, arms } => self.switch(subject, arms, e.span),

            ExprKind::ArrayLit(items) => {
                let mut out = Vec::new();
                for it in items {
                    match self.need_value(it)? {
                        Ok(v) => out.push(v),
                        Err(x) => return Ok(Eval::Escape(x)),
                    }
                }
                let elem = out.first().map(|v| v.type_of()).unwrap_or(ValueType::I64);
                Ok(Eval::Value(Value::Array { elem, items: out }))
            }

            ExprKind::MapLit(pairs) => {
                let mut entries = std::collections::BTreeMap::new();
                let mut kt = ValueType::I64;
                let mut vt = ValueType::I64;
                for (k, v) in pairs {
                    let kv = match self.need_value(k)? {
                        Ok(v) => v,
                        Err(x) => return Ok(Eval::Escape(x)),
                    };
                    let vv = match self.need_value(v)? {
                        Ok(v) => v,
                        Err(x) => return Ok(Eval::Escape(x)),
                    };
                    kt = kv.type_of();
                    vt = vv.type_of();
                    let Some(key) = kv.as_key() else {
                        return rt("写像の鍵にできない値", k.span);
                    };
                    entries.insert(key, vv);
                }
                Ok(Eval::Value(Value::Map { key: kt, val: vt, entries }))
            }

            ExprKind::Construct { ty, args } => self.construct(ty, args, e.span),
            ExprKind::Field { base, name } => self.field(base, name, e.span),
            ExprKind::Index { base, index } => self.index(base, index, e.span),
            ExprKind::Call { callee, args } => self.call(callee, args, e.span),
        }
    }

    // ---- 段送り ----

    /// 脱出が段の境界に来たときの処理。
    /// **`break` は段を終結させ、`continue` は段を再開させる**（C-72）。
    fn pass_stage(&mut self, r: Eval, is_loop: bool, span: Span) -> R<Eval> {
        let Eval::Escape(mut x) = r else { return Ok(r) };
        if x.stages > 1 {
            x.stages -= 1;
            return Ok(Eval::Escape(x));
        }
        match x.kind {
            EKind::Break => Ok(match x.payload.take() {
                Some(v) => Eval::Value(v),
                // 値の無い脱出は何も置かない → 外界面は paradox
                None => Eval::Paradox(x.span),
            }),
            EKind::Continue => {
                if is_loop {
                    // ループ側が拾う。ここへは来ない
                    Ok(Eval::Escape(x))
                } else {
                    rt("再開できるものが無い（抜けた先がループではない）", span)
                }
            }
        }
    }

    // ---- 脱出を作る ----

    fn make_escape(&mut self, esc: &Escape) -> R<Eval> {
        match &esc.kind {
            EscapeKind::Break { outward } => {
                let base = if *outward { 1 } else { 0 };
                match &esc.operand {
                    // 被演算子が脱出なら**即時に合成する**。段数を足す（C-92）
                    Some(Operand::Escape(inner)) => {
                        let Eval::Escape(mut x) = self.make_escape(inner)? else {
                            return rt("脱出のはず", esc.span);
                        };
                        x.stages += 1;
                        x.outward += base;
                        Ok(Eval::Escape(x))
                    }
                    // 値なら**即時に評価する**（C-73）
                    Some(Operand::Value(v)) => {
                        let val = match self.need_value(v)? {
                            Ok(v) => v,
                            Err(x) => return Ok(Eval::Escape(x)),
                        };
                        Ok(Eval::Escape(EscapeVal {
                            kind: EKind::Break,
                            stages: 1,
                            outward: base,
                            payload: Some(val),
                            deferred: None,
                            span: esc.span,
                        }))
                    }
                    None => Ok(Eval::Escape(EscapeVal {
                        kind: EKind::Break,
                        stages: 1,
                        outward: base,
                        payload: None,
                        deferred: None,
                        span: esc.span,
                    })),
                }
            }
            EscapeKind::Continue => Ok(Eval::Escape(EscapeVal {
                kind: EKind::Continue,
                stages: 1,
                outward: 0,
                // **被演算子は即時に評価されない**（C-73）。段数も足さない（C-92）
                deferred: match &esc.operand {
                    Some(Operand::Escape(x)) => Some(x.clone()),
                    _ => None,
                },
                payload: None,
                span: esc.span,
            })),
            EscapeKind::Flow { name, args } => self.flow(name, args, &esc.operand, esc.span),
        }
    }

    /// 作用素式。**本体は使用位置で読み直される**（C-15）。
    fn flow(
        &mut self,
        name: &str,
        args: &[FlowArg],
        operand: &Option<Operand>,
        span: Span,
    ) -> R<Eval> {
        // 組み込みの組み合わせ子
        if name == "$repeat" {
            let [op, n] = args else {
                return rt("`$repeat` は作用素と回数を取る", span);
            };
            let FlowArg::Escape(inner) = op else {
                return rt("`$repeat` の第一引数は脱出に限る", span);
            };
            let FlowArg::Value(nv) = n else {
                return rt("`$repeat` の第二引数は回数", span);
            };
            let count = match self.need_value(nv)? {
                Ok(v) => v.as_int().unwrap_or(0),
                Err(x) => return Ok(Eval::Escape(x)),
            };
            let Eval::Escape(base) = self.make_escape(inner)? else {
                return rt("脱出のはず", span);
            };
            // `n` が 0 以下なら作用素を一つも重ねない（C-75 の 5）
            if count <= 0 {
                return self.operand_only(operand, span);
            }
            let mut x = base;
            x.stages = count as u32;
            if let Some(Operand::Value(v)) = operand {
                x.payload = match self.need_value(v)? {
                    Ok(v) => Some(v),
                    Err(e) => return Ok(Eval::Escape(e)),
                };
            }
            return Ok(Eval::Escape(x));
        }

        let Some(decl) = self.flows.get(name).cloned() else {
            return rt(format!("知らない作用素式 `{name}`"), span);
        };
        // 使用位置で読み直す
        let mut r = self.make_escape(&decl.body)?;
        if let (Eval::Escape(x), Some(Operand::Value(v))) = (&mut r, operand) {
            x.payload = match self.need_value(v)? {
                Ok(v) => Some(v),
                Err(e) => return Ok(Eval::Escape(e)),
            };
        }
        Ok(r)
    }

    fn operand_only(&mut self, operand: &Option<Operand>, span: Span) -> R<Eval> {
        match operand {
            Some(Operand::Value(v)) => self.face(v),
            Some(Operand::Escape(x)) => self.make_escape(x),
            None => Ok(Eval::Paradox(span)),
        }
    }
}

// ================= 数値 =================

fn parse_int(src: &str, span: Span) -> R<Value> {
    let clean: String = src.chars().filter(|c| *c != '_').collect();
    let v = if let Some(h) = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")) {
        i128::from_str_radix(h, 16)
    } else if let Some(b) = clean.strip_prefix("0b").or_else(|| clean.strip_prefix("0B")) {
        i128::from_str_radix(b, 2)
    } else if let Some(o) = clean.strip_prefix("0o").or_else(|| clean.strip_prefix("0O")) {
        i128::from_str_radix(o, 8)
    } else {
        clean.parse::<i128>()
    };
    match v {
        // 注釈が無ければ整数は i64
        Ok(n) => Ok(Value::I64(n as i64)),
        Err(_) => rt("整数として読めない", span),
    }
}

fn parse_float(src: &str, span: Span) -> R<Value> {
    let clean: String = src.chars().filter(|c| *c != '_').collect();
    match clean.parse::<f64>() {
        // **非有限な結果はすべて paradox**（C-84）だが、リテラル自体が非有限なら書けない
        Ok(v) if v.is_finite() => Ok(Value::F64(v)),
        _ => rt("浮動小数として読めない（非有限な値は書けない）", span),
    }
}

fn unary(op: UnOp, v: Value, span: Span) -> R<Value> {
    Ok(match op {
        UnOp::Pos => v,
        UnOp::Neg => match v {
            Value::F32(f) => Value::F32(-f),
            Value::F64(f) => Value::F64(-f),
            other => {
                let Some(i) = other.as_int() else {
                    return rt("符号反転できない", span);
                };
                wrap_like(&other, -i)
            }
        },
        UnOp::Not => match v {
            Value::U1(b) => Value::U1(!b),
            other => {
                let Some(i) = other.as_int() else {
                    return rt("否定できない", span);
                };
                wrap_like(&other, !i)
            }
        },
    })
}

/// **すべての整数演算は 2^N を法として行う**（C-79 (3)）。折り返しは失敗ではない。
pub fn wrap_like(sample: &Value, v: i128) -> Value {
    match sample {
        Value::U1(_) => Value::U1((v & 1) != 0),
        Value::U8(_) => Value::U8(v as u8),
        Value::U16(_) => Value::U16(v as u16),
        Value::U32(_) => Value::U32(v as u32),
        Value::I32(_) => Value::I32(v as i32),
        _ => Value::I64(v as i64),
    }
}

/// ユークリッド除算（C-76）。**余りは常に `[0, |b|)` に入る。**
pub fn euclid_div(a: i128, b: i128) -> (i128, i128) {
    let mut q = a / b;
    let mut r = a % b;
    if r < 0 {
        if b > 0 {
            q -= 1;
            r += b;
        } else {
            q += 1;
            r -= b;
        }
    }
    (q, r)
}

// ================= 演算・束縛・制御 =================

/// 経路。代入の左辺を解決した結果。
enum Place {
    Cell(CellId),
    Field(CellId, Vec<Step>),
}

#[derive(Clone, Debug)]
enum Step {
    Field(String),
    Index(i128),
    Key(MapKey),
}

impl Interp {
    // ---- 中置演算子 ----

    fn binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr, span: Span) -> R<Eval> {
        // `??` は paradox の**唯一の除去子**（C-22）。左を右で置き換えるだけで、右を検査しない
        if op == BinOp::Coalesce {
            return match self.face(lhs)? {
                Eval::Paradox(_) => self.face(rhs),
                other => Ok(other),
            };
        }
        // `&&` / `||` は短絡する
        if op == BinOp::And || op == BinOp::Or {
            let l = match self.need_value(lhs)? {
                Ok(v) => v,
                Err(x) => return Ok(Eval::Escape(x)),
            };
            let lb = l.is_nonzero();
            if (op == BinOp::And && !lb) || (op == BinOp::Or && lb) {
                return Ok(Eval::Value(Value::U1(lb)));
            }
            let r = match self.need_value(rhs)? {
                Ok(v) => v,
                Err(x) => return Ok(Eval::Escape(x)),
            };
            return Ok(Eval::Value(Value::U1(r.is_nonzero())));
        }
        // `|>` は構文の水準の糖衣（C-15）。`x |> f(a)` ≡ `f(x, a)`
        if op == BinOp::Feed {
            return self.feed(lhs, rhs, span);
        }

        let a = match self.need_value(lhs)? {
            Ok(v) => v,
            Err(x) => return Ok(Eval::Escape(x)),
        };
        let b = match self.need_value(rhs)? {
            Ok(v) => v,
            Err(x) => return Ok(Eval::Escape(x)),
        };
        arith(op, a, b, span)
    }

    fn feed(&mut self, lhs: &Expr, rhs: &Expr, span: Span) -> R<Eval> {
        let ExprKind::Call { callee, args } = &rhs.kind else {
            return rt("`|>` の右辺は呼び出しでなければならない", span);
        };
        let mut all = vec![lhs.clone()];
        all.extend(args.iter().cloned());
        self.call(callee, &all, span)
    }

    // ---- 宣言 ----

    fn decl(&mut self, d: &Decl, span: Span) -> R<Eval> {
        for b in &d.bindings {
            match &b.init {
                // `:=` は**深く複製する**（C-33）。値は自己完結しているので clone で足りる
                BindInit::Value(e) => {
                    let v = match self.need_value(e)? {
                        Ok(v) => v,
                        Err(x) => return Ok(Eval::Escape(x)),
                    };
                    let v = coerce(v, b.ty.as_ref());
                    let cell = self.arena.alloc(Some(v));
                    self.declare(&b.name, Binding { cell, kind: d.kind, is_alias: false });
                }
                // `&=` は**名前が別のセルを指すようにする**。対象は名前だけ（C-53）
                BindInit::AliasOf(target) => {
                    let Some(tb) = self.lookup(target).cloned() else {
                        return rt(format!("知らない名前 `{target}`"), b.span);
                    };
                    // **経路の権限は増やせない**（C-51）。凍結は常に許される
                    if !can_narrow(tb.kind, d.kind) {
                        return rt(
                            format!("権限が増える（`{:?}` から `{:?}` の別名は作れない）", tb.kind, d.kind),
                            b.span,
                        );
                    }
                    if d.kind == BindKind::Const {
                        // **そのセルが誰からも書けない**（C-78）。字句的に解ける
                        self.frozen.push(tb.cell);
                        self.scopes.last_mut().unwrap().frozen.push(tb.cell);
                    }
                    self.declare(&b.name, Binding { cell: tb.cell, kind: d.kind, is_alias: true });
                }
            }
        }
        // 宣言は値を置かない。外界面は paradox
        Ok(Eval::Paradox(span))
    }

    // ---- 代入 ----

    fn assign(&mut self, op: AssignOp, lhs: &Expr, rhs: &Expr, span: Span) -> R<Eval> {
        // `b &= c` は名前を動かす。セルには書かない
        if op == AssignOp::Alias {
            let (ExprKind::Name(n), ExprKind::Name(t)) = (&lhs.kind, &rhs.kind) else {
                return rt("`&=` は名前どうしでなければならない", span);
            };
            let Some(cur) = self.lookup(n).cloned() else {
                return rt(format!("知らない名前 `{n}`"), lhs.span);
            };
            if !cur.is_alias {
                return rt(format!("`{n}` は別名ではないので指し直せない"), span);
            }
            if cur.kind != BindKind::Var {
                return rt("指し直せるのは `var` の別名だけ", span);
            }
            let Some(tb) = self.lookup(t).cloned() else {
                return rt(format!("知らない名前 `{t}`"), rhs.span);
            };
            if !can_narrow(tb.kind, cur.kind) {
                return rt("権限が増える", span);
            }
            let cell = tb.cell;
            for s in self.scopes[self.frame_base..].iter_mut().rev() {
                if let Some(b) = s.vars.get_mut(n) {
                    b.cell = cell;
                    break;
                }
            }
            return Ok(Eval::Paradox(span));
        }

        // **左辺の場所を先に解決してから**右辺を評価する（C-79 (6)）
        let place = self.resolve_place(lhs)?;
        let rv = match self.need_value(rhs)? {
            Ok(v) => v,
            Err(x) => return Ok(Eval::Escape(x)),
        };
        let cell = match &place {
            Place::Cell(c) => *c,
            Place::Field(c, _) => *c,
        };
        if self.frozen.contains(&cell) {
            return rt("凍っているセルには書けない（`const` の別名がある）", span);
        }
        let new = if op == AssignOp::Set {
            rv
        } else {
            let cur = self.read_place(&place, span)?;
            let bop = match op {
                AssignOp::Add => BinOp::Add,
                AssignOp::Sub => BinOp::Sub,
                AssignOp::Mul => BinOp::Mul,
                AssignOp::Div => BinOp::Div,
                AssignOp::Mod => BinOp::Mod,
                AssignOp::Shl => BinOp::Shl,
                AssignOp::Shr => BinOp::Shr,
                AssignOp::BitXor => BinOp::BitXor,
                AssignOp::BitOr => BinOp::BitOr,
                _ => unreachable!(),
            };
            match arith(bop, cur, rv, span)? {
                Eval::Value(v) => v,
                // 0 除算などは paradox。代入は成立しない
                Eval::Paradox(sp) => return rt("消費されなかった paradox", sp),
                other => return Ok(other),
            }
        };
        self.write_place(&place, new, span)?;
        // 代入は値を置かない。外界面は paradox
        Ok(Eval::Paradox(span))
    }

    fn resolve_place(&mut self, e: &Expr) -> R<Place> {
        match &e.kind {
            ExprKind::Name(n) => {
                let Some(b) = self.lookup(n).cloned() else {
                    return rt(format!("知らない名前 `{n}`"), e.span);
                };
                if b.kind != BindKind::Var {
                    return rt(format!("`{n}` は書けない（`{:?}` で束縛されている）", b.kind), e.span);
                }
                Ok(Place::Cell(b.cell))
            }
            ExprKind::Field { base, name } => {
                let p = self.resolve_place(base)?;
                Ok(push_step(p, Step::Field(name.clone())))
            }
            ExprKind::Index { base, index } => {
                let p = self.resolve_place(base)?;
                let iv = match self.need_value(index)? {
                    Ok(v) => v,
                    Err(_) => return rt("添字の位置で脱出した", index.span),
                };
                let step = match &iv {
                    Value::Str(_) | Value::F32(_) | Value::F64(_) => {
                        Step::Key(iv.as_key().unwrap())
                    }
                    _ => match iv.as_int() {
                        Some(i) => Step::Index(i),
                        None => return rt("添字にできない値", index.span),
                    },
                };
                Ok(push_step(p, step))
            }
            _ => rt("代入の左辺は経路でなければならない", e.span),
        }
    }

    fn read_place(&mut self, p: &Place, span: Span) -> R<Value> {
        let (cell, steps) = match p {
            Place::Cell(c) => (*c, &[][..]),
            Place::Field(c, s) => (*c, &s[..]),
        };
        let Some(mut cur) = self.arena.get(cell).cloned() else {
            return rt("まだ束縛されていない", span);
        };
        for st in steps {
            cur = match step_get(&cur, st) {
                Some(v) => v,
                None => return rt("経路がたどれない", span),
            };
        }
        Ok(cur)
    }

    fn write_place(&mut self, p: &Place, v: Value, span: Span) -> R<()> {
        match p {
            Place::Cell(c) => {
                self.arena.set(*c, v);
                Ok(())
            }
            Place::Field(c, steps) => {
                let Some(root) = self.arena.get_mut(*c) else {
                    return rt("まだ束縛されていない", span);
                };
                if !step_set(root, steps, v) {
                    return rt("経路がたどれない", span);
                }
                Ok(())
            }
        }
    }
}

fn push_step(p: Place, s: Step) -> Place {
    match p {
        Place::Cell(c) => Place::Field(c, vec![s]),
        Place::Field(c, mut v) => {
            v.push(s);
            Place::Field(c, v)
        }
    }
}

fn step_get(v: &Value, s: &Step) -> Option<Value> {
    match (v, s) {
        (Value::Struct { fields, .. }, Step::Field(n)) => {
            fields.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone())
        }
        (Value::Array { items, .. }, Step::Index(i)) => {
            if *i < 0 {
                None
            } else {
                items.get(*i as usize).cloned()
            }
        }
        (Value::Str(b), Step::Index(i)) => {
            if *i < 0 {
                None
            } else {
                b.get(*i as usize).map(|x| Value::U8(*x))
            }
        }
        (Value::Map { entries, .. }, Step::Key(k)) => entries.get(k).cloned(),
        (Value::Map { entries, .. }, Step::Index(i)) => entries.get(&MapKey::Int(*i)).cloned(),
        _ => None,
    }
}

fn step_set(v: &mut Value, steps: &[Step], new: Value) -> bool {
    let Some((first, rest)) = steps.split_first() else {
        *v = new;
        return true;
    };
    match (v, first) {
        (Value::Struct { fields, .. }, Step::Field(n)) => {
            match fields.iter_mut().find(|(k, _)| k == n) {
                Some((_, slot)) => step_set(slot, rest, new),
                None => false,
            }
        }
        (Value::Array { items, .. }, Step::Index(i)) => {
            if *i < 0 {
                return false;
            }
            match items.get_mut(*i as usize) {
                Some(slot) => step_set(slot, rest, new),
                None => false,
            }
        }
        (Value::Str(b), Step::Index(i)) => {
            if *i < 0 {
                return false;
            }
            match (b.get_mut(*i as usize), &new) {
                (Some(slot), Value::U8(x)) if rest.is_empty() => {
                    *slot = *x;
                    true
                }
                _ => false,
            }
        }
        (Value::Map { entries, .. }, Step::Key(k)) => {
            if rest.is_empty() {
                entries.insert(k.clone(), new);
                true
            } else {
                match entries.get_mut(k) {
                    Some(slot) => step_set(slot, rest, new),
                    None => false,
                }
            }
        }
        (Value::Map { entries, .. }, Step::Index(i)) => {
            let k = MapKey::Int(*i);
            if rest.is_empty() {
                entries.insert(k, new);
                true
            } else {
                match entries.get_mut(&k) {
                    Some(slot) => step_set(slot, rest, new),
                    None => false,
                }
            }
        }
        _ => false,
    }
}

/// **凍結は常に許される**——能力を奪うだけで、与えないから（C-51）。
fn can_narrow(from: BindKind, to: BindKind) -> bool {
    match (from, to) {
        (BindKind::Var, _) => true,
        (BindKind::Let, BindKind::Var) => false,
        (BindKind::Let, _) => true,
        (BindKind::Const, BindKind::Const) => true,
        (BindKind::Const, _) => false,
    }
}

/// 注釈があればリテラルの型を決める（C-31：インタプリタは型を解決するが検査はしない）。
fn coerce(v: Value, ty: Option<&Type>) -> Value {
    let Some(t) = ty else { return v };
    match (&t.value, &v) {
        (ValueType::U1, _) => v.as_int().map(|i| Value::U1(i != 0)).unwrap_or(v),
        (ValueType::U8, _) => v.as_int().map(|i| Value::U8(i as u8)).unwrap_or(v),
        (ValueType::U16, _) => v.as_int().map(|i| Value::U16(i as u16)).unwrap_or(v),
        (ValueType::U32, _) => v.as_int().map(|i| Value::U32(i as u32)).unwrap_or(v),
        (ValueType::I32, _) => v.as_int().map(|i| Value::I32(i as i32)).unwrap_or(v),
        (ValueType::I64, _) => v.as_int().map(|i| Value::I64(i as i64)).unwrap_or(v),
        (ValueType::F32, _) => v.as_float().map(|f| Value::F32(f as f32)).unwrap_or(v),
        (ValueType::F64, _) => v.as_float().map(Value::F64).unwrap_or(v),
        _ => v,
    }
}

/// 算術。**0 除算は paradox。浮動小数の非有限な結果もすべて paradox**（C-84）。
fn arith(op: BinOp, a: Value, b: Value, span: Span) -> R<Eval> {
    use BinOp::*;
    // 比較の結果は `u1`
    if matches!(op, Lt | Le | Gt | Ge | Eq | Ne) {
        let ord = compare(&a, &b);
        let Some(ord) = ord else {
            return rt("比較できない値どうし", span);
        };
        let r = match op {
            Lt => ord.is_lt(),
            Le => ord.is_le(),
            Gt => ord.is_gt(),
            Ge => ord.is_ge(),
            Eq => ord.is_eq(),
            _ => ord.is_ne(),
        };
        return Ok(Eval::Value(Value::U1(r)));
    }

    if let (Some(x), Some(y)) = (a.as_float(), b.as_float()) {
        if matches!(a, Value::F32(_) | Value::F64(_)) {
            let r = match op {
                Add => x + y,
                Sub => x - y,
                Mul => x * y,
                Div => x / y,
                Mod => x % y,
                _ => return rt("浮動小数に使えない演算子", span),
            };
            // **inf も NaN も、この言語には存在しない**
            if !r.is_finite() {
                return Ok(Eval::Paradox(span));
            }
            return Ok(Eval::Value(match a {
                Value::F32(_) => Value::F32(r as f32),
                _ => Value::F64(r),
            }));
        }
    }

    let (Some(x), Some(y)) = (a.as_int(), b.as_int()) else {
        return rt("この演算子は数値にしか使えない", span);
    };
    let r = match op {
        Add => x.wrapping_add(y),
        Sub => x.wrapping_sub(y),
        Mul => x.wrapping_mul(y),
        Div | Mod => {
            if y == 0 {
                return Ok(Eval::Paradox(span));
            }
            let (q, m) = euclid_div(x, y);
            if op == Div {
                q
            } else {
                m
            }
        }
        Shl => x.wrapping_shl(y as u32),
        Shr => x.wrapping_shr(y as u32),
        BitAnd => x & y,
        BitXor => x ^ y,
        BitOr => x | y,
        _ => return rt("使えない演算子", span),
    };
    Ok(Eval::Value(wrap_like(&a, r)))
}

fn compare(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    if let (Value::Str(x), Value::Str(y)) = (a, b) {
        return Some(x.cmp(y));
    }
    if let (Some(x), Some(y)) = (a.as_float(), b.as_float()) {
        if matches!(a, Value::F32(_) | Value::F64(_)) || matches!(b, Value::F32(_) | Value::F64(_))
        {
            // **NaN が無いので比較は全順序である**（C-75）
            return x.partial_cmp(&y);
        }
    }
    match (a.as_int(), b.as_int()) {
        (Some(x), Some(y)) => Some(x.cmp(&y)),
        _ => {
            if a == b {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
    }
}

// ================= 制御構造 =================

impl Interp {
    /// `if` は演算子。**分岐は被演算子位置なので領域だが、スコープでも脱出段でもない。**
    fn if_expr(&mut self, i: &If, span: Span) -> R<Eval> {
        for (cond, body) in &i.arms {
            let c = match self.need_value(cond)? {
                Ok(v) => v,
                Err(x) => return Ok(Eval::Escape(x)),
            };
            let Value::U1(b) = c else {
                return rt("`if` の条件は `u1` でなければならない", cond.span);
            };
            if b {
                return self.face(body);
            }
        }
        match &i.els {
            Some(e) => self.face(e),
            // `else` の無い `if` で条件が偽 → paradox
            None => Ok(Eval::Paradox(span)),
        }
    }

    /// ループ本体は**その構文の段**であって、二重にはならない（C-64）。
    ///
    /// **正常終了したループの値は反復回数。脱出で終わったループの値は脱出が置いた値**（C-62）。
    fn loop_expr(&mut self, body: &Expr, mut limit: Option<i64>, span: Span) -> R<Eval> {
        let mut count: i64 = 0;
        // `continue` の遅延した被演算子（C-73）
        let mut pending: Option<Box<Escape>> = None;
        loop {
            if let Some(n) = limit {
                if count >= n {
                    break;
                }
            }
            match self.run_body(body, &mut pending, None)? {
                BodyOut::Normal => count = count.wrapping_add(1),
                BodyOut::Continue(next) => {
                    pending = next;
                    count = count.wrapping_add(1);
                }
                BodyOut::Break(v) => {
                    return Ok(match v {
                        Some(v) => Eval::Value(v),
                        None => Eval::Paradox(span),
                    })
                }
                BodyOut::Escape(x) => return Ok(Eval::Escape(x)),
            }
            if limit.is_none() {
                // `loop` は無限。**発散はエラーではない**
                limit = None;
            }
        }
        // 0 周なら領域が空になり paradox（C-44）
        if count == 0 {
            return Ok(Eval::Paradox(span));
        }
        Ok(Eval::Value(Value::I64(count)))
    }

    fn while_expr(&mut self, cond: &Expr, body: &Expr, span: Span) -> R<Eval> {
        let mut count: i64 = 0;
        let mut pending: Option<Box<Escape>> = None;
        loop {
            let c = match self.need_value(cond)? {
                Ok(v) => v,
                Err(x) => return Ok(Eval::Escape(x)),
            };
            // 条件は**任意の整数型。`≠ 0` で継続**
            if !c.is_nonzero() {
                break;
            }
            match self.run_body(body, &mut pending, None)? {
                BodyOut::Normal => count = count.wrapping_add(1),
                BodyOut::Continue(next) => {
                    pending = next;
                    count = count.wrapping_add(1);
                }
                BodyOut::Break(v) => {
                    return Ok(match v {
                        Some(v) => Eval::Value(v),
                        None => Eval::Paradox(span),
                    })
                }
                BodyOut::Escape(x) => return Ok(Eval::Escape(x)),
            }
        }
        if count == 0 {
            return Ok(Eval::Paradox(span));
        }
        Ok(Eval::Value(Value::I64(count)))
    }

    /// `nfor` の三被演算子は**束縛名・開始・回数**。歩幅は無い。
    /// 束縛名は本体の中で `let` であり、**スコープは本体**。
    fn nfor(
        &mut self,
        name: &str,
        start: &Expr,
        count: &Expr,
        body: &Expr,
        span: Span,
    ) -> R<Eval> {
        let sv = match self.need_value(start)? {
            Ok(v) => v,
            Err(x) => return Ok(Eval::Escape(x)),
        };
        let cv = match self.need_value(count)? {
            Ok(v) => v,
            Err(x) => return Ok(Eval::Escape(x)),
        };
        // 回数の型は `i64`、`i` の型は開始値の型（C-86）
        let n = cv.as_int().unwrap_or(0);
        let s0 = sv.as_int().unwrap_or(0);
        // 回数が 0 以下なら 0 周 → paradox
        if n <= 0 {
            return Ok(Eval::Paradox(span));
        }
        let mut iters: i64 = 0;
        let mut pending: Option<Box<Escape>> = None;
        for k in 0..n {
            // `i` の増加は通常の整数演算（溢れれば折り返す）
            let iv = wrap_like(&sv, s0.wrapping_add(k));
            match self.run_body(body, &mut pending, Some((name, iv)))? {
                BodyOut::Normal | BodyOut::Continue(None) => iters = iters.wrapping_add(1),
                BodyOut::Continue(next) => {
                    pending = next;
                    iters = iters.wrapping_add(1);
                }
                BodyOut::Break(v) => {
                    return Ok(match v {
                        Some(v) => Eval::Value(v),
                        None => Eval::Paradox(span),
                    })
                }
                BodyOut::Escape(x) => return Ok(Eval::Escape(x)),
            }
        }
        Ok(Eval::Value(Value::I64(iters)))
    }

    /// ループ本体を一周。**ループが受け取るブロックの中身は、何も残ってはいけない。**
    fn run_body(
        &mut self,
        body: &Expr,
        pending: &mut Option<Box<Escape>>,
        loop_var: Option<(&str, Value)>,
    ) -> R<BodyOut> {
        let ExprKind::Block(items) = &body.kind else {
            return rt("ループの本体はブロックでなければならない", body.span);
        };
        // 本体はループの段。**二重にはならない**
        self.push_scope(true, true, false);
        if let Some((n, v)) = loop_var {
            let cell = self.arena.alloc(Some(v));
            self.declare(n, Binding { cell, kind: BindKind::Let, is_alias: false });
        }
        self.collect_decls(items);

        // 遅延した被演算子は**再開した本体の先頭**で実行される（C-73）
        let mut r: Option<Eval> = None;
        if let Some(esc) = pending.take() {
            r = Some(self.make_escape(&esc)?);
        }
        let out = match r {
            Some(x) => Ok(x),
            None => self.region(items, body.span),
        };
        let out = match out {
            Ok(v) => v,
            Err(e) => {
                self.pop_scope();
                return Err(e);
            }
        };
        self.pop_scope();

        Ok(match out {
            Eval::Escape(mut x) => {
                if x.stages > 1 {
                    x.stages -= 1;
                    BodyOut::Escape(x)
                } else {
                    match x.kind {
                        EKind::Break => BodyOut::Break(x.payload.take()),
                        EKind::Continue => BodyOut::Continue(x.deferred.take()),
                    }
                }
            }
            Eval::Akasha | Eval::Paradox(_) => BodyOut::Normal,
            Eval::Value(_) => return rt("ループの本体に値が残っている", body.span),
        })
    }

    /// `switch` は閉じる語を持たない。腕は**上から順に照合**し、**当たった腕の値だけを評価する**。
    fn switch(&mut self, subject: &Expr, arms: &[Arm], span: Span) -> R<Eval> {
        let s = match self.need_value(subject)? {
            Ok(v) => v,
            Err(x) => return Ok(Eval::Escape(x)),
        };
        for a in arms {
            let p = match self.need_value(&a.pattern)? {
                Ok(v) => v,
                Err(x) => return Ok(Eval::Escape(x)),
            };
            if compare(&s, &p).map(|o| o.is_eq()).unwrap_or(false) {
                return self.face(&a.value);
            }
        }
        // どの腕にも当たらない → paradox
        Ok(Eval::Paradox(span))
    }
}

enum BodyOut {
    Normal,
    Continue(Option<Box<Escape>>),
    Break(Option<Value>),
    Escape(EscapeVal),
}

// ================= 構築・アクセス・呼び出し =================

impl Interp {
    fn construct(&mut self, ty: &Type, args: &CtorArgs, span: Span) -> R<Eval> {
        match (&ty.value, args) {
            (ValueType::Named(name), CtorArgs::Named(fields)) => {
                let Some(decl) = self.structs.get(name).cloned() else {
                    return rt(format!("知らない型 `{name}`"), span);
                };
                let mut out = Vec::new();
                for f in &decl.fields {
                    let given = fields.iter().find(|(n, _)| n == &f.name);
                    let v = if let Some((_, e)) = given {
                        match self.need_value(e)? {
                            Ok(v) => v,
                            Err(x) => return Ok(Eval::Escape(x)),
                        }
                    } else if let Some(d) = &f.default {
                        match self.need_value(d)? {
                            Ok(v) => v,
                            Err(x) => return Ok(Eval::Escape(x)),
                        }
                    } else {
                        return rt(format!("欄 `{}` に値が無い", f.name), span);
                    };
                    out.push((f.name.clone(), coerce(v, Some(&f.ty))));
                }
                Ok(Eval::Value(Value::Struct { name: name.clone(), fields: out }))
            }
            (ValueType::Array(elem), CtorArgs::Positional(a)) => {
                let (n, fill) = match a.len() {
                    0 => (0, Value::I64(0)),
                    _ => {
                        let n = match self.need_value(&a[0])? {
                            Ok(v) => v.as_int().unwrap_or(0),
                            Err(x) => return Ok(Eval::Escape(x)),
                        };
                        let f = if a.len() > 1 {
                            match self.need_value(&a[1])? {
                                Ok(v) => v,
                                Err(x) => return Ok(Eval::Escape(x)),
                            }
                        } else {
                            Value::I64(0)
                        };
                        (n.max(0), f)
                    }
                };
                Ok(Eval::Value(Value::Array {
                    elem: (**elem).clone(),
                    items: vec![fill; n as usize],
                }))
            }
            (ValueType::Map(k, v), CtorArgs::Positional(_)) => Ok(Eval::Value(Value::Map {
                key: (**k).clone(),
                val: (**v).clone(),
                entries: Default::default(),
            })),
            // ラップ型：包むのも剥がすのも `new`（C-78）
            (ValueType::Str, CtorArgs::Positional(a)) if a.len() == 1 => {
                match self.need_value(&a[0])? {
                    Ok(Value::Array { items, .. }) => {
                        let b: Vec<u8> =
                            items.iter().filter_map(|v| v.as_int()).map(|i| i as u8).collect();
                        Ok(Eval::Value(Value::Str(b)))
                    }
                    Ok(Value::Str(b)) => Ok(Eval::Value(Value::Str(b))),
                    Ok(_) => rt("`str` は `u8 array` から作る", span),
                    Err(x) => Ok(Eval::Escape(x)),
                }
            }
            (ValueType::Array(_), CtorArgs::Named(_)) | (_, CtorArgs::Named(_)) => {
                rt("この型は欄を名前で取らない", span)
            }
            _ => rt("この型は構築できない", span),
        }
    }

    /// `.` は**アクセスであり複製しない**（C-20）。ここでは値を読むだけ。
    fn field(&mut self, base: &Expr, name: &str, span: Span) -> R<Eval> {
        let b = match self.need_value(base)? {
            Ok(v) => v,
            Err(x) => return Ok(Eval::Escape(x)),
        };
        match step_get(&b, &Step::Field(name.to_string())) {
            Some(v) => Ok(Eval::Value(v)),
            None => rt(format!("欄 `{name}` が無い"), span),
        }
    }

    fn index(&mut self, base: &Expr, index: &Expr, span: Span) -> R<Eval> {
        let b = match self.need_value(base)? {
            Ok(v) => v,
            Err(x) => return Ok(Eval::Escape(x)),
        };
        let i = match self.need_value(index)? {
            Ok(v) => v,
            Err(x) => return Ok(Eval::Escape(x)),
        };
        let step = match (&b, &i) {
            (Value::Map { .. }, _) => match i.as_key() {
                Some(k) => Step::Key(k),
                None => return rt("写像の鍵にできない値", index.span),
            },
            _ => match i.as_int() {
                Some(n) => Step::Index(n),
                None => return rt("添字にできない値", index.span),
            },
        };
        match step_get(&b, &step) {
            Some(v) => Ok(Eval::Value(v)),
            // 配列の範囲外・写像の欠損キーは paradox
            None => Ok(Eval::Paradox(span)),
        }
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> R<Eval> {
        // メンバ関数。**レシーバは経路でよい**（C-64）
        if let ExprKind::Field { base, name } = &callee.kind {
            return self.method(base, name, args, span);
        }
        let ExprKind::Name(name) = &callee.kind else {
            return rt("呼び出せるのは名前だけ（第一級関数は無い）", callee.span);
        };
        // 組み込み
        if name == "getdepth" {
            return Ok(Eval::Value(Value::I64(self.depth())));
        }
        let Some(f) = self.fns.get(name).cloned() else {
            return rt(format!("知らない関数 `{name}`"), callee.span);
        };
        if f.decl.params.len() != args.len() {
            return rt(
                format!("`{name}` は引数を {} 個取るが {} 個来た", f.decl.params.len(), args.len()),
                span,
            );
        }

        // **非 alias の引数は深く複製する**（C-85）
        let mut bound: Vec<(String, BindKind, CellId, bool)> = Vec::new();
        for (p, a) in f.decl.params.iter().zip(args) {
            if p.ty.is_alias {
                // `alias` を受ける引数に渡せるのは**名前だけ**（C-53）
                let ExprKind::Name(n) = &a.kind else {
                    return rt("`alias` 引数に渡せるのは名前だけ", a.span);
                };
                let Some(b) = self.lookup(n).cloned() else {
                    return rt(format!("知らない名前 `{n}`"), a.span);
                };
                if !can_narrow(b.kind, p.kind) {
                    return rt("権限が増える", a.span);
                }
                bound.push((p.name.clone(), p.kind, b.cell, true));
            } else {
                let v = match self.need_value(a)? {
                    Ok(v) => v,
                    Err(x) => return Ok(Eval::Escape(x)),
                };
                let cell = self.arena.alloc(Some(coerce(v, Some(&p.ty))));
                bound.push((p.name.clone(), p.kind, cell, false));
            }
        }
        // **同じ呼び出しで、同じセルに届く別名を二つ以上渡せない**（C-87）
        let cells: Vec<CellId> = bound.iter().filter(|b| b.3).map(|b| b.2).collect();
        for i in 0..cells.len() {
            for j in i + 1..cells.len() {
                if cells[i] == cells[j] {
                    return rt("同じセルに別名が二つ届く", span);
                }
            }
        }

        // 関数本体は領域・スコープ・脱出段（**フレーム**）
        let saved_base = self.frame_base;
        self.push_scope(true, false, true);
        self.frame_base = self.scopes.len() - 1;
        for (n, k, c, is_alias) in bound {
            self.declare(&n, Binding { cell: c, kind: k, is_alias });
        }
        let ExprKind::Block(items) = &f.decl.body.kind else {
            self.frame_base = saved_base;
            self.pop_scope();
            return rt("関数の本体はブロックでなければならない", span);
        };
        self.collect_decls(items);
        let r = self.region(items, f.decl.body.span);
        self.frame_base = saved_base;
        self.pop_scope();

        let r = r?;
        // 脱出は**フレームで止まる**。**上限まで抜けるのは許される**（C-66）
        Ok(match r {
            Eval::Escape(mut x) => {
                if x.stages > 1 {
                    if x.outward > 0 {
                        x.outward -= 1;
                        x.stages -= 1;
                        Eval::Escape(x)
                    } else {
                        return rt("フレームを越える脱出", x.span);
                    }
                } else {
                    match x.kind {
                        EKind::Break => match x.payload.take() {
                            Some(v) => Eval::Value(v),
                            None => Eval::Paradox(x.span),
                        },
                        EKind::Continue => return rt("再開できるものが無い", x.span),
                    }
                }
            }
            Eval::Akasha => Eval::Paradox(span),
            other => other,
        })
    }

    /// 組み込みのメンバ関数。**レシーバは複製されない。破壊的である**（C-20）。
    fn method(&mut self, base: &Expr, name: &str, args: &[Expr], span: Span) -> R<Eval> {
        // 読むだけのもの
        if name == "len" {
            let b = match self.need_value(base)? {
                Ok(v) => v,
                Err(x) => return Ok(Eval::Escape(x)),
            };
            let n = match &b {
                Value::Array { items, .. } => items.len(),
                Value::Str(s) => s.len(),
                Value::Map { entries, .. } => entries.len(),
                _ => return rt("`len` は集合体にしか使えない", span),
            };
            return Ok(Eval::Value(Value::I64(n as i64)));
        }
        // 破壊的なもの。**レシーバに `var` を要求する**（C-64）
        if name == "push" {
            let place = self.resolve_place(base)?;
            let cell = match &place {
                Place::Cell(c) | Place::Field(c, _) => *c,
            };
            if self.frozen.contains(&cell) {
                return rt("凍っているセルには書けない", span);
            }
            let [a] = args else {
                return rt("`push` は値を一つ取る", span);
            };
            let v = match self.need_value(a)? {
                Ok(v) => v,
                Err(x) => return Ok(Eval::Escape(x)),
            };
            let mut cur = self.read_place(&place, span)?;
            match (&mut cur, &v) {
                (Value::Array { items, .. }, _) => items.push(v.clone()),
                (Value::Str(b), Value::U8(x)) => b.push(*x),
                _ => return rt("`push` は配列か `str` にしか使えない", span),
            }
            self.write_place(&place, cur, span)?;
            return Ok(Eval::Paradox(span));
        }
        rt(format!("知らないメンバ関数 `{name}`"), span)
    }
}

impl Default for Interp {
    fn default() -> Self {
        Self::new()
    }
}

/// 走らせて、最上位の外界面を返す。
pub fn run(src: &str) -> Result<Eval, String> {
    let prog = crate::parser::parse(src).map_err(|e| format!("構文: {}", e.msg))?;
    let mut it = Interp::new();
    it.run(&prog).map_err(|e| e.msg)
}
