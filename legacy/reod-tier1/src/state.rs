//! 束縛と、TeX の save stack と同型の状態管理。
//!
//! 設計書 §4.6 / §4.8、および docs/decisions.md の D-2r, D-4, D-5 を実装する。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// 束縛の中身。
///
/// `src` は式のトークン範囲。**環境を捕捉しない**——設計書 §4.6 の
/// 「名前解決自体も強制の瞬間まで先送りされる」により、強制時点の状態で
/// 名前を引き直す。この一点で `#[forward]` なしの再帰が成立する（D-6）。
#[derive(Debug)]
pub struct Thunk {
    pub src: Option<(usize, usize)>,
    pub value: Option<i64>,
    /// 強制中フラグ。自己参照 thunk の発散を検出する。
    pub forcing: bool,
}

pub type Cell = Rc<RefCell<Thunk>>;

/// 関数。クロージャを構成しないので環境を持たず、本体のトークン範囲だけを持つ。
/// 値（`Cell`）とは別の束縛種にしてあるため、`let g := f;` で関数を持ち回ることは
/// できない——「関数は第一級ではない」が構造的に保証される。
#[derive(Debug)]
pub struct FuncDef {
    pub name: String,
    pub params: Vec<String>,
    /// 本体の `{` と `}` のトークン位置。
    pub body: (usize, usize),
    /// 返り値型（D-8 の後置 `->`）。Tier 1 では型検査を持たないので保持のみ。
    pub ret: Option<String>,
}

/// D-39: 作用素式の別名。値でも関数でもない第3の束縛種。
///
/// 本体はトークン範囲として持ち、**使用のたびに使用位置で読み直される**。
/// 定義時には評価しない——`$repeat($break, getdepth())` の `getdepth()` は
/// 使用位置の深さを見なければ意味がないため。フレームも積まない。
#[derive(Debug)]
pub struct FlowDef {
    pub name: String,
    /// `=` の直後から `;` の手前まで。
    pub body: (usize, usize),
    /// D-68: 遅延した `#[flat]`。本体の `goto` が使用位置で flat になる。
    pub flat: bool,
}

#[derive(Debug, Clone)]
pub enum Binding {
    Val(Cell),
    Func(Rc<FuncDef>),
    Flow(Rc<FlowDef>),
}

pub fn thunk_expr(start: usize, end: usize) -> Cell {
    Rc::new(RefCell::new(Thunk {
        src: Some((start, end)),
        value: None,
        forcing: false,
    }))
}

pub fn thunk_value(v: i64) -> Cell {
    Rc::new(RefCell::new(Thunk {
        src: None,
        value: Some(v),
        forcing: false,
    }))
}

/// `^=`：現在保持している未評価の計算だけを複製する。
/// 複製対象の決定は即座に行われるが、中身の評価はなお遅延する（§4.6）。
pub fn thunk_clone_computation(src: &Cell) -> Cell {
    let t = src.borrow();
    Rc::new(RefCell::new(Thunk {
        src: t.src,
        value: if t.src.is_some() { None } else { t.value },
        forcing: false,
    }))
}

#[derive(Debug)]
enum Undo {
    Var(String, Option<Binding>),
    Comefrom(String, Option<(usize, usize)>),
}

