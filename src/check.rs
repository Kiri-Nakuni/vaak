//! 静的検査。**インタプリタとは独立している**（C-31）。
//!
//! 検査するのは型だけではない——**領域の値の数・脱出の段数・権限・別名・宣言の位置**。
//! 「走らせる前に分かるので、数を絞る必要が無い」（C-61）。
//!
//! **paradox の消費は検査しない。** それは実行時（値）のエラーである。

use crate::ast::*;
use crate::span::Span;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct StaticError {
    pub msg: String,
    pub span: Span,
}

/// 式が「正常に続く経路で」何を置くか。**脱出する経路は値を置かない**（C-62）。
#[derive(Clone, Copy, PartialEq, Debug)]
enum Places {
    /// 何も置かない（`;` の後）。
    Nothing,
    /// 値を置く。
    Value,
    /// paradox を置く（宣言・代入・空の領域など）。
    Paradox,
    /// 正常に続く経路が無い（必ず脱出する）。
    Never,
}

impl Places {
    /// 領域に「実在するもの」を置くか。数えるのはこれ。
    fn occupies(self) -> bool {
        matches!(self, Places::Value | Places::Paradox)
    }
}

/// 脱出段。**裸のブロック・ループ本体・関数本体だけ**が作る（C-64）。
#[derive(Clone, Copy, PartialEq, Debug)]
struct Stage {
    is_loop: bool,
    is_frame: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct Env;

struct Checker {
    errs: Vec<StaticError>,
    /// 名前 → 束縛種。スコープの鎖。
    scopes: Vec<HashMap<String, (BindKind, bool)>>,
    /// 変数の探索はここより外へ行かない。**関数は局所変数を見ない**（C-86）。
    frame_base: usize,
    fns: HashMap<String, FnDecl>,
    structs: HashMap<String, StructDecl>,
    wraps: HashMap<String, ValueType>,
    flows: HashSet<String>,
    /// 内側から外へ。段送りはこの順に起きる（C-70）。
    stages: Vec<Stage>,
    /// 名前が見える範囲の上限。**`continue` の被演算子は本体の先頭で見える名前だけ**（C-81）。
    visible_limit: Option<usize>,
}

pub fn check(prog: &Program) -> Vec<StaticError> {
    check_with_host(prog, &[])
}

/// ホストが見せている名前を添えて検査する（S-4）。**`var` で見える。**
pub fn check_with_host(prog: &Program, host: &[(String, ValueType)]) -> Vec<StaticError> {
    let mut c = Checker {
        errs: Vec::new(),
        scopes: vec![HashMap::new()],
        frame_base: 0,
        fns: HashMap::new(),
        structs: HashMap::new(),
        wraps: HashMap::new(),
        flows: HashSet::new(),
        stages: vec![Stage { is_loop: false, is_frame: true }],
        visible_limit: None,
    };
    for (n, _) in host {
        c.scopes[0].insert(n.clone(), (BindKind::Var, false));
    }
    c.flows.insert("$return".into());
    c.flows.insert("$repeat".into());
    c.collect(&prog.body);
    c.check_struct_dag();
    c.region(&prog.body, Env, Span::NONE, RegionKind::Normal);
    c.errs
}

#[derive(Clone, Copy, PartialEq)]
enum RegionKind {
    Normal,
    /// ループ本体。**中身は何も残ってはいけない**（C-3）。
    LoopBody,
}

impl Checker {
    fn err(&mut self, msg: impl Into<String>, span: Span) {
        self.errs.push(StaticError { msg: msg.into(), span });
    }

    fn collect(&mut self, body: &[Expr]) {
        for e in body {
            let mut e = e;
            while let ExprKind::Discard(Some(i)) = &e.kind {
                e = i;
            }
            match &e.kind {
                ExprKind::FnDecl(f) => {
                    self.fns.insert(crate::ast::fn_key(f), f.clone());
                }
                ExprKind::StructDecl(s) => {
                    self.structs.insert(s.name.clone(), s.clone());
                }
                ExprKind::WrapDecl(w) => {
                    self.wraps.insert(w.name.clone(), w.base.value.clone());
                }
                ExprKind::FlowDecl(f) => {
                    self.flows.insert(f.name.clone());
                }
                _ => {}
            }
        }
    }

