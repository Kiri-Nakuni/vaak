//! カーソルマシン。
//!
//! プログラム全体の AST は構築しない。トークン列の上をカーソルが進み、
//! 読んだそばから実行する（設計書 §1.2 / §2）。`goto` の走査は文を実行せず、
//! 波括弧の深さのみを認識してカーソルを動かす（§4.1-2）。

use std::rc::Rc;

use crate::lexer::{Lexed, Tok};
use crate::state::{
    thunk_clone_computation, thunk_expr, thunk_value, Binding, Cell, FuncDef, FlowDef, State,
};

#[derive(Debug, PartialEq)]
pub enum Flow {
    Normal,
    /// D-34 / D-38: `break` は「このブロックを閉じて、外側の文脈で X を行う」。
    /// u32 は残りの段数、`After` が外側で起きること。
    Break(u32, After),
    /// D-29: ループの次の周回へ。スコープを動かさないので巻き戻しは起きない。
    Continue(u32),
    /// カーソルは移動済み。着地点を含むフレームまで Rust スタックを巻き戻す。
    Jump,
}

/// D-38: `break` の段数を抜けきったあと、外側の文脈で起きること。
/// 作用素式の終端に対応する。
#[derive(Debug, PartialEq, Clone)]
pub enum After {
    /// 値。§4.8 により break の時点で強制済み。ブロックの値になる。
    Value(Option<i64>),
    /// 反復を n 個進める。`continue` は「この周回を終えて次へ」なので、
    /// 重ねれば n 個進む——`ifor` なら n 個消費、`nfor` なら n 個飛ばす、
    /// `while` なら条件を n 回試す。`loop` は進める状態がないので退化する。
    Continue(u32),
    /// ブロックを閉じた位置から前方走査して跳ぶ。jump ではないので巻き戻さない。
    /// bool は `#[flat]`（走査から構造の認識を外す）。
    Goto(String, bool),
    /// 外側の文脈に戻り先を登録する。**唯一、実行が続く終端**。
    Comefrom(String),
    /// 外側の文脈で label を実行する。
    Label(String),
}

/// 名前参照ではない識別子（D-30 の存在チェックで除外する）。
const RESERVED: &[&str] = &[
    "let", "fn", "typ", "if", "else", "loop", "while", "nfor", "ifor", "comp", "switch", "break",
    "continue", "goto", "label", "comefrom", "in", "mod", "print", "read", "read_lines",
    "getdepth", "repeat", "flow", "frame", "flat", "lazy_attribute",
];

type R<T> = Result<T, String>;

const MAX_CALL_DEPTH: u32 = 900;

pub struct Interp {
    toks: Vec<Tok>,
    lines: Vec<u32>,
    /// 論理行。改行の意味（アトリビュートと返り値型の終端）に使う。
    /// コメントが飲んだ改行では進まない（D-66）。
    logical: Vec<u32>,
    eof: usize,
    pos: usize,
    st: State,
    /// §4.2: comefrom トリガーが禁止されている文脈の深さ。
    no_comefrom: u32,
    depth: u32,
    /// D-39: 作用素の別名の展開深さ。自己参照の検出用。
    flow_depth: u32,
    /// 無名標準ライブラリの行数。エラー位置をユーザーコード基準に直す。
    line_offset: u32,
    /// 現在のフレームのトークン範囲。`goto` の走査はここで打ち切る——
    /// depth 基準を跨いだ label は**見えない**（エラーではなく隠蔽）。
    frame_span: (usize, usize),
    /// 式の位置に現れたブロック（`loop` / `{ }` / `if` 式）から外へ出ようとしている
    /// 制御。式評価は値しか返せないので、ここに預けて文の位置で回収する。
    pending: Option<Flow>,
    input: Vec<i64>,
    input_at: usize,
    pub out: Vec<String>,
}

impl Interp {
    pub fn new(lx: Lexed, input: Vec<i64>) -> Self {
        let eof = lx.toks.len() - 1;
        Interp {
            toks: lx.toks,
            lines: lx.lines,
            logical: lx.logical,
            eof,
            pos: 0,
            st: State::new(),
            no_comefrom: 0,
            depth: 0,
            flow_depth: 0,
            line_offset: 0,
            frame_span: (0, eof),
            pending: None,
            input,
            input_at: 0,
            out: Vec::new(),
        }
    }

    pub fn set_line_offset(&mut self, n: u32) {
        self.line_offset = n;
    }

    /// プログラムを走らせ、ホストへ渡す正常終了の値を返す（3.6節）。
    /// トップレベルもフレームなので、`break` の段数超過はここでも検査される。
    pub fn run(&mut self) -> R<i64> {
        match self.exec_seq(0, self.eof)? {
            Flow::Normal => Ok(0),
            Flow::Jump => Err(self.err("jump の着地点がどのフレームにも含まれていません")),
            Flow::Continue(_) => {
                Err(self.err("ここは繰り返しの中ではないので continue できません"))
            }
            Flow::Break(n, after) => {
                if n > 1 {
                    return Err(self.err(&format!(
                        "break の段数が足りません——プログラムの外へはあと {} 段抜けられません（getdepth() を確認してください）",
                        n - 1
                    )));
                }
                match after {
                    After::Value(v) => Ok(v.unwrap_or(0)),
                    After::Continue(_) => {
                        Err(self.err("プログラムの外へ continue を持ち出すことはできません"))
                    }
                    After::Goto(_, _) | After::Label(_) | After::Comefrom(_) => {
                        Err(self.err("プログラムの外へ制御を持ち出すことはできません"))
                    }
                }
            }
        }
    }

    // ---- トークン操作 ----

    fn tok(&self) -> &Tok {
        &self.toks[self.pos.min(self.eof)]
    }

    fn at(&self, i: usize) -> &Tok {
        &self.toks[i.min(self.eof)]
    }

    fn err(&self, m: &str) -> String {
        self.at_line(self.lines[self.pos.min(self.eof)], m)
    }

    /// 前置きの分を差し引く。無名標準ライブラリ内で起きたエラーはそう明示する。
    fn at_line(&self, line: u32, m: &str) -> String {
        if line < self.line_offset {
            format!("prelude:{}行: {}", line, m)
        } else {
            format!("{}行: {}", line - self.line_offset + 1, m)
        }
    }