pub struct State {
    vars: HashMap<String, Binding>,
    /// 名前ごとに1エントリの可変マップ（§4.1-1）。
    comefroms: HashMap<String, (usize, usize)>,
    /// levels[0] が global レベル。以降がスコープまたはチェックポイント。
    levels: Vec<Vec<Undo>>,
    /// D-31: jump の巻き戻しが降りられる下限。関数呼び出しのたびに更新する。
    /// 関数の中のコードにとっての「global な基準点」は関数の入口である——
    /// ここより下まで戻すと、引数束縛（フレームそのもの）が消えてしまう。
    frame_base: usize,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        State {
            vars: HashMap::new(),
            comefroms: HashMap::new(),
            levels: vec![Vec::new()],
            frame_base: 0,
        }
    }

    pub fn lookup(&self, name: &str) -> Option<Binding> {
        self.vars.get(name).cloned()
    }

    pub fn exists(&self, name: &str) -> bool {
        self.vars.contains_key(name)
    }

    /// トップレベル（levels.len() == 1）では記録しない。
    /// 「トップレベルには巻き戻し先となる `{ }` が存在しない」（§4.8）が
    /// 特別扱いではなくこの条件から自然に出る。
    fn record_var(&mut self, name: &str) {
        if self.levels.len() <= 1 {
            return;
        }
        let old = self.vars.get(name).cloned();
        let top = self.levels.last_mut().unwrap();
        if top
            .iter()
            .any(|u| matches!(u, Undo::Var(n, _) if n == name))
        {
            return; // TeX と同じく、1レベルにつき1回だけ記録する
        }
        top.push(Undo::Var(name.to_string(), old));
    }

    pub fn set(&mut self, name: &str, b: Binding) {
        self.record_var(name);
        self.vars.insert(name.to_string(), b);
    }

    /// D-4: global 書き込みは全 save level から当該セルのエントリを除去する（TeX 方式）。
    pub fn set_global(&mut self, name: &str, b: Binding) {
        for lvl in self.levels.iter_mut() {
            lvl.retain(|u| !matches!(u, Undo::Var(n, _) if n == name));
        }
        self.vars.insert(name.to_string(), b);
    }

    fn record_comefrom(&mut self, name: &str) {
        if self.levels.len() <= 1 {
            return;
        }
        let old = self.comefroms.get(name).copied();
        let top = self.levels.last_mut().unwrap();
        if top
            .iter()
            .any(|u| matches!(u, Undo::Comefrom(n, _) if n == name))
        {
            return;
        }
        top.push(Undo::Comefrom(name.to_string(), old));
    }

    /// §4.1-1: 通過するたびに無条件で上書き登録する。
    pub fn register_comefrom(&mut self, name: &str, pos: usize) {
        self.record_comefrom(name);
        let base = self.frame_base;
        self.comefroms.insert(name.to_string(), (pos, base));
    }

    /// 跳び先は登録された depth 基準の中でしか見えない。
    /// 見えなければ「保存されていない」のと同じ——§4.1-3 が何もしない。
    pub fn comefrom_target(&self, name: &str) -> Option<usize> {
        match self.comefroms.get(name) {
            Some((pos, base)) if *base == self.frame_base => Some(*pos),
            _ => None,
        }
    }

    // ---- スコープ（`{ }` のグルーピング）----

    pub fn push_group(&mut self) {
        self.levels.push(Vec::new());
    }

    /// 抜ける際に journal を適用する。TeX のグルーピングと同型（§4.8）。
    pub fn pop_group(&mut self) {
        if self.levels.len() <= 1 {
            return;
        }
        let lvl = self.levels.pop().unwrap();
        self.apply(lvl);
    }

    // ---- チェックポイント（ループ本体）----
    //
    // D-2r: ループ本体はスコープではない。jump に巻き戻し先を与えるためだけに
    // 記録レベルを作り、正常に抜けるときは journal を**適用せず破棄**する。
    // これにより、ループ内の非 global な蓄積はループを抜けても残る一方、
    // jump は §4.8 の「跨いだブロックの数に関わらず一括で」を満たす。

    pub fn push_checkpoint(&mut self) {
        self.levels.push(Vec::new());
    }

    /// 値の変更は蓄積するので破棄する。しかし **jump の跳び先は構造的**であり、
    /// 開いていない構文の中を指す跳び先は指す先が無いのと同じなので、
    /// comefrom の登録だけは適用する（飛び込みの禁止はここから出る）。
    pub fn discard_checkpoint(&mut self) {
        if self.levels.len() <= 1 {
            return;
        }
        let lvl = self.levels.pop().unwrap();
        let only_comefrom: Vec<Undo> = lvl
            .into_iter()
            .filter(|u| matches!(u, Undo::Comefrom(_, _)))
            .collect();
        self.apply(only_comefrom);
    }

    // ---- 呼び出しフレーム ----

    /// 関数本体に入る。戻り値を `leave_frame` に渡すこと。
    pub fn enter_frame(&mut self) -> usize {
        let old = self.frame_base;
        self.frame_base = self.levels.len() - 1;
        old
    }

    pub fn leave_frame(&mut self, old: usize) {
        self.frame_base = old;
    }

    /// D-33: 現在のフレーム内で開いているブロックの数。すなわち、この関数
    /// （トップレベルならプログラム）を抜けるのに `break` を何回重ねる必要があるか。
    /// `break` が吸収されるレベルだけを数えるので、`if` / `else` の本体は入らない。
    pub fn depth(&self) -> i64 {
        (self.levels.len() - self.frame_base) as i64
    }

    /// D-5: jump の巻き戻し。journal を内側から適用して空にするが、レベル構造は
    /// pop しない——「着地はカーソル移動以上の意味を持たない」（§4.1-2）ので、
    /// jump はブロック構造を変えず状態だけを戻す。
    /// D-31: 降りられるのは現在の呼び出しフレームの底まで。
    pub fn jump_unwind(&mut self) {
        for idx in ((self.frame_base + 1)..self.levels.len()).rev() {
            let lvl = std::mem::take(&mut self.levels[idx]);
            self.apply(lvl);
        }
    }

    fn apply(&mut self, lvl: Vec<Undo>) {
        for u in lvl.into_iter().rev() {
            match u {
                Undo::Var(n, Some(b)) => {
                    self.vars.insert(n, b);
                }
                Undo::Var(n, None) => {
                    self.vars.remove(&n);
                }
                Undo::Comefrom(n, Some(e)) => {
                    self.comefroms.insert(n, e);
                }
                Undo::Comefrom(n, None) => {
                    self.comefroms.remove(&n);
                }
            }
        }
    }
}