    fn lookup(&self, n: &str) -> Option<(BindKind, bool)> {
        let end = self.visible_limit.unwrap_or(self.scopes.len()).min(self.scopes.len());
        if end <= self.frame_base {
            return None;
        }
        for s in self.scopes[self.frame_base..end].iter().rev() {
            if let Some(b) = s.get(n) {
                return Some(*b);
            }
        }
        None
    }

    fn declare(&mut self, n: &str, k: BindKind, is_alias: bool) {
        self.scopes.last_mut().unwrap().insert(n.to_string(), (k, is_alias));
    }

    /// **構造体の型依存グラフは DAG でなければならない**（C-63）。
    fn check_struct_dag(&mut self) {
        let names: Vec<String> = self.structs.keys().cloned().collect();
        for n in names {
            let mut seen = HashSet::new();
            if self.reaches(&n, &n, &mut seen) {
                let span = self.structs[&n].span;
                self.err(format!("型 `{n}` の依存が循環している"), span);
            }
        }
    }

    fn reaches(&self, from: &str, target: &str, seen: &mut HashSet<String>) -> bool {
        let Some(s) = self.structs.get(from) else { return false };
        for f in &s.fields {
            for dep in type_deps(&f.ty.value) {
                if dep == target {
                    return true;
                }
                if seen.insert(dep.clone()) && self.reaches(&dep, target, seen) {
                    return true;
                }
            }
        }
        false
    }

    // ---- 領域 ----

    fn region(&mut self, body: &[Expr], env: Env, span: Span, kind: RegionKind) -> Places {
        let mut occupied: Option<Span> = None;
        let mut last = Places::Nothing;
        let mut dead = false;

        for e in body {
            let p = self.expr(e, env);
            if dead {
                // 脱出の後は到達しない。数えない
                continue;
            }
            match p {
                Places::Never => dead = true,
                _ if p.occupies() => {
                    if let Some(prev) = occupied {
                        let _ = prev;
                        self.err("一つの領域に値が二つある", e.span);
                    }
                    occupied = Some(e.span);
                    last = p;
                }
                _ => {}
            }
        }

        if kind == RegionKind::LoopBody {
            if let Some(sp) = occupied {
                self.err("ループの本体に値が残っている", sp);
            }
            return Places::Nothing;
        }
        if dead && occupied.is_none() {
            return Places::Never;
        }
        let _ = span;
        match occupied {
            Some(_) => last,
            // 内面に何も残っていない → 外界面は paradox
            None => Places::Paradox,
        }
    }

    // ---- 式 ----

