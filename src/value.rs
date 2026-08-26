//! 値・セル・アリーナ。
//!
//! **値は自己完結している**（C-48）——別名は値の中に入らない。
//! したがって値のグラフに循環は無く、**深い複製は必ず停止する**。
//!
//! **記憶は領域ごとのアリーナ**（C-90）。領域を抜けたらまとめて捨てる。
//! 個々の値を辿る解放処理は走らない。

use crate::ast::ValueType;
use std::collections::{BTreeMap, HashMap};

/// ホストが**呼べる名前**に答える側（S-11）。
///
/// 番号は組み立ての時点で決まっている（`Program2::host_fns` の並び）。
///
/// **第一段は同期呼び出しである。** S-11 は界面の実装として中断・再開を選んだが、
/// それは**再入**（ホストが Vaak の実行中に Vaak を呼ぶ）を安く済ませるためであり、
/// 呼べること自体には要らない。**再入が要るようになったら中断へ移す。**
pub trait HostFns {
    fn call(&mut self, index: u16, args: &[Value]) -> Option<Value>;

    /// 直前の[`call`](HostFns::call)でhost契約違反を検出した場合に取り出す。
    ///
    /// 既存dispatcherは既定の`None`のままでよい。検査済みembedding dispatcherは、
    /// 返値の宣言型・schema違反をここへ置き、VMが後続命令を走らせる前に止める。
    fn take_contract_error(&mut self) -> Option<String> {
        None
    }
}

/// 呼べる名前を持たないホスト。
pub struct NoHostFns;

impl HostFns for NoHostFns {
    fn call(&mut self, _index: u16, _args: &[Value]) -> Option<Value> {
        None
    }
}

/// セルの番号。名前はセルを指し、セルは値を持つ。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct CellId(pub u32);