    fn eat(&mut self, t: Tok, what: &str) -> R<()> {
        if *self.tok() == t {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.err(&format!("{} が必要です（{:?} が来ました）", what, self.tok())))
        }
    }

    fn ident(&mut self) -> R<String> {
        match self.tok().clone() {
            Tok::Ident(s) => {
                self.pos += 1;
                Ok(s)
            }
            t => Err(self.err(&format!("識別子が必要です（{:?} が来ました）", t))),
        }
    }

    /// 型は後置形式（`i64 array` / `T U pair`）なので識別子の並び。
    /// Tier 1 は型検査を持たないので読み捨てる。
    fn skip_type(&mut self) -> R<String> {
        let first = self.ident()?;
        while matches!(self.tok(), Tok::Ident(_)) {
            self.pos += 1;
        }
        Ok(first)
    }

    /// 返り値型は改行で終端する（D-21r）。同じ行にある識別子だけを型として読むので、
    /// 型名表を持たなくても次の文の先頭を吸い込まない。
    fn skip_type_to_eol(&mut self) -> R<String> {
        let line = self.logical[self.pos.min(self.eof)];
        let first = self.ident()?;
        while matches!(self.tok(), Tok::Ident(_)) && self.logical[self.pos.min(self.eof)] == line {
            self.pos += 1;
        }
        Ok(first)
    }

    fn match_pair(&self, open: usize, o: Tok, c: Tok) -> R<usize> {
        let mut d = 0i32;
        let mut i = open;
        while i < self.eof {
            if self.toks[i] == o {
                d += 1;
            } else if self.toks[i] == c {
                d -= 1;
                if d == 0 {
                    return Ok(i);
                }
            }
            i += 1;
        }
        Err(format!("{}行: 括弧が閉じていません", self.lines[open]))
    }

    fn match_brace(&self, open: usize) -> R<usize> {
        self.match_pair(open, Tok::LBrace, Tok::RBrace)
    }

    fn match_paren(&self, open: usize) -> R<usize> {
        self.match_pair(open, Tok::LParen, Tok::RParen)
    }

    // ---- 文の実行 ----

    /// `[start, end)` の範囲の文を順に実行する。この範囲が「フレーム」であり、
    /// jump の着地点がこの範囲に入っていればここで受け止め、さもなくば上へ伝播する。
    fn exec_seq(&mut self, start: usize, end: usize) -> R<Flow> {
        self.pos = start;
        loop {
            if self.pos >= end {
                return Ok(Flow::Normal);
            }
            let mut f = self.exec_stmt()?;
            if f == Flow::Normal {
                if let Some(p) = self.pending.take() {
                    f = p;
                }
            }
            match f {
                Flow::Normal => {}
                Flow::Jump => {
                    if self.pos >= start && self.pos < end {
                        continue;
                    }
                    return Ok(Flow::Jump);
                }
                b => return Ok(b),
            }
        }
    }

    fn exec_stmt(&mut self) -> R<Flow> {
        // アトリビュート。D-18r: `#[global, immediate]` のようにリストで書け、
        // 行末で終端し、**次の行**の評価方法を変更する。
        //
        // D-19 は撤回済み：global（書き込みレベル）と immediate（評価時点）は
        // 直交する2つの操作であり、束ねない。
        let mut immediate = false;
        let mut is_global = false;
        let mut forward = false;
        // D-67: `goto` の走査から波括弧の認識を外す。
        let mut flat = false;
        // D-68: 次のアトリビュートを遅延させる。宣言に保存され、使用位置で効く。
        let mut lazy_next = false;
        let mut lazy_flat = false;
        while *self.tok() == Tok::Hash {
            self.pos += 1;
            self.eat(Tok::LBracket, "`[`")?;
            let mut attr_was_lazy_marker = false;
            loop {
                let a = self.ident()?;
                if a == "lazy_attribute" {
                    attr_was_lazy_marker = true;
                }
                match a.as_str() {
                    "immediate" => immediate = true,
                    "global" => is_global = true,
                    // D-6 / D-30: 名前の存在チェックを抑止する。
                    "forward" => forward = true,
                    // D-67: 走査がブロック構造を見なくなる。
                    "flat" => {
                        if lazy_next {
                            lazy_flat = true;
                        } else {
                            flat = true;
                        }
                    }
                    // D-68: 次のアトリビュートを遅延させる。
                    "lazy_attribute" => {}
                    _ => return Err(self.err(&format!("未知のアトリビュート: {}", a))),
                }
                if *self.tok() == Tok::Comma {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            self.eat(Tok::RBracket, "`]`")?;
            // 次のアトリビュートを遅延させる指定は、この行のうちに記録する。
            lazy_next = attr_was_lazy_marker;
            if self.pos < self.eof && self.logical[self.pos] == self.logical[self.pos - 1] {
                return Err(self.err(
                    "アトリビュートは行末で終端します——修飾する文は次の行に置いてください（行末のコメントは改行を飲むので、ここには置けません）",
                ));
            }
        }

        // 何も抑止しない属性は嘘になる。`#[flat]` は `goto` 専用。
        if flat && !self.tok().is_kw("goto") {
            return Err(self.err("`#[flat]` は `goto` にのみ付けられます（走査の規則を変える属性です）"));
        }

        if *self.tok() == Tok::Semi {
            self.pos += 1;
            return Ok(Flow::Normal);
        }

        if *self.tok() == Tok::LBrace {
            // 裸のブロックはスコープを作る（D-1）。
            return self.exec_block(true);
        }

        // D-38: `$` で始まる文は作用素式。
        if *self.tok() == Tok::Dollar {
            return self.exec_operator_chain();
        }

        let kw = match self.tok() {
            Tok::Ident(s) => s.clone(),
            t => return Err(self.err(&format!("文の先頭に置けません: {:?}", t))),
        };

        match kw.as_str() {
            "let" => self.exec_let(immediate, is_global, forward),

            // D-27: 関数定義は `fn`。`let`（値の束縛）とは束縛種も終端規則も
            // 第一級性も違う別の操作なので、同じキーワードに載せない。
            "fn" => {
                if forward {
                    // 関数本体は検査しないので、抑止するものがない（D-30）。
                    return Err(self.err(
                        "`#[forward]` は `fn` には付けられません——関数本体は名前検査の対象外です（D-30）",
                    ));
                }
                self.pos += 1;
                let name = self.ident()?;
                if *self.tok() != Tok::LParen {
                    return Err(self.err("`fn` の後には引数リストが必要です"));
                }
                let def = self.parse_func(&name)?;
                let b = Binding::Func(Rc::new(def));
                if is_global {
                    self.st.set_global(&name, b);
                } else {
                    self.st.set(&name, b);
                }
                Ok(Flow::Normal)
            }

            "if" => self.exec_if(),

            "loop" => {
                let (f, _) = self.run_loop()?;
                Ok(f)
            }

            "break" => self.exec_operator_chain(),

            // D-39: 作用素式に別名を与える。`let`（値）・`fn`（関数）と並ぶ
            // 第3の束縛種であり、本体は使用のたびに使用位置で読み直される。
            "flow" => {
                self.pos += 1;
                let name = self.ident()?;
                // D-42: `:=` ではなく `=`。束縛演算子は thunk の作り方を区別する
                // ためのもので、`flow` は thunk を作らない。
                self.eat(Tok::Define, "`=`")?;
                let start = self.pos;
                let mut d = 0i32;
                while self.pos < self.eof {
                    match self.tok() {
                        Tok::LParen => d += 1,
                        Tok::RParen => d -= 1,
                        Tok::Semi if d <= 0 => break,
                        _ => {}
                    }
                    self.pos += 1;
                }
                let end = self.pos;
                self.eat(Tok::Semi, "`;`")?;
                let b = Binding::Flow(Rc::new(FlowDef {
                    name: name.clone(),
                    body: (start, end),
                    flat: lazy_flat,
                }));
                if is_global {
                    self.st.set_global(&name, b);
                } else {
                    self.st.set(&name, b);
                }
                Ok(Flow::Normal)
            }


            // D-29: continue はループの次の周回へ移る。jump ではあるが
            // ブロックを出ないので、非 global 状態の巻き戻しは起きない。
            "continue" => {
                self.pos += 1;
                self.eat(Tok::Semi, "`;`")?;
                Ok(Flow::Continue(1))
            }

            "goto" => {
                self.pos += 1;
                let name = self.ident()?;
                self.eat(Tok::Semi, "`;`")?;
                match self.scan_to_label_with(&name, flat) {
                    Some(p) => {
                        self.st.jump_unwind();
                        self.pos = p;
                        Ok(Flow::Jump)
                    }
                    None => Err(self.err(&format!("label {} が前方に見つかりません", name))),
                }
            }

            "label" => {
                self.pos += 1;
                let name = self.ident()?;
                self.eat(Tok::Semi, "`;`")?;
                // §4.1-3: 保存されている comefrom があればその直後へ飛び、さもなくば何もしない。
                match self.st.comefrom_target(&name) {
                    Some(target) => {
                        self.st.jump_unwind();
                        self.pos = target;
                        Ok(Flow::Jump)
                    }
                    None => Ok(Flow::Normal),
                }
            }

            "comefrom" => {
                self.pos += 1;
                let name = self.ident()?;
                self.eat(Tok::Semi, "`;`")?;
                if self.no_comefrom > 0 {
                    return Err(self.err(
                        "この文脈では comefrom トリガーを置けません（§4.2：let の右辺・ループ本体・関数本体）",
                    ));
                }
                // §4.1-1: 戻り先はこの文の直後。通過するたびに無条件で上書き。
                let here = self.pos;
                self.st.register_comefrom(&name, here);
                Ok(Flow::Normal)
            }

            "print" => {
                self.pos += 1;
                self.eat(Tok::LParen, "`(`")?;
                let (s, e) = self.scan_expr()?;
                let v = self.eval_range(s, e)?;
                self.eat(Tok::RParen, "`)`")?;
                self.eat(Tok::Semi, "`;`")?;
                self.out.push(v.to_string());
                println!("{}", v);
                Ok(Flow::Normal)
            }

            _ => {
                // D-39: 名前が作用素の別名なら作用素式として読む
                if matches!(self.st.lookup(&kw), Some(Binding::Flow(_))) {
                    return self.exec_operator_chain();
                }
                // 式文。`;` が値を潰す（4.9節）。関数呼び出しを文として書く経路。
                if *self.at(self.pos + 1) == Tok::LParen {
                    let (s, e) = self.scan_expr()?;
                    self.eval_range(s, e)?;
                    self.eat(Tok::Semi, "`;`")?;
                    return Ok(Flow::Normal);
                }
                // 再束縛または複合代入
                let name = self.ident()?;
                self.exec_assign(&name, is_global, immediate)
            }
        }
    }

    /// 作用素式（D-38）。
    ///
    /// ```text
    /// 作用素式 := 作用素* 終端
    /// 作用素   := 'break' | '$break' | '$repeat(' 作用素 ',' 式 ')'
    /// 終端     := 式 | 'continue' | 'goto' 識別子 | ε
    /// ```
    ///
    /// 作用素は値ではなく、束縛に入れられない（D-27 と同じ線）。段数は式から
    /// 計算してよいので `$repeat($break, getdepth()) v;` が return になる。
    /// 並置が順序づけを担うので、別に fusion のような構成子は要らない——
    /// `$repeat($break, n) continue` がそれを書ける。
    fn exec_operator_chain(&mut self) -> R<Flow> {
        let (levels, term) = self.parse_op_expr(self.eof)?;

        let after = match term {
            // 別名が終端を持っていた場合、使用位置に重ねることはできない。
            Some(t) => {
                if *self.tok() != Tok::Semi {
                    return Err(self.err(
                        "この作用素式は既に終端を持っています（値や continue を重ねられません）",
                    ));
                }
                self.pos += 1;
                t
            }
            None if *self.tok() == Tok::Semi => {
                self.pos += 1;
                After::Value(None)
            }
            None => {
                let (s, e) = self.scan_expr()?;
                // §4.8: break の引数は自動的に強制評価される。
                let v = self.eval_range(s, e)?;
                self.eat(Tok::Semi, "`;`")?;
                After::Value(Some(v))
            }
        };

        if levels == 0 {
            // ブロックを抜けない作用素式。終端をその場で行う。
            return self.resolve_after(after);
        }
        Ok(Flow::Break(levels, after))
    }

    /// 作用素式を読む。`[pos, end)` の範囲で止まり、段数と終端の対を返す。
    ///
    /// D-39: 名前が作用素の別名なら、その本体を**この位置で**読み直す。
    /// フレームを積まないので、本体中の `getdepth()` は使用位置の深さを見る。
    /// 別名の本体は終端を含んでよい——インラインで書けるものは名前を付けられる。
    /// その場合、使用位置に終端を重ねることはできない。
    fn parse_op_expr(&mut self, end: usize) -> R<(u32, Option<After>)> {
        let mut levels: u32 = 0;
        let mut term: Option<After> = None;
        while self.pos < end && term.is_none() {
            if self.tok().is_kw("break") {
                self.pos += 1;
                levels = levels.saturating_add(1);
                continue;
            }
            // D-38: `$` を読んだ時点で作用素の評価規則へ入る。入ったあとは
            // 印を重ねない——`$repeat(break, 2)` であって `$repeat($break, 2)` ではない。
            if *self.tok() == Tok::Dollar && self.at(self.pos + 1).is_kw("repeat") {
                let (lv, t) = self.parse_repeat()?;
                levels = levels.saturating_add(lv);
                term = t;
                continue;
            }
            if self.tok().is_kw("continue") {
                self.pos += 1;
                term = Some(After::Continue(1));
                continue;
            }
            if self.tok().is_kw("goto") {
                self.pos += 1;
                term = Some(After::Goto(self.ident()?, false));
                continue;
            }
            if self.tok().is_kw("comefrom") {
                self.pos += 1;
                term = Some(After::Comefrom(self.ident()?));
                continue;
            }
            if self.tok().is_kw("label") {
                self.pos += 1;
                term = Some(After::Label(self.ident()?));
                continue;
            }
            if let Tok::Ident(n) = self.tok().clone() {
                if let Some(Binding::Flow(def)) = self.st.lookup(&n) {
                    self.pos += 1;
                    if self.flow_depth >= 64 {
                        return Err(self.err("作用素の別名が再帰しています"));
                    }
                    self.flow_depth += 1;
                    let save = self.pos;
                    self.pos = def.body.0;
                    let r = self.parse_op_expr(def.body.1);
                    let leftover = self.pos < def.body.1;
                    self.pos = save;
                    self.flow_depth -= 1;
                    let (lv, t) = r?;
                    if leftover {
                        return Err(self.err(&format!(
                            "作用素 {} の本体に作用素でないものが含まれています",
                            def.name
                        )));
                    }
                    levels = levels.saturating_add(lv);
                    // D-68: 遅延した `#[flat]` を本体の goto へ渡す。
                    term = match (t, def.flat) {
                        (Some(After::Goto(n, _)), true) => Some(After::Goto(n, true)),
                        (other, _) => other,
                    };
                    continue;
                }
            }
            break;
        }
        Ok((levels, term))
    }

    /// `$repeat(op, n)` — 段数と終端の対を返す。
    ///
    /// `break` を重ねればブロックを n 段抜け、`continue` を重ねれば反復を n 個進める。
    /// `goto` は行き先がラベルで決まるので重ねられない。
    fn parse_repeat(&mut self) -> R<(u32, Option<After>)> {
        self.pos += 2; // `$` `nest`
        self.eat(Tok::LParen, "`(`")?;
        // 既に作用素の評価規則の中なので、`break` に印は付けない。
        let (lv, term) = if *self.tok() == Tok::Dollar && self.at(self.pos + 1).is_kw("repeat") {
            self.parse_repeat()?
        } else if self.tok().is_kw("break") {
            self.pos += 1;
            (1, None)
        } else if self.tok().is_kw("continue") {
            self.pos += 1;
            (0, Some(After::Continue(1)))
        } else if matches!(self.tok(), Tok::Ident(n) if matches!(self.st.lookup(n), Some(Binding::Flow(_)))) {
            let end = self.pos + 1;
            self.parse_op_expr(end)?
        } else {
            return Err(self.err(
                "`$repeat` の第1引数は `break` / `continue` / 作用素の別名に限ります（`goto` は行き先がラベルで決まるので重ねられません）",
            ));
        };
        self.eat(Tok::Comma, "`,`")?;
        let (s, e) = self.scan_expr()?;
        let n = self.eval_range(s, e)?;
        self.eat(Tok::RParen, "`)`")?;
        if n < 0 {
            return Err(self.err(&format!("`$repeat` の段数が負です: {}", n)));
        }
        let n = n.min(u32::MAX as i64) as u32;
        match term {
            None => Ok((lv.saturating_mul(n), None)),
            Some(After::Continue(k)) if lv == 0 => Ok((0, Some(After::Continue(k.saturating_mul(n))))),
            Some(_) => Err(self.err(
                "この作用素は重ねられません（`goto` と値は行き先／結果が1つに決まるため）",
            )),
        }
    }

    fn exec_let(&mut self, immediate: bool, is_global: bool, forward: bool) -> R<Flow> {
        self.pos += 1;
        let name = self.ident()?;

        if *self.tok() == Tok::LParen {
            return Err(self.err("関数定義は `fn` で書きます（`let` は値の束縛専用、D-27）"));
        }

        let cell = self.bind_rhs_checked(forward)?;
        if immediate {
            self.force(&cell)?;
        }
        let b = Binding::Val(cell);
        if is_global {
            self.st.set_global(&name, b);
        } else {
            self.st.set(&name, b);
        }
        Ok(Flow::Normal)
    }

    /// 関数定義。本体はトークン範囲として保存するだけで、この時点では読まない。
    fn parse_func(&mut self, name: &str) -> R<FuncDef> {
        self.pos += 1; // (
        let mut params = Vec::new();
        while *self.tok() != Tok::RParen {
            let p = self.ident()?;
            if *self.tok() == Tok::Colon {
                self.pos += 1;
                self.skip_type()?;
            }
            params.push(p);
            if *self.tok() == Tok::Comma {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.eat(Tok::RParen, "`)`")?;

        if *self.tok() != Tok::LBrace {
            return Err(self.err("関数本体の `{` が必要です"));
        }
        let lb = self.pos;
        let rb = self.match_brace(lb)?;
        self.pos = rb + 1;

        // D-8: 返り値型は本体の後置 `->`。
        // 宣言は paradox 値を産むので `;` で消さねばならない（4.9節）。
        // 型の終端は改行で切るので、型名表がなくても次の行を吸い込まない。
        let ret = if *self.tok() == Tok::Arrow {
            self.pos += 1;
            Some(self.skip_type_to_eol()?)
        } else {
            None
        };
        self.eat(Tok::Semi, "`;`（宣言は paradox を産むので消さねばなりません）")?;

        Ok(FuncDef {
            name: name.to_string(),
            params,
            body: (lb, rb),
            ret,
        })
    }

    /// `let` の右辺は評価が遅延するので、そのままでは未知の名前のエラーが
    /// 「いつ強制されたか」に依存して現れる。エラーの出るタイミングを評価戦略から
    /// 独立させるため、束縛の時点で名前の存在を検査する（§3.3 の「前方参照を認めない」）。
    /// `#[forward]` はこの検査だけを抑止する（D-30）。
    ///
    /// 検査するのは `let` の右辺だけで、`fn` の本体は対象外——本体を検査すると、
    /// `goto` で読み飛ばされる区間に未定義の名前があってよい（§4.3、例4）という
    /// 中心的な性質と衝突するため。
    fn check_names(&self, start: usize, end: usize) -> R<()> {
        let mut introduced: Vec<String> = Vec::new();
        let mut i = start;
        while i < end {
            // アトリビュート `#[...]` の中身は名前参照ではない
            if self.toks[i] == Tok::Hash {
                i += 1;
                while i < end && self.toks[i] != Tok::RBracket {
                    i += 1;
                }
                i += 1;
                continue;
            }
            let name = match &self.toks[i] {
                Tok::Ident(s) => s.clone(),
                _ => {
                    i += 1;
                    continue;
                }
            };
            // `-> T` の T は型名であって値の参照ではない（D-8）
            if i > start && self.toks[i - 1] == Tok::Arrow {
                while i < end && matches!(self.toks[i], Tok::Ident(_)) {
                    i += 1;
                }
                continue;
            }
            // `goto L` / `label L` / `comefrom L` の L はラベル名であって値の参照ではない
            if i > start
                && matches!(&self.toks[i - 1],
                    Tok::Ident(k) if k == "goto" || k == "label" || k == "comefrom")
            {
                i += 1;
                continue;
            }
            // `let x := ...` / `nfor (i in ...)` はこの範囲内で名前を導入する
            if i > start && matches!(&self.toks[i - 1], Tok::Ident(k) if k == "let" || k == "in") {
                introduced.push(name);
                i += 1;
                continue;
            }
            if RESERVED.contains(&name.as_str())
                || introduced.contains(&name)
                || self.st.exists(&name)
            {
                i += 1;
                continue;
            }
            return Err(self.at_line(
                self.lines[i],
                &format!("未束縛の名前: {}（前方参照するなら `#[forward]` を付けてください）", name),
            ));
        }
        Ok(())
    }

    fn bind_rhs_checked(&mut self, forward: bool) -> R<Cell> {
        if forward || *self.tok() != Tok::Assign {
            return self.bind_rhs();
        }
        let start = self.pos + 1;
        let cell = self.bind_rhs()?;
        // `;` の手前まで
        let end = self.pos - 1;
        self.check_names(start, end)?;
        Ok(cell)
    }

    /// `:=` / `&=` / `^=` の右辺から束縛セルを作る。
    fn bind_rhs(&mut self) -> R<Cell> {
        match self.tok().clone() {
            Tok::Assign => {
                self.pos += 1;
                let (s, e) = self.scan_expr()?;
                self.eat(Tok::Semi, "`;`")?;
                Ok(thunk_expr(s, e))
            }
            // §4.6: a が現在指している thunk そのものを共有する。
            Tok::AliasBind => {
                self.pos += 1;
                let src = self.ident()?;
                self.eat(Tok::Semi, "`;`")?;
                self.value_of(&src)
            }
            // §4.6: 未評価の計算を独立した新 thunk として複製する。
            Tok::CloneBind => {
                self.pos += 1;
                let src = self.ident()?;
                self.eat(Tok::Semi, "`;`")?;
                let c = self.value_of(&src)?;
                Ok(thunk_clone_computation(&c))
            }
            t => Err(self.err(&format!("束縛演算子が必要です（{:?} が来ました）", t))),
        }
    }

    fn value_of(&self, name: &str) -> R<Cell> {
        match self.st.lookup(name) {
            Some(Binding::Val(c)) => Ok(c),
            Some(Binding::Func(_)) => Err(self.err(&format!(
                "{} は関数です。関数は第一級ではないので束縛できません",
                name
            ))),
            // D-38: 作用素は値ではない。D-27 と同じ線。
            Some(Binding::Flow(_)) => Err(self.err(&format!(
                "{} は作用素です。作用素は第一級ではないので束縛できません",
                name
            ))),
            None => Err(self.err(&format!("未束縛の名前: {}", name))),
        }
    }

    fn exec_assign(&mut self, name: &str, is_global: bool, immediate: bool) -> R<Flow> {
        let cell = match self.tok().clone() {
            Tok::Assign | Tok::AliasBind | Tok::CloneBind => self.bind_rhs()?,
            op @ (Tok::PlusEq | Tok::MinusEq | Tok::StarEq | Tok::SlashEq) => {
                self.pos += 1;
                // D-17: 複合代入は現在値を即時強制する。遅延にすると
                // `x += 1` が自己参照 thunk になって発散する。
                let cur_cell = self.value_of(name)?;
                let cur = self.force(&cur_cell)?;
                let (s, e) = self.scan_expr()?;
                let rhs = self.eval_range(s, e)?;
                self.eat(Tok::Semi, "`;`")?;
                let v = match op {
                    Tok::PlusEq => cur.wrapping_add(rhs),
                    Tok::MinusEq => cur.wrapping_sub(rhs),
                    Tok::StarEq => cur.wrapping_mul(rhs),
                    _ => {
                        if rhs == 0 {
                            return Err(self.err("0 除算"));
                        }
                        cur / rhs
                    }
                };
                thunk_value(v)
            }
            t => return Err(self.err(&format!("代入演算子が必要です（{:?} が来ました）", t))),
        };
        if immediate {
            self.force(&cell)?;
        }
        let b = Binding::Val(cell);
        if is_global {
            self.st.set_global(name, b);
        } else {
            self.st.set(name, b);
        }
        Ok(Flow::Normal)
    }

    /// D-1: `if` / `else` の本体はスコープを作らない。
    fn exec_if(&mut self) -> R<Flow> {
        self.pos += 1;
        let (s, e) = self.scan_expr()?;
        let c = self.eval_range(s, e)?;
        if c != 0 {
            let f = self.exec_branch()?;
            if f != Flow::Normal {
                return Ok(f);
            }
            if self.tok().is_kw("else") {
                self.pos += 1;
                self.skip_stmt()?;
            }
            Ok(Flow::Normal)
        } else {
            self.skip_stmt()?;
            if self.tok().is_kw("else") {
                self.pos += 1;
                return self.exec_branch();
            }
            Ok(Flow::Normal)
        }
    }

    fn exec_branch(&mut self) -> R<Flow> {
        if *self.tok() == Tok::LBrace {
            self.exec_block(false)
        } else {
            self.exec_stmt()
        }
    }

    fn exec_block(&mut self, scoped: bool) -> R<Flow> {
        let lb = self.pos;
        let rb = self.match_brace(lb)?;
        if scoped {
            self.st.push_group();
        }
        let f = self.exec_seq(lb + 1, rb)?;
        if scoped {
            self.st.pop_group();
        }
        match f {
            Flow::Normal => {
                self.pos = rb + 1;
                Ok(Flow::Normal)
            }
            // D-28: break は**そのブロック**を終了する。`if` / `else` の本体は
            // 状態に対して透明（D-1）なので break に対しても透明——素通しして、
            // その外側の本物のブロックが受け止める。
            // D-34: 段数が残っていれば1つ減らして上へ渡す。
            // 使い切ったら After を、**ブロックを閉じた位置で**解決する（D-38）。
            Flow::Break(n, after) if scoped => {
                self.pos = rb + 1;
                if n > 1 {
                    Ok(Flow::Break(n - 1, after))
                } else {
                    self.resolve_after(after)
                }
            }
            other => Ok(other),
        }
    }

    /// D-2r: ループ本体はスコープではなく**チェックポイント**。
    /// jump に巻き戻し先を与えるためだけにレベルを作り、正常に抜けるときは
    /// journal を適用せず破棄する——ループ内の蓄積は確定する。
    fn run_loop(&mut self) -> R<(Flow, Option<i64>)> {
        self.pos += 1;
        if *self.tok() != Tok::LBrace {
            return Err(self.err("`loop` の後には `{` が必要です"));
        }
        let lb = self.pos;
        let rb = self.match_brace(lb)?;
        self.st.push_checkpoint();
        self.no_comefrom += 1;
        let mut guard = 0u64;
        let r = loop {
            guard += 1;
            if guard > 50_000_000 {
                break Err(self.err("ループが停止しません（安全弁）"));
            }
            match self.exec_seq(lb + 1, rb) {
                Err(e) => break Err(e),
                // D-29: スコープを動かさないので巻き戻しは起きない。
                Ok(Flow::Normal) | Ok(Flow::Continue(_)) => continue,
                Ok(Flow::Break(n, after)) => {
                    self.pos = rb + 1;
                    if n > 1 {
                        break Ok((Flow::Break(n - 1, after), None));
                    }
                    let v = match &after {
                        After::Value(v) => *v,
                        _ => None,
                    };
                    match self.resolve_after(after) {
                        Ok(f) => break Ok((f, v)),
                        Err(e) => break Err(e),
                    }
                }
                Ok(Flow::Jump) => break Ok((Flow::Jump, None)),
            }
        };
        self.no_comefrom -= 1;
        self.st.discard_checkpoint();
        r
    }

    /// 段数を使い切ったあと、**ブロックを閉じた位置**で外側の文脈の動作を行う（D-38）。
    ///
    /// `goto` はここでは jump ではない——ブロックは正常終了しており、`jump_unwind`
    /// を呼ばない。裸の `goto` が D-5 の一括巻き戻しを起こすのに対し、`break goto`
    /// はブロック構造を尊重して閉じてから跳ぶ。ループ本体では journal の扱いが
    /// 破棄／適用で分かれる（D-2r）ので、この差は観測できる。
    fn resolve_after(&mut self, after: After) -> R<Flow> {
        match after {
            After::Value(_) => Ok(Flow::Normal),
            After::Continue(n) => Ok(Flow::Continue(n)),
            After::Goto(name, flat) => match self.scan_to_label_with(&name, flat) {
                Some(p) => {
                    self.pos = p;
                    Ok(Flow::Jump)
                }
                None => Err(self.err(&format!("label {} が前方に見つかりません", name))),
            },
            // 唯一、実行が続く終端。閉じたブロックの直後を戻り先として登録する。
            After::Comefrom(name) => {
                let here = self.pos;
                self.st.register_comefrom(&name, here);
                Ok(Flow::Normal)
            }
            After::Label(name) => match self.st.comefrom_target(&name) {
                Some(target) => {
                    self.st.jump_unwind();
                    self.pos = target;
                    Ok(Flow::Jump)
                }
                None => Ok(Flow::Normal),
            },
        }
    }

    /// 取られなかった分岐を実行せずに読み飛ばす。
    fn skip_stmt(&mut self) -> R<()> {
        while *self.tok() == Tok::Hash {
            self.pos += 1;
            self.pos = self.match_pair(self.pos, Tok::LBracket, Tok::RBracket)? + 1;
        }
        if *self.tok() == Tok::LBrace {
            self.pos = self.match_brace(self.pos)? + 1;
            return Ok(());
        }
        if self.tok().is_kw("if") {
            self.pos += 1;
            self.scan_expr()?;
            self.skip_stmt()?;
            if self.tok().is_kw("else") {
                self.pos += 1;
                self.skip_stmt()?;
            }
            return Ok(());
        }
        if self.tok().is_kw("loop") {
            self.pos += 1;
            self.pos = self.match_brace(self.pos)? + 1;
            return Ok(());
        }
        if self.tok().is_kw("fn") {
            self.pos += 1;
            self.ident()?;
            self.pos = self.match_paren(self.pos)? + 1;
            self.pos = self.match_brace(self.pos)? + 1;
            if *self.tok() == Tok::Arrow {
                self.pos += 1;
                self.skip_type_to_eol()?;
            }
            return Ok(());
        }
        let mut d = 0i32;
        while self.pos < self.eof {
            match self.tok() {
                Tok::LBrace | Tok::LParen => d += 1,
                Tok::RBrace | Tok::RParen => d -= 1,
                Tok::Semi if d <= 0 => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => {}
            }
            self.pos += 1;
        }
        Ok(())
    }

    /// §4.1-2: `goto` は対応する `label` の直前まで読み飛ばす。
    /// 文は一切実行せず、波括弧の深さのみを認識する（認識的スキップ）。
    /// 深さ 0 以下でのみ照合するので、より深いブロックの内側へは着地しない。
    /// D-67: `flat` なら波括弧の深さを一切見ない。最初に見つかった `label` に着地する。
    /// depth 基準（フレーム）の境界は越えない——そこは構造ではなく制御の境界である。
    fn scan_to_label_with(&self, name: &str, flat: bool) -> Option<usize> {
        let mut d = 0i32;
        let mut i = self.pos;
        // depth 基準を跨がない。フレームの外の label は見えない。
        let limit = self.frame_span.1.min(self.eof);
        while i < limit {
            match &self.toks[i] {
                Tok::LBrace => d += 1,
                Tok::RBrace => d -= 1,
                Tok::Ident(s) if s == "label" && (flat || d <= 0) => {
                    if matches!(self.at(i + 1), Tok::Ident(n) if n == name) {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    // ---- 式：走査（範囲の切り出し）と評価を分ける ----
    //
    // `:=` は式の**トークン範囲**から thunk を作るので、評価せずに末尾を
    // 決める操作が必須になる。走査と評価が同じ文法を二重に持つのは、
    // 遅延束縛を持つカーソルマシンの構造上の帰結。

    fn scan_expr(&mut self) -> R<(usize, usize)> {
        let s = self.pos;
        self.scan_bin(0)?;
        Ok((s, self.pos))
    }

    fn bin_level(t: &Tok) -> Option<u8> {
        Some(match t {
            Tok::OrOr => 0,
            Tok::AndAnd => 1,
            Tok::Eq | Tok::Ne | Tok::Lt | Tok::Gt | Tok::Le | Tok::Ge => 2,
            Tok::Plus | Tok::Minus => 3,
            Tok::Star | Tok::Slash => 4,
            Tok::Ident(s) if s == "mod" => 4, // D-7
            _ => return None,
        })
    }

    fn scan_bin(&mut self, min: u8) -> R<()> {
        self.scan_unary()?;
        while let Some(l) = Self::bin_level(self.tok()) {
            if l < min {
                break;
            }
            self.pos += 1;
            self.scan_bin(l + 1)?;
        }
        Ok(())
    }

    fn scan_unary(&mut self) -> R<()> {
        while matches!(self.tok(), Tok::Minus | Tok::Bang) {
            self.pos += 1;
        }
        self.scan_primary()
    }

    fn scan_primary(&mut self) -> R<()> {
        match self.tok().clone() {
            Tok::Int(_) => {
                self.pos += 1;
                Ok(())
            }
            Tok::LParen => {
                self.pos = self.match_paren(self.pos)? + 1;
                Ok(())
            }
            Tok::LBrace => {
                self.pos = self.match_brace(self.pos)? + 1;
                Ok(())
            }
            // `$frame { … }`（3.9節）
            Tok::Dollar if self.at(self.pos + 1).is_kw("frame") => {
                self.pos += 2;
                self.pos = self.match_brace(self.pos)? + 1;
                Ok(())
            }
            Tok::Ident(s) if s == "loop" => {
                self.pos += 1;
                self.pos = self.match_brace(self.pos)? + 1;
                Ok(())
            }
            // 式としての if（§3.4 の getroot がこの形）
            Tok::Ident(s) if s == "if" => {
                self.pos += 1;
                self.scan_expr()?;
                self.pos = self.match_brace(self.pos)? + 1;
                if self.tok().is_kw("else") {
                    self.pos += 1;
                    self.pos = self.match_brace(self.pos)? + 1;
                }
                Ok(())
            }
            Tok::Ident(_) => {
                self.pos += 1;
                if *self.tok() == Tok::LParen {
                    self.pos = self.match_paren(self.pos)? + 1;
                }
                // D-8: 呼出側の型アノテーション。
                if *self.tok() == Tok::Arrow {
                    self.pos += 1;
                    self.skip_type()?;
                }
                Ok(())
            }
            t => Err(self.err(&format!("式が必要です（{:?} が来ました）", t))),
        }
    }

    fn eval_range(&mut self, s: usize, _e: usize) -> R<i64> {
        let save = self.pos;
        self.pos = s;
        let v = self.eval_bin(0);
        // `loop` 式や関数呼び出しは文を実行するのでカーソルを動かす。必ず戻す。
        self.pos = save;
        v
    }

    fn eval_bin(&mut self, min: u8) -> R<i64> {
        let mut lhs = self.eval_unary()?;
        while let Some(l) = Self::bin_level(self.tok()) {
            if l < min {
                break;
            }
            let op = self.tok().clone();
            self.pos += 1;

            // D-11: `&&` / `||` は短絡評価する。
            if op == Tok::AndAnd && lhs == 0 {
                self.scan_bin(l + 1)?;
                lhs = 0;
                continue;
            }
            if op == Tok::OrOr && lhs != 0 {
                self.scan_bin(l + 1)?;
                lhs = 1;
                continue;
            }

            let rhs = self.eval_bin(l + 1)?;
            lhs = match op {
                Tok::OrOr => ((lhs != 0) || (rhs != 0)) as i64,
                Tok::AndAnd => ((lhs != 0) && (rhs != 0)) as i64,
                Tok::Eq => (lhs == rhs) as i64,
                Tok::Ne => (lhs != rhs) as i64,
                Tok::Lt => (lhs < rhs) as i64,
                Tok::Gt => (lhs > rhs) as i64,
                Tok::Le => (lhs <= rhs) as i64,
                Tok::Ge => (lhs >= rhs) as i64,
                Tok::Plus => lhs.wrapping_add(rhs),
                Tok::Minus => lhs.wrapping_sub(rhs),
                Tok::Star => lhs.wrapping_mul(rhs),
                Tok::Slash => {
                    if rhs == 0 {
                        return Err(self.err("0 除算"));
                    }
                    lhs / rhs
                }
                Tok::Ident(_) => {
                    if rhs == 0 {
                        return Err(self.err("0 剰余"));
                    }
                    lhs % rhs
                }
                _ => unreachable!(),
            };
        }
        Ok(lhs)
    }

    fn eval_unary(&mut self) -> R<i64> {
        match self.tok().clone() {
            Tok::Minus => {
                self.pos += 1;
                Ok(self.eval_unary()?.wrapping_neg())
            }
            Tok::Bang => {
                self.pos += 1;
                Ok((self.eval_unary()? == 0) as i64)
            }
            _ => self.eval_primary(),
        }
    }

    fn eval_primary(&mut self) -> R<i64> {
        match self.tok().clone() {
            Tok::Int(v) => {
                self.pos += 1;
                Ok(v)
            }
            Tok::LParen => {
                self.pos += 1;
                let v = self.eval_bin(0)?;
                self.eat(Tok::RParen, "`)`")?;
                Ok(v)
            }
            // `$frame { … }` — 呼び出しを伴わない depth 基準。ブロックをクロージャ
            // のように使うとき、内部の `getdepth()` がブロック自身から測るようになる。
            Tok::Dollar if self.at(self.pos + 1).is_kw("frame") => {
                self.pos += 2;
                if *self.tok() != Tok::LBrace {
                    return Err(self.err("`$frame` の後には `{` が必要です"));
                }
                let lb = self.pos;
                let rb = self.match_brace(lb)?;
                self.st.push_group();
                let outer_frame = self.st.enter_frame();
                let outer_span = self.frame_span;
                self.frame_span = (lb + 1, rb);
                let v = self.eval_block_value(lb + 1, rb, true);
                self.frame_span = outer_span;
                self.st.leave_frame(outer_frame);
                self.st.pop_group();
                self.pos = rb + 1;
                v
            }
            // 裸のブロックはスコープを作る（D-1）。
            Tok::LBrace => {
                let lb = self.pos;
                let rb = self.match_brace(lb)?;
                self.st.push_group();
                let v = self.eval_block_value(lb + 1, rb, true);
                self.st.pop_group();
                self.pos = rb + 1;
                v
            }
            Tok::Ident(s) if s == "loop" => {
                let (f, v) = self.run_loop()?;
                if f != Flow::Normal {
                    self.pending = Some(f);
                }
                Ok(v.unwrap_or(0))
            }
            // 式としての if。分岐の本体はスコープを作らない（D-1）。
            Tok::Ident(s) if s == "if" => {
                self.pos += 1;
                let (s0, e0) = self.scan_expr()?;
                let c = self.eval_range(s0, e0)?;
                let lb = self.pos;
                let rb = self.match_brace(lb)?;
                if c != 0 {
                    let v = self.eval_block_value(lb + 1, rb, false)?;
                    self.pos = rb + 1;
                    if self.tok().is_kw("else") {
                        self.pos += 1;
                        self.pos = self.match_brace(self.pos)? + 1;
                    }
                    Ok(v)
                } else {
                    self.pos = rb + 1;
                    if self.tok().is_kw("else") {
                        self.pos += 1;
                        let lb2 = self.pos;
                        let rb2 = self.match_brace(lb2)?;
                        let v = self.eval_block_value(lb2 + 1, rb2, false)?;
                        self.pos = rb2 + 1;
                        Ok(v)
                    } else {
                        Ok(0)
                    }
                }
            }
            // D-33: 現在のフレーム内で開いているブロックの数。`break` を何回
            // 重ねればこの関数（トップレベルならプログラム）を抜けられるか。
            Tok::Ident(s) if s == "getdepth" => {
                self.pos += 1;
                self.eat(Tok::LParen, "`(`")?;
                self.eat(Tok::RParen, "`)`")?;
                Ok(self.st.depth())
            }
            // §4.5: read() は入力ストリームを消費する。遅延束縛に含まれる場合、
            // 入力の消費も強制の時点まで遅れる。
            Tok::Ident(s) if s == "read" => {
                self.pos += 1;
                self.eat(Tok::LParen, "`(`")?;
                self.eat(Tok::RParen, "`)`")?;
                if self.input_at >= self.input.len() {
                    return Err(self.err("入力がありません"));
                }
                let v = self.input[self.input_at];
                self.input_at += 1;
                Ok(v)
            }
            Tok::Ident(name) => {
                self.pos += 1;
                if *self.tok() == Tok::LParen {
                    return self.eval_call(&name);
                }
                let c = self.value_of(&name)?;
                self.force(&c)
            }
            t => Err(self.err(&format!("式が必要です（{:?} が来ました）", t))),
        }
    }

    /// 関数呼び出し。
    ///
    /// D-20: 引数は呼出側で**即時強制**する。thunk は環境を捕捉しないので
    /// （クロージャなし）、遅延したまま渡すと引数名が呼ばれた側の束縛の下で
    /// 解決されてしまう——`f(x)` を `let f (x)` に渡した時点で自己参照になる。
    fn eval_call(&mut self, name: &str) -> R<i64> {
        let lp = self.pos;
        self.pos += 1;
        let mut args = Vec::new();
        while *self.tok() != Tok::RParen {
            let (s, e) = self.scan_expr()?;
            args.push(self.eval_range(s, e)?);
            if *self.tok() == Tok::Comma {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.pos = self.match_paren(lp)? + 1;
        // D-8: 呼出側の型アノテーション。Tier 1 は単相なので読み捨てる。
        if *self.tok() == Tok::Arrow {
            self.pos += 1;
            self.skip_type()?;
        }

        let def = match self.st.lookup(name) {
            Some(Binding::Func(d)) => d,
            Some(Binding::Val(_)) | Some(Binding::Flow(_)) => {
                return Err(self.err(&format!("{} は関数ではありません", name)))
            }
            None => return Err(self.err(&format!("未束縛の名前: {}", name))),
        };
        self.call(def, args)
    }

    fn call(&mut self, def: Rc<FuncDef>, args: Vec<i64>) -> R<i64> {
        if args.len() != def.params.len() {
            return Err(self.err(&format!(
                "{} は引数 {} 個ですが {} 個渡されました",
                def.name,
                def.params.len(),
                args.len()
            )));
        }
        if self.depth >= MAX_CALL_DEPTH {
            return Err(self.err("再帰が深すぎます"));
        }
        self.depth += 1;
        // 関数本体はスコープ（D-1）。引数の束縛も含めて抜ける際に巻き戻る。
        self.st.push_group();
        // D-31: この呼び出しより下へ jump が巻き戻らないようにする。
        // 引数束縛はこのレベルに入るので、消されるとフレームごと壊れる。
        let outer_frame = self.st.enter_frame();
        self.no_comefrom += 1;

        for (p, v) in def.params.iter().zip(args) {
            self.st.set(p, Binding::Val(thunk_value(v)));
        }

        let save = self.pos;
        let outer_span = self.frame_span;
        self.frame_span = (def.body.0 + 1, def.body.1);
        let mut r = self.eval_block_value(def.body.0 + 1, def.body.1, true);
        self.frame_span = outer_span;
        if r.is_ok() {
            if let Some(f) = self.pending.take() {
                r = Err(match f {
                    Flow::Break(n, _) => self.err(&format!(
                        "break の段数が足りません——関数の外へはあと {} 段抜けられません（getdepth() を確認してください）",
                        n
                    )),
                    Flow::Continue(_) => self.err("関数の外へ continue を持ち出すことはできません"),
                    _ => self.err("関数の外へ制御を持ち出すことはできません"),
                });
            }
        }
        self.pos = save;

        self.no_comefrom -= 1;
        self.st.leave_frame(outer_frame);
        self.st.pop_group();
        self.depth -= 1;
        r
    }

    /// ブロックを実行し、末尾式があればその値を返す。なければ 0（既定値）。
    ///
    /// 末尾式の判定は「残りのトークンがちょうど1つの式として読み切れるか」。
    /// 文はすべて `;` か `}` で終わるので、これで一意に決まる。
    /// `absorb` が false なのは `if` 式の分岐——本体は透明（D-28）なので
    /// break を吸収してはいけないが、式の位置からは外へ渡せないのでエラーにする。
    fn eval_block_value(&mut self, start: usize, end: usize, absorb: bool) -> R<i64> {
        self.pos = start;
        loop {
            if self.pos >= end {
                return Ok(0);
            }
            let save = self.pos;
            if self.scan_expr().is_ok() && self.pos == end {
                self.pos = save;
                let v = self.eval_range(save, end)?;
                // 末尾式の中のブロックから出てきた制御を、ここで1段吸収する。
                if let Some(Flow::Break(n, after)) = self.pending.take() {
                    let n = if absorb { n.saturating_sub(1) } else { n };
                    if n > 0 {
                        self.pending = Some(Flow::Break(n, after));
                        return Ok(0);
                    }
                    return match after {
                        After::Value(bv) => Ok(bv.unwrap_or(0)),
                        other => {
                            self.pending = Some(Flow::Break(0, other));
                            Ok(0)
                        }
                    };
                }
                return Ok(v);
            }
            self.pos = save;

            match self.exec_stmt()? {
                Flow::Normal => {}
                Flow::Jump => {
                    if self.pos >= start && self.pos < end {
                        continue;
                    }
                    return Err(self.err("値を返すブロックの外へ jump することはできません"));
                }
                // D-28: break はこのブロックの値を正格に束縛して終了する。
                // 関数本体でこれが起きれば、それが関数の返り値になる。
                Flow::Break(n, after) => {
                    // `if` 式の分岐は透明（D-28）なので吸収せず上へ預ける。
                    let n = if absorb { n - 1 } else { n };
                    if n > 0 {
                        self.pending = Some(Flow::Break(n, after));
                        return Ok(0);
                    }
                    return match after {
                        After::Value(v) => Ok(v.unwrap_or(0)),
                        other => {
                            self.pending = Some(Flow::Break(0, other));
                            Ok(0)
                        }
                    };
                }
                Flow::Continue(n) => {
                    self.pending = Some(Flow::Continue(n));
                    return Ok(0);
                }
            }
        }
    }

    /// thunk の強制。**環境は捕捉していない**ので、名前はこの時点の状態で引き直される。
    fn force(&mut self, cell: &Cell) -> R<i64> {
        {
            let t = cell.borrow();
            if let Some(v) = t.value {
                return Ok(v);
            }
            if t.forcing {
                return Err(self.err("自己参照 thunk を強制しました（発散）"));
            }
        }
        let (s, e) = {
            let mut t = cell.borrow_mut();
            t.forcing = true;
            t.src.ok_or_else(|| "値も式も持たない thunk".to_string())?
        };
        // §4.2: let の右辺の内部では comefrom トリガーを禁止する。
        self.no_comefrom += 1;
        let r = self.eval_range(s, e);
        self.no_comefrom -= 1;
        let mut t = cell.borrow_mut();
        t.forcing = false;
        let v = r?;
        t.value = Some(v);
        Ok(v)
    }
}