    fn expr(&mut self, e: &Expr, env: Env) -> Places {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str(_) => Places::Value,

            ExprKind::Name(n) => {
                if self.lookup(n).is_none() {
                    self.err(format!("知らない名前 `{n}`"), e.span);
                }
                Places::Value
            }

            // `( )` は領域を作るが、スコープでも脱出段でもない
            ExprKind::Paren(body) => {
                self.no_decls(body, "`( )` はスコープを作らないので宣言を置けない");
                self.region(body, env, e.span, RegionKind::Normal)
            }

            // 裸のブロックは領域・スコープ・脱出段の三つ
            ExprKind::Block(body) => {
                self.scopes.push(HashMap::new());
                self.collect(body);
                self.stages.push(Stage { is_loop: false, is_frame: false });
                let p = self.region(body, env, e.span, RegionKind::Normal);
                self.stages.pop();
                self.scopes.pop();
                // 段を一つ越えるので、脱出は外へ出ない可能性がある
                if p == Places::Never {
                    Places::Paradox
                } else {
                    p
                }
            }

            ExprKind::Discard(inner) => {
                if let Some(x) = inner {
                    if self.expr(x, env) == Places::Never {
                        return Places::Never;
                    }
                }
                Places::Nothing
            }

            ExprKind::Unary { rhs, .. } => self.operand(rhs, env),
            ExprKind::Binary { op, lhs, rhs } => {
                // **比較と代入の連鎖は禁じる。混在も禁じる**（C-86）
                if is_cmp(*op) {
                    if let ExprKind::Binary { op: inner, .. } = &lhs.kind {
                        if is_cmp(*inner) {
                            self.err("比較は連鎖できない", e.span);
                        }
                    }
                }
                let a = self.operand(lhs, env);
                let b = self.operand(rhs, env);
                if a == Places::Never || b == Places::Never {
                    Places::Never
                } else {
                    Places::Value
                }
            }

            ExprKind::Field { base, .. } => self.operand(base, env),
            ExprKind::Index { base, index } => {
                let a = self.operand(base, env);
                let b = self.operand(index, env);
                if a == Places::Never || b == Places::Never {
                    Places::Never
                } else {
                    // 配列の範囲外・写像の欠損キーは paradox になりうるが、静的には値
                    Places::Value
                }
            }

            ExprKind::Call { callee, args } => self.call(callee, args, env, e.span),

            ExprKind::ArrayLit(items) => {
                for i in items {
                    self.operand(i, env);
                }
                Places::Value
            }
            ExprKind::MapLit(pairs) => {
                for (k, v) in pairs {
                    self.operand(k, env);
                    self.operand(v, env);
                }
                Places::Value
            }
            ExprKind::Construct { ty, args } => self.construct(ty, args, env, e.span),

            ExprKind::Decl(d) => self.decl(d, env, e.span),
            ExprKind::Assign { op, lhs, rhs } => self.assign(*op, lhs, rhs, env, e.span),

            ExprKind::FnDecl(f) => {
                self.fn_decl(f);
                Places::Paradox
            }
            ExprKind::StructDecl(_) | ExprKind::FlowDecl(_) | ExprKind::WrapDecl(_) => {
                Places::Paradox
            }

            ExprKind::If(i) => {
                let mut any_value = false;
                let mut all_never = true;
                for (c, b) in &i.arms {
                    self.operand(c, env);
                    self.no_decl_expr(b, "`if` の分岐はスコープを作らないので宣言を置けない");
                    let p = self.expr(b, env);
                    if p != Places::Never {
                        all_never = false;
                    }
                    if p == Places::Value {
                        any_value = true;
                    }
                }
                match &i.els {
                    Some(b) => {
                        self.no_decl_expr(b, "`else` の分岐はスコープを作らないので宣言を置けない");
                        let p = self.expr(b, env);
                        if p != Places::Never {
                            all_never = false;
                        }
                        if p == Places::Value {
                            any_value = true;
                        }
                    }
                    // `else` の無い `if` で条件が偽 → paradox
                    None => all_never = false,
                }
                if all_never {
                    Places::Never
                } else if any_value {
                    Places::Value
                } else {
                    Places::Paradox
                }
            }

            ExprKind::Loop(body) => {
                self.loop_body(body, env);
                Places::Value
            }
            ExprKind::While { cond, body } => {
                self.operand(cond, env);
                self.loop_body(body, env);
                Places::Value
            }
            ExprKind::NFor { name, start, count, body } => {
                self.operand(start, env);
                self.operand(count, env);
                // ループ変数は**本体の先頭で見えている**。本体の局所とは別のスコープに置く
                self.scopes.push(HashMap::new());
                self.declare(name, BindKind::Let, false);
                self.loop_body(body, env);
                self.scopes.pop();
                Places::Value
            }

            ExprKind::Switch { subject, arms } => {
                self.operand(subject, env);
                let mut any_value = false;
                for a in arms {
                    self.operand(&a.pattern, env);
                    self.no_decl_expr(
                        &a.value,
                        "`switch` の腕はスコープを作らないので宣言を置けない",
                    );
                    if self.expr(&a.value, env) == Places::Value {
                        any_value = true;
                    }
                }
                if any_value {
                    Places::Value
                } else {
                    Places::Paradox
                }
            }

            ExprKind::Escape(esc) => {
                self.escape(esc, env);
                Places::Never
            }
        }
    }

    /// 被演算子位置。**領域だがスコープではない**ので宣言を置けない。
    fn operand(&mut self, e: &Expr, env: Env) -> Places {
        self.no_decl_expr(e, "被演算子位置はスコープを作らないので宣言を置けない");
        self.expr(e, env)
    }

    fn no_decls(&mut self, body: &[Expr], msg: &str) {
        for e in body {
            self.no_decl_expr(e, msg);
        }
    }

    /// **宣言はスコープを作る領域にしか置けない**（C-86）。
    fn no_decl_expr(&mut self, e: &Expr, msg: &str) {
        let mut e = e;
        while let ExprKind::Discard(Some(i)) = &e.kind {
            e = i;
        }
        if matches!(
            e.kind,
            ExprKind::Decl(_)
                | ExprKind::FnDecl(_)
                | ExprKind::StructDecl(_)
                | ExprKind::FlowDecl(_)
                | ExprKind::WrapDecl(_)
        ) {
            self.err(msg.to_string(), e.span);
        }
    }