/// 値。**paradox はここに入らない**（C-85 (2)：セルは paradox を保持しない）。
///
/// # 大きさ
///
/// **16 バイト。** 大きいものは指すだけで、値そのものには入れない。
///
/// 以前は 72 バイトあった——`Map` が型を二つと木を直に持っていたからである。
/// スタック機械は値を積んでは降ろす。**降ろすたびに 72 バイト写していた。**
/// 加算一回が 92 ns（約 300 サイクル）掛かっていたのはこれである。
///
/// 集合体は `Box` の先にある。**集合体を積む機会はそもそも少ない**——
/// `[]` と `.` はアクセスであって複製ではない（C-20）ので、
/// 添字も欄も、集合体をスタックへ写さずに要素だけを取る。
#[derive(Clone, PartialEq, Debug)]
pub enum Value {
    /// `u1` が真偽値。
    U1(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    /// `u8 array` をラップした型（C-77）。
    Str(Box<Vec<u8>>),
    Array(Box<ArrayVal>),
    Map(Box<MapVal>),
    Hash(Box<HashVal>),
    Struct(Box<StructVal>),
}

#[derive(Clone, PartialEq, Debug)]
pub struct ArrayVal {
    pub elem: ValueType,
    pub items: Vec<Value>,
}

/// **鍵の値で飛ぶ連想**（C-98）。
///
/// 二段になっている:
///
/// ```text
/// entries : [ Some((鍵, 値)), None, Some((鍵, 値)), … ]   ← 入れた順。抜くと穴が空く
/// index   : 鍵 → entries の位置
/// ```
///
/// **抜くのは穴を空けるだけ**（O(1)）。`.keys()` は穴を飛ばして読むので、
/// **入れた順が費用ゼロで取れる。**
#[derive(Clone, Debug)]
pub struct HashVal {
    pub key: ValueType,
    pub val: ValueType,
    pub entries: Vec<Option<(MapKey, Value)>>,
    pub index: HashMap<MapKey, usize>,
}

impl HashVal {
    pub fn new(key: ValueType, val: ValueType) -> Self {
        Self {
            key,
            val,
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn get(&self, k: &MapKey) -> Option<&Value> {
        let i = *self.index.get(k)?;
        self.entries[i].as_ref().map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, k: &MapKey) -> Option<&mut Value> {
        let i = *self.index.get(k)?;
        self.entries[i].as_mut().map(|(_, v)| v)
    }

    pub fn insert(&mut self, k: MapKey, v: Value) {
        if let Some(&i) = self.index.get(&k) {
            if let Some((_, slot)) = self.entries[i].as_mut() {
                *slot = v;
                return;
            }
        }
        self.index.insert(k.clone(), self.entries.len());
        self.entries.push(Some((k, v)));
    }

    pub fn remove(&mut self, k: &MapKey) -> Option<Value> {
        let i = self.index.remove(k)?;
        let taken = self.entries[i].take().map(|(_, v)| v);
        // **穴が半分を越えたら詰め直す。** 抜き続けても並びが伸び続けない
        if self.entries.len() >= 16 && self.index.len() * 2 < self.entries.len() {
            self.compact();
        }
        taken
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
    }

    /// 穴を詰める。**入れた順は保つ。**
    fn compact(&mut self) {
        let live: Vec<_> = self.entries.drain(..).flatten().collect();
        self.index.clear();
        for (i, (k, _)) in live.iter().enumerate() {
            self.index.insert(k.clone(), i);
        }
        self.entries = live.into_iter().map(Some).collect();
    }

    /// 入れた順の（鍵, 値）。穴は飛ばす。
    pub fn iter(&self) -> impl Iterator<Item = (&MapKey, &Value)> {
        self.entries.iter().flatten().map(|(k, v)| (k, v))
    }
}

impl PartialEq for HashVal {
    /// **並びも含めて等しいか。** 入れた順が観測できる以上、順序は値の一部である
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.val == other.val
            && self.len() == other.len()
            && self.iter().zip(other.iter()).all(|(a, b)| a == b)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct MapVal {
    pub key: ValueType,
    pub val: ValueType,
    pub entries: BTreeMap<MapKey, Value>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct StructVal {
    pub name: String,
    pub fields: Vec<(String, Value)>,
}

impl Value {
    pub fn str(bytes: Vec<u8>) -> Value {
        Value::Str(Box::new(bytes))
    }

    pub fn array(elem: ValueType, items: Vec<Value>) -> Value {
        Value::Array(Box::new(ArrayVal { elem, items }))
    }

    pub fn map(key: ValueType, val: ValueType, entries: BTreeMap<MapKey, Value>) -> Value {
        Value::Map(Box::new(MapVal { key, val, entries }))
    }

    pub fn strukt(name: String, fields: Vec<(String, Value)>) -> Value {
        Value::Struct(Box::new(StructVal { name, fields }))
    }
}

/// 浮動小数を**単調な u64 へ写す。**
///
/// 生のビット列は数の順と一致しない——負の値ほど大きいビット列になり、
/// 正より後ろへ回る。だから並べ替えてから持つ。
///
/// **この写しは順序と等しさを同時に直す。** `Ord` も `Hash` も導いたままでよく、
/// `map`（並べる）と `hash`（混ぜる）が**同じ鍵を使える。**
///
/// `-0.0` は `0.0` へ潰す。**`-0.0 == 0.0` が真である以上、鍵も同じでなければならない**
/// ——でなければ `a == b` なのに `m[a]` と `m[b]` が別になる（同値関係の破れ）。
pub fn float_key(x: f64) -> u64 {
    // `x == 0.0` は `-0.0` にも真である
    let x = if x == 0.0 { 0.0 } else { x };
    let b = x.to_bits();
    if b >> 63 != 0 {
        // 負：ビット列の大小が数の大小と逆なので、反転する
        !b
    } else {
        // 非負：負より後ろへ回す
        b | (1 << 63)
    }
}

/// `float_key` の逆。
pub fn float_from_key(k: u64) -> f64 {
    let b = if k >> 63 != 0 { k & !(1u64 << 63) } else { !k };
    f64::from_bits(b)
}

/// 写像の鍵。**NaN が存在しないので比較は全順序である**（C-75）。
///
/// `Hash` も導く——`hash` は同じ鍵を使う（C-98）。
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum MapKey {
    Int(i128),
    Bytes(Vec<u8>),
    /// 浮動小数はビット列で並べる。非有限は存在しないので順序は全順序。
    Float(u64),
}

impl Value {
    pub fn type_of(&self) -> ValueType {
        match self {
            Value::U1(_) => ValueType::U1,
            Value::U8(_) => ValueType::U8,
            Value::U16(_) => ValueType::U16,
            Value::U32(_) => ValueType::U32,
            Value::I32(_) => ValueType::I32,
            Value::I64(_) => ValueType::I64,
            Value::F32(_) => ValueType::F32,
            Value::F64(_) => ValueType::F64,
            Value::Str(_) => ValueType::Str,
            Value::Array(a) => ValueType::Array(Box::new(a.elem.clone())),
            Value::Map(m) => ValueType::Map(Box::new(m.key.clone()), Box::new(m.val.clone())),
            Value::Hash(h) => ValueType::Hash(Box::new(h.key.clone()), Box::new(h.val.clone())),
            Value::Struct(t) => ValueType::Named(t.name.clone()),
        }
    }

    /// 整数として読む。混在は禁じられているので、呼ぶ側が型を保証する。
    pub fn as_int(&self) -> Option<i128> {
        Some(match self {
            Value::U1(b) => *b as i128,
            Value::U8(v) => *v as i128,
            Value::U16(v) => *v as i128,
            Value::U32(v) => *v as i128,
            Value::I32(v) => *v as i128,
            Value::I64(v) => *v as i128,
            _ => return None,
        })
    }

    pub fn as_float(&self) -> Option<f64> {
        Some(match self {
            Value::F32(v) => *v as f64,
            Value::F64(v) => *v,
            _ => return None,
        })
    }

    /// `while` の条件は**任意の整数型。`≠ 0` で継続**。
    pub fn is_nonzero(&self) -> bool {
        self.as_int().map(|v| v != 0).unwrap_or(false)
    }

    pub fn as_key(&self) -> Option<MapKey> {
        Some(match self {
            Value::Str(b) => MapKey::Bytes((**b).clone()),
            Value::F32(v) => MapKey::Float(float_key(*v as f64)),
            Value::F64(v) => MapKey::Float(float_key(*v)),
            _ => MapKey::Int(self.as_int()?),
        })
    }

    /// 表示。ホストへ返すときとテストのため。
    pub fn show(&self) -> String {
        match self {
            Value::U1(b) => (if *b { "1" } else { "0" }).to_string(),
            Value::U8(v) => v.to_string(),
            Value::U16(v) => v.to_string(),
            Value::U32(v) => v.to_string(),
            Value::I32(v) => v.to_string(),
            Value::I64(v) => v.to_string(),
            Value::F32(v) => fmt_float(*v as f64),
            Value::F64(v) => fmt_float(*v),
            Value::Str(b) => format!("{:?}", String::from_utf8_lossy(b)),
            Value::Array(a) => {
                let s: Vec<String> = a.items.iter().map(|v| v.show()).collect();
                format!("[{}]", s.join(", "))
            }
            Value::Map(m) => {
                let s: Vec<String> = m
                    .entries
                    .iter()
                    .map(|(k, v)| format!("{} => {}", show_key(k), v.show()))
                    .collect();
                format!("({})", s.join(", "))
            }
            Value::Hash(h) => {
                let s: Vec<String> = h
                    .iter()
                    .map(|(k, v)| format!("{} => {}", show_key(k), v.show()))
                    .collect();
                format!("({})", s.join(", "))
            }
            Value::Struct(t) => {
                let s: Vec<String> = t
                    .fields
                    .iter()
                    .map(|(n, v)| format!("{n} := {}", v.show()))
                    .collect();
                format!("{}({})", t.name, s.join(", "))
            }
        }
    }
}

fn fmt_float(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

fn show_key(k: &MapKey) -> String {
    match k {
        MapKey::Int(v) => v.to_string(),
        MapKey::Bytes(b) => format!("{:?}", String::from_utf8_lossy(b)),
        MapKey::Float(k) => fmt_float(float_from_key(*k)),
    }
}

/// セルの置き場。**領域ごとのアリーナ**（C-90）。
///
/// スコープを抜けるときは `truncate` するだけ。
/// **個々の値を辿らない**——値は自己完結しているので、辿る必要が無い。
pub struct Arena {
    cells: Vec<Option<Value>>,
}

impl Arena {
    pub fn new() -> Self {
        Self { cells: Vec::new() }
    }

    /// **中身だけ捨てる。** 容量は残す——次の実行で同じだけ要る。
    ///
    /// ホストが繰り返し呼ぶとき、毎回の確保が積み上がる。
    /// 領域の一括解放（`release`）と同じ原理を、**実行そのものにも掛ける。**
    pub fn clear(&mut self) {
        self.cells.clear();
    }

    /// 印を付ける。スコープの入口で取り、出口で `release` に渡す。
    pub fn mark(&self) -> usize {
        self.cells.len()
    }

    /// 印まで捨てる。**ポインタを一つ戻すだけ。**
    pub fn release(&mut self, mark: usize) {
        self.cells.truncate(mark);
    }

    pub fn alloc(&mut self, v: Option<Value>) -> CellId {
        let id = CellId(self.cells.len() as u32);
        self.cells.push(v);
        id
    }

    pub fn get(&self, id: CellId) -> Option<&Value> {
        self.cells.get(id.0 as usize).and_then(|v| v.as_ref())
    }

    pub fn get_mut(&mut self, id: CellId) -> Option<&mut Value> {
        self.cells.get_mut(id.0 as usize).and_then(|v| v.as_mut())
    }

    pub fn set(&mut self, id: CellId, v: Value) {
        if let Some(slot) = self.cells.get_mut(id.0 as usize) {
            *slot = Some(v);
        }
    }

    /// 値を**取り出す**。写さない。
    ///
    /// 走り終わってホストへ返すときに使う——**セルはもう要らない**ので、
    /// 複製する理由が無い。
    pub fn take(&mut self, id: CellId) -> Option<Value> {
        self.cells.get_mut(id.0 as usize).and_then(|v| v.take())
    }

    pub fn is_live(&self, id: CellId) -> bool {
        (id.0 as usize) < self.cells.len()
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}