    fn loop_body(&mut self, body: &Expr, env: Env) {
        self.scopes.push(HashMap::new());
        self.loop_body_inner(body, env);
        self.scopes.pop();
    }

    fn loop_body_inner(&mut self, body: &Expr, env: Env) {
        let ExprKind::Block(items) = &body.kind else {
            self.err("ループの本体はブロックでなければならない", body.span);
            return;
        };
        self.collect(items);
        // 本体は**その構文の段**。二重にはならない（C-64）
        self.stages.push(Stage { is_loop: true, is_frame: false });
        self.region(items, env, body.span, RegionKind::LoopBody);
        self.stages.pop();
    }

    // ---- 宣言・代入 ----

    fn decl(&mut self, d: &Decl, env: Env, span: Span) -> Places {
        for b in &d.bindings {
            match &b.init {
                BindInit::Value(e) => {
                    // `alias` の束縛は `&=` に限る（C-58）
                    if b.is_alias {
                        self.err("`alias` の束縛は `&=` に限る", b.span);
                    }
                    self.operand(e, env);
                }
                BindInit::AliasOf(t) => {
                    if !b.is_alias && b.ty.is_some() {
                        self.err("`&=` で束縛するなら型に `alias` が要る", b.span);
                    }
                    match self.lookup(t) {
                        None => self.err(format!("知らない名前 `{t}`"), b.span),
                        Some((k, _)) => {
                            if !can_narrow(k, d.kind) {
                                self.err("経路の権限は増やせない", b.span);
                            }
                        }
                    }
                }
            }
            self.declare(&b.name, d.kind, matches!(b.init, BindInit::AliasOf(_)));
        }
        let _ = span;
        // 宣言は値を置かない。外界面は paradox
        Places::Paradox
    }

    fn assign(&mut self, op: AssignOp, lhs: &Expr, rhs: &Expr, env: Env, span: Span) -> Places {
        if op == AssignOp::Alias {
            let (ExprKind::Name(n), ExprKind::Name(t)) = (&lhs.kind, &rhs.kind) else {
                self.err("`&=` は名前どうしでなければならない", span);
                return Places::Paradox;
            };
            match self.lookup(n) {
                None => self.err(format!("知らない名前 `{n}`"), lhs.span),
                Some((k, is_alias)) => {
                    if !is_alias {
                        self.err(format!("`{n}` は別名ではないので指し直せない"), span);
                    } else if k != BindKind::Var {
                        self.err("指し直せるのは `var` の別名だけ", span);
                    }
                }
            }
            if self.lookup(t).is_none() {
                self.err(format!("知らない名前 `{t}`"), rhs.span);
            }
            return Places::Paradox;
        }
        // 代入の連鎖は禁じる
        if matches!(rhs.kind, ExprKind::Assign { .. }) {
            self.err("代入は連鎖できない", span);
        }
        // 左辺は経路でなければならない
        match root_of(lhs) {
            None => self.err("代入の左辺は経路でなければならない", lhs.span),
            Some(n) => match self.lookup(n) {
                None => self.err(format!("知らない名前 `{n}`"), lhs.span),
                Some((k, _)) => {
                    if k != BindKind::Var {
                        self.err(format!("`{n}` は書けない（`{k:?}` で束縛されている）"), lhs.span);
                    }
                }
            },
        }
        self.expr(lhs, env);
        self.operand(rhs, env);
        Places::Paradox
    }

    fn fn_decl(&mut self, f: &FnDecl) {
        // **関数は局所変数を見ない**（C-86）。フレームで名前の鎖を切る
        let saved = self.frame_base;
        self.scopes.push(HashMap::new());
        self.frame_base = self.scopes.len() - 1;
        for p in &f.params {
            self.declare(&p.name, p.kind, p.ty.is_alias);
        }
        let ExprKind::Block(items) = &f.body.kind else {
            self.err("関数の本体はブロックでなければならない", f.span);
            self.frame_base = saved;
            self.scopes.pop();
            return;
        };
        self.collect(items);
        // 本体はフレーム。**段数はここから数え直す**
        self.stages.push(Stage { is_loop: false, is_frame: true });
        let p = self.region(items, Env, f.body.span, RegionKind::Normal);
        self.stages.pop();
        // `->` は**外界面の型**（C-66）。無ければ外界面は paradox のみ
        if f.ret.is_none() && p == Places::Value {
            self.err(
                "`->` の無い関数は paradox しか置けない（値を置くなら `-> 型` を書く）",
                f.span,
            );
        }
        self.frame_base = saved;
        self.scopes.pop();
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], env: Env, span: Span) -> Places {
        // メンバ関数。レシーバは経路でよい（C-64）
        if let ExprKind::Field { base, name } = &callee.kind {
            for a in args {
                self.operand(a, env);
            }
            // **破壊的メンバ関数はレシーバに `var` を要求する**（C-64）。
            // 利用者定義（S-1）なら `var self` かで決まる
            let destructive = matches!(name.as_str(), "push" | "pop" | "clear" | "insert" | "remove")
                || self
                    .fns
                    .values()
                    .any(|f| f.owner.is_some() && &f.name == name
                        && f.params.first().map(|p| p.kind == BindKind::Var).unwrap_or(false));
            if destructive {
                match root_of(base) {
                    Some(n) => match self.lookup(n) {
                        Some((BindKind::Var, _)) => {}
                        Some(_) => self.err(
                            format!("破壊的メンバ関数はレシーバに `var` を要求する（`{n}`）"),
                            span,
                        ),
                        None => self.err(format!("知らない名前 `{n}`"), base.span),
                    },
                    None => self.err("レシーバは経路でなければならない", base.span),
                }
            } else {
                self.operand(base, env);
            }
            return Places::Value;
        }
        let ExprKind::Name(name) = &callee.kind else {
            self.err("呼び出せるのは名前だけ（第一級関数は無い）", callee.span);
            return Places::Value;
        };
        if name == "getdepth" {
            return Places::Value;
        }
        let Some(f) = self.fns.get(name).cloned() else {
            self.err(format!("知らない関数 `{name}`"), callee.span);
            for a in args {
                self.operand(a, env);
            }
            return Places::Value;
        };
        if f.params.len() != args.len() {
            self.err(
                format!("`{name}` は引数を {} 個取るが {} 個来た", f.params.len(), args.len()),
                span,
            );
        }
        // **`alias` を受ける引数に渡せるのは名前だけ。同じセルに二つ届かない**（C-87）
        let mut alias_roots: Vec<&str> = Vec::new();
        for (p, a) in f.params.iter().zip(args) {
            if p.ty.is_alias {
                let ExprKind::Name(n) = &a.kind else {
                    self.err("`alias` 引数に渡せるのは名前だけ", a.span);
                    continue;
                };
                match self.lookup(n) {
                    None => self.err(format!("知らない名前 `{n}`"), a.span),
                    Some((k, _)) => {
                        if !can_narrow(k, p.kind) {
                            self.err("経路の権限は増やせない", a.span);
                        }
                    }
                }
                if alias_roots.contains(&n.as_str()) {
                    self.err("同じセルに別名が二つ届く", a.span);
                }
                alias_roots.push(n);
            } else {
                self.operand(a, env);
            }
        }
        Places::Value
    }

    fn construct(&mut self, ty: &Type, args: &CtorArgs, env: Env, span: Span) -> Places {
        if let ValueType::Named(n) = &ty.value {
            // 構造体かラップ型（S-2）
            if !self.structs.contains_key(n) && !self.wraps.contains_key(n) {
                self.err(format!("知らない型 `{n}`"), span);
            }
        }
        match args {
            CtorArgs::Named(v) => {
                for (_, e) in v {
                    self.operand(e, env);
                }
            }
            CtorArgs::Positional(v) => {
                for e in v {
                    self.operand(e, env);
                }
            }
        }
        Places::Value
    }

    // ---- 脱出 ----

    /// フレームまでの段数。`getdepth()` と同じ数え方（C-23）。
    fn depth(&self) -> u32 {
        let mut n = 0;
        for s in self.stages.iter().rev() {
            n += 1;
            if s.is_frame {
                break;
            }
        }
        n
    }

    /// 段数を静的に数える。**定数個並んでいる場合だけ**（C-88）。
    ///
    /// `outward` は**書いた `break` の段送りに掛かる**（C-70）ので、
    /// 「何回目が越えるか」を位置で見る。**空振りの `outward` は静的エラー。**
    fn escape(&mut self, esc: &Escape, env: Env) {
        let (stages, kind, mask) = self.escape_shape(esc, env);
        let Some(stages) = stages else { return }; // 動的なら実行時に検査する
        if stages == 0 {
            return;
        }
        // 内側から順に段を送る
        let mut here = self.stages.len();
        for k in 0..stages {
            if here == 0 {
                self.err("段が足りない", esc.span);
                return;
            }
            let st = self.stages[here - 1];
            let is_last = k + 1 == stages;
            let crosses = (mask >> k) & 1 != 0;

            if crosses && !st.is_frame {
                self.err("`outward` がフレームを越えていない（空振り）", esc.span);
                return;
            }
            if !is_last {
                // 通り抜ける段。フレームなら `outward` が要る
                if st.is_frame && !crosses {
                    self.err("フレームを越える脱出", esc.span);
                    return;
                }
                // フレームを越えた先の段数は**呼び出し位置による**ので、ここで打ち切る
                if crosses {
                    return;
                }
            } else {
                // 行き先。`continue` はループでなければ再開できるものが無い（C-72）
                if kind == Some(EKind2::Continue) && !st.is_loop {
                    self.err("再開できるものが無い（抜けた先がループではない）", esc.span);
                }
            }
            here -= 1;
        }
    }

    /// 脱出の形。段数が静的に決まらなければ `None`。
    /// 第三の値は**どの段送りがフレームを越えるか**のビット列（ビット 0 が最初）。
    fn escape_shape(&mut self, esc: &Escape, env: Env) -> (Option<u32>, Option<EKind2>, u64) {
        match &esc.kind {
            EscapeKind::Break { outward } => {
                let base = if *outward { 1u64 } else { 0 };
                match &esc.operand {
                    // `break X` は**段数を足す**（C-92）。内側の印は一つ後ろへずれる
                    Some(Operand::Escape(inner)) => {
                        let (s, k, m) = self.escape_shape(inner, env);
                        (s.map(|x| x + 1), k, (m << 1) | base)
                    }
                    Some(Operand::Value(v)) => {
                        self.operand(v, env);
                        (Some(1), Some(EKind2::Break), base)
                    }
                    None => (Some(1), Some(EKind2::Break), base),
                }
            }
            // `continue X` は**足さない**。X は再開した本体の先頭で走る（C-92）。
            // したがって**本体の先頭で見えている名前しか見えない**（C-81）
            EscapeKind::Continue => {
                if let Some(Operand::Escape(inner)) = &esc.operand {
                    let saved = self.visible_limit;
                    // 本体のスコープを外す。ループ変数と外側の名前だけが見える
                    self.visible_limit = Some(self.scopes.len().saturating_sub(1));
                    self.escape(inner, env);
                    self.visible_limit = saved;
                }
                (Some(1), Some(EKind2::Continue), 0)
            }
            EscapeKind::Flow { name, args } => {
                if !self.flows.contains(name) {
                    self.err(format!("知らない作用素式 `{name}`"), esc.span);
                }
                for a in args {
                    match a {
                        FlowArg::Value(v) => {
                            self.operand(v, env);
                        }
                        FlowArg::Escape(_) => {}
                    }
                }
                if let Some(Operand::Value(v)) = &esc.operand {
                    self.operand(v, env);
                }
                // `$return` は `getdepth()` が字句的に決まるので静的
                if name == "$return" {
                    return (Some(self.depth()), Some(EKind2::Break), 0);
                }
                (None, None, 0)
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum EKind2 {
    Break,
    Continue,
}

fn is_cmp(op: BinOp) -> bool {
    matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne)
}

fn can_narrow(from: BindKind, to: BindKind) -> bool {
    match (from, to) {
        (BindKind::Var, _) => true,
        (BindKind::Let, BindKind::Var) => false,
        (BindKind::Let, _) => true,
        (BindKind::Const, BindKind::Const) => true,
        (BindKind::Const, _) => false,
    }
}

/// 経路の根（最初の識別子）。**別名の同一性は根で判定する**（C-87）。
fn root_of(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Name(n) => Some(n),
        ExprKind::Field { base, .. } | ExprKind::Index { base, .. } => root_of(base),
        _ => None,
    }
}

fn type_deps(t: &ValueType) -> Vec<String> {
    match t {
        ValueType::Named(n) => vec![n.clone()],
        ValueType::Array(i) => type_deps(i),
        ValueType::Map(k, v) => {
            let mut o = type_deps(k);
            o.extend(type_deps(v));
            o
        }
        _ => Vec::new(),
    }
}
