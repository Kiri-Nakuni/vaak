# モジュールと細粒度ライブラリの設計試案

> **状態：実験案。決定ではない。** Claude 側でも同時に検討しているため、構文・意味論・core API を
> この文書では確定しない。`docs/vaak/decisions.md` の決定を変更するものでもない。

## 1. 先に分けるもの

モジュールの**意味**は一つにし、読み込み方だけを実行形態ごとに分ける。

| 共通にするもの | STEEL | 埋め込みインタプリタ / VM |
|---|---|---|
| canonical module ID、依存辺、公開宣言、名前解決、source map | 全依存を集めて静的に検査・特殊化・LLVM IR 化する | host が解決済み集合を一度 prepare し、何度も走らせる |
| content / interface / artifact key、compiler/dialect version | parse・型検査・object/IR cache を分ける | parsed AST / bytecode cache と host layout の鍵を分ける |
| import cycle の扱い | SCC を一コンパイル単位にして縮約 DAG を並べる | 同じ SCC と公開表を prepare 時に一度作る |
| 診断上の module path | multi-file source map へ残す | filesystem を使わない resolver でも同じ表示にする |

**実行中に import を解決しない。** 埋め込み用途で欲しいのは loader の高速化ではなく、loader 自体を
hot path から消すことである。

## 2. 最小の意味モデル

概念上は次の四段に分ける。

1. `Resolver`: `(from_module, specifier)` を canonical `ModuleId` と内容へ解決する。
2. `ResolvedGraph`: module と import 辺を重複なく持つ。ID の文字列順を tie-break に使う。
3. `LinkedProgram`: 公開表・名前解決・型検査を終え、module 名を含む内部名へ束縛した AST。
4. `PreparedProgram`: 木を辿る実装向けの宣言表、または VM の `Program2`、または STEEL artifact。

説明用の Rust API は次の程度で足りる。綴りは未決定である。

```rust
pub trait ModuleResolver {
    fn resolve(&mut self, from: Option<&ModuleId>, specifier: &str)
        -> Result<ModuleUnit, ResolveError>;
}

pub enum ModuleBody {
    Source(std::sync::Arc<str>),
    // `Program` を裸で受けず、source id と検証済み fingerprint を伴う不透明な型にする。
    Parsed(std::sync::Arc<ParsedModule>),
}

pub struct ModuleUnit {
    pub id: ModuleId,
    pub content_fingerprint: ContentFingerprint,
    pub body: ModuleBody,
}

pub fn prepare(
    root: ModuleUnit,
    resolver: &mut dyn ModuleResolver,
    backend: Backend,
) -> Result<PreparedProgram, PrepareError>;
```

`Source` は CLI / STEEL 用、`Parsed` は生成済み・埋め込み済み標準ライブラリ用である。現行の
`ast::Program` をそのまま公開入力にすると、`Span` が source ID を持たず、各 parser の `NodeId` も
0 から始まるため、複数 module を結んだ時点で診断位置と node identity が衝突する。したがって
`ParsedModule` は少なくとも `(SourceId, Program)` を持つ不透明な値とし、link 後の node は
`(SourceId, NodeId)` で識別する。`Span` 自体を直すか side table にするかは core API に触れるので未決定とする。

`content_fingerprint` は内容の指紋であり、後述の interface fingerprint や artifact key とは別物である。
同じ canonical ID を別経路から解決して内容指紋が違ったら、どちらかを採用せず resolver error にする。
将来 bytecode を直に受けるなら、**信頼済みの検証済み形式**に限り、compiler version、dialect、Value ABI
を鍵へ必ず含める。任意の byte 列を `Program2` として信用しない。

この Rust trait は prepare-time の host interface であり、Vaak の第一級 callback ではない。hot run からは
一度も呼ばれないので、language-level callback の費用・捕捉問題とは別である。

### 2.1 import 先は宣言だけにする案を第一候補とする

C-36 により関数宣言は初期化順を持たず、相互再帰できる。一方、変数宣言は位置から先に見え、評価順を
持つ。この差を module 境界でも保存する。

- import 可能な module は当面 `fn` / `struct` / `wrap` / `flow` と公開指定だけを持つ。
- 実行式と可変セルは root module にだけ置く。
- Vaak の `const` は compile-time 定数でなく、セルを凍結する束縛である。したがって第一段の library module
  へ `const` を例外的に許さない。大きな事前計算表が必要なら root/host から alias で渡すか、将来の
  static data 設計として初期化 graph と一緒に検討する。
- import graph の循環自体は、module が宣言だけなら初期化順を生まない。SCC 単位で全 header を同時に
  宣言表へ入れた後、下記の**宣言ごとの依存 graph**を別々に検査する。
- 構造体の物理的な型依存は module graph と別に検査し、C-63 の DAG 制約を保つ。

これなら「ファイルを読んだ副作用」も「初期化順を決めるためだけの import 順」も生じない。将来 module
初期化が本当に要るなら、宣言 graph と initialization DAG を別物として追加検討する。

### 2.2 構文より先に固定すること

- module ID は filesystem path そのものではない。host が `stdlib/string/find` を静的 table にも、
  package 内ファイルにも対応させられる。
- import specifier は top-level の静的な綴りに限る。値から module を選ぶ dynamic import は持たず、
  graph は実行前に閉じる。
- 同じ canonical ID は graph 中に一度だけ現れる。
- 公開名は module namespace に属し、内部では `(ModuleId, local name)` で一意にする。
- named type の identity は `(定義 ModuleId, local type name)` であり、利用側の import alias を含めない。
  現行の `ValueType::Named(String)` / `StructVal.name` を生の表示文字列のまま使うと host ABI で衝突するため、
  内部 type ID と表示名を分ける必要がある。この core API 変更は rtex/Claude と合意してから行う。
- wildcard import の有無に関係なく、曖昧な非修飾名は静的エラーにする。
- 依存順や hash table の反復順で artifact が変わらないよう、全 tie-break を canonical ID / 宣言位置で
  決定する。

### 2.3 import graph と混ぜてはいけない graph

module SCC を通っただけでは link は終わらない。少なくとも次を別に持つ。

| graph | 循環 | 理由 |
|---|---|---|
| module import | 許す | 宣言だけなら初期化順が無い |
| 関数 call | 許す | C-36 の相互再帰 |
| struct の値包含 | **拒否** | C-63 の有限な深い複製を守る。配列等を挟んでも辺を消さない |
| wrap の基底型依存 | 当面は循環を拒否 | C-63 は struct の決定なので同一視しないが、循環 wrap の構築・型解決を未規定にしない |
| `flow` 展開 | **拒否** | `flow` は使用位置で読み直すため。現行検査器も本体内の別 `flow` を拒否する |
| module 初期化 | 当面存在しない | root 以外に実行式と可変セルを置かないため |

構造体の default 式も定義 module の名前空間で link する。現在の評価器は default を構築位置で評価するため、
単純な AST 連結だけでは呼び出し側の名前を偶然捕捉しうる。module 内に可変セルを置かない第一段では、
default から参照できるのを link 済みの宣言だけに制限する。

### 2.4 名前の coherence

内部名は宣言の種類を問わず `(ModuleId, local name)` にする。利用者定義メンバ関数は現行実装では
`"型名.関数名"` の大域表へ入るため、別 module が同じ型へ同名 method を足すと最後の一つが勝ちうる。
第一段は次のいずれかが必要であり、構文を決める前に Claude 側の案と揃える。

- named type の method は、その type を宣言した module にだけ置ける。
- 組み込み型を拡張する library は method でなく module-qualified free function を公開する。

後者なら `str` や `i64 array` に第三者が method を足したときの衝突を避けられる。少なくとも、同一 module
内の関数・型・method の重複は link error にする。現行の収集器には `flow` 以外を `HashMap::insert` で
上書きする経路があるので、module linker が暗黙の上書きを引き継いではならない。

### 2.5 実装前の gate

次の四点が Claude 側の案と一致するまでは、import 構文や core 型を実装しない。

1. **source identity**：multi-file の `Span` / `NodeId` をどう一意にし、paradox と静的エラーを元 module へ戻すか。
2. **type identity**：named type を `(definition ModuleId, local name)` とし、host ABI と表示名へどう渡すか。
3. **method coherence**：named type の所有 module と、`str` / array 等の組み込み型 extension をどう衝突させないか。
4. **prepared host layout**：host names/signatures/slot/read-write plan をいつ pin し、registry 変更をどう検出するか。

これは新しい決定ではなく、二つの並行案を比較するための acceptance criteria である。構文の好みより先に
この四つを照合する。

`module`、`use`、`import` のどれを表面構文にするか、公開指定を宣言側と利用側のどちらへ置くかは、
Claude 側の案と比較するまで決めない。

## 3. STEEL の graph pipeline

STEEL は単なる source concatenation にしない。

1. root から全 module header を読み、canonical ID と辺を確定する。
2. Tarjan 等で SCC を求め、SCC 縮約 graph を作る。
3. 縮約 DAG を依存先優先・ID tie-break の決定的順序で並べる。
4. SCC ごとに宣言 header を先に集め、C-36 の相互再帰を保ったまま名前・型・効果を検査する。
5. export interface hash を算出する。実装 hash が変わっても interface が不変なら依存側の再検査を
   省ける余地を残す。
6. root entry と、生成物として明示された公開 entry から到達可能な宣言だけを LLVM IR へ落とす。
7. module-qualified symbol へ mangle し、LLVM 側の internal linkage / inlining / DCE を使う。

関数 callback を受ける汎用アルゴリズムは Vaak の書き味にも木を辿る実装の速度にも合わない。
STEEL では module instance を静的特殊化して `op` 等を直接呼び、LLVM が inline できる形が望ましい。
ただし module parameter / template 構文を今すぐ言語仕様へ足すことはしない。最初は型・演算別の薄い
module、または build-time source generation で実測する。

### 3.1 cache の鍵は三つに分ける

「fingerprint」を一つにまとめると、型検査 cache は正しくても古い実装を実行する事故が起きる。

| 鍵 | 含めるもの | 再利用できるもの |
|---|---|---|
| content key | canonical module ID、source/parsed content、parser version、dialect | parse 結果 |
| interface key | 公開名、完全修飾型、alias / `var`、構造体欄、default の有無 | 依存側の名前・型検査 |
| artifact key | 到達した全実装の content key、`flow` 本体、default 式、host signature、target triple/data layout、LLVM・compiler version、最適化設定、Value/host ABI | IR/object/executable |

全体最適化では依存関数が inline されるため、**interface が同じでも実装が変われば artifact は無効**である。
また `flow` と構造体 default は利用側へ展開されるので、本文が変われば依存 artifact も無効にする。
interface key は「再型検査を省ける」という狭い用途に留める。

循環 import の interface key を「相手の interface key を再帰的に含む」と定義してはいけない。SCC 内の
型参照は canonical `(ModuleId, name)` のまま正規化し、SCC 全 module の公開 header を ID/位置順に並べた
一つの SCC interface key を作る。縮約 DAG の外側だけが依存 SCC の key を取り込む。

辺 `A -> B` を「A が B を import する」と定義し、縮約 DAG は B を A より先に処理する。Tarjan の
発見順や `HashMap` の反復順は使わず、隣接辺、SCC 内 module、宣言を canonical ID と source position で
並べてから番号・symbol を付ける。DCE の root は実行 root だけでなく、生成物として外へ公開すると指定した
entry も含める。

## 4. 埋め込み側の準備と反復実行

高頻度に行うのは `run` だけにする。

```text
host startup / configuration
  resolver tableを構築
  → resolve graph
  → parse/check/link
  → tree declarations または Program2 を prepare
  → PreparedProgram を保持

hot phase
  同じ prepared layout へ host values / HostFn 実装を差し替え
  → PreparedProgram.run(...)
  → 結果と書き戻し集合を受け取る
```

標準ライブラリは `&'static str` または build 時に解析した形式で登録できる。PraTeX の文書処理中に
filesystem、UTF-8 source decode、module graph 構築、静的検査を繰り返さない。

現行にも低水準部品はある。AST は `parser::parse` 後に再利用でき、VM は `Program2` と `Runner` を
再利用できる。しかし高水準 `Host::run(&str)` は毎回 parse/check/compile する。module 実装より先に、
`Host::prepare` と prepared host layout を分離すると測定しやすい。

**steel4 の S-22 で host 界面の既知の四穴は既に塞がった。** `read_at` / `write_at` / `len`、
`host_reads` / `host_writes`、誤り時にも残る `run_writeback`、変更時だけの書き戻しを module 側で
再実装しない。そこで測った `Host::run` は約 310 µs で、支配項は毎回の parse/check/compile だった。
prepared high-level API はこの残った支配項を外す仕事である。

`PreparedProgram` は immutable な link/compile 結果、`Runner` は一実行系列が持つ mutable な arena/stack
と分ける。これなら同じ prepared program から runner を複数作れ、共有可能性と再入可能性を混同しない。
VM には既にこの形の `Program2` + `Runner` がある。木を辿る実装は現在 `run` のたびに宣言を収集し、
scope/arena も実行状態と同居するため、`PreparedInterp` の宣言表と `InterpRunner` の状態を分けない限り、
「AST を保存した」だけでは hot run から準備費用は消えない。

### 4.1 host layout も prepare の一部である

`Program2` の host function は登録順の番号で呼ばれ、host value も slot 順に並ぶ。したがって prepare 後に
名前の追加・無効化・型変更が起きたのに古い番号を使うことは許せない。

- prepare key に、実際に参照する host value / function の `(name, type/signature, mutability, ABI version)` を入れる。
- run は同じ `PreparedHostLayout` の値だけを順番どおり受ける。名前検索は prepare 時だけにする。
- host registry を可変に保つ API なら generation を比較し、不一致時は re-prepare を要求する。
- S-22 の `host_reads` / `host_writes` の結果も layout に固定し、静的添字だけの read/write plan を毎回解析しない。

module を一度読んでも host layout を毎回作れば、埋め込みの小さい処理ではそちらが支配する。測定は
`resolve + parse + link + compile`、`layout bind`、`run`、`writeback` を別々に取る。

## 5. 配列ライブラリは細かく分ける

一つの `array` module に全機能を集めない。基礎層は相互依存を避け、高級層だけが必要なものを選ぶ。
以下の path は説明用である。

| module | 主な操作 | 依存 |
|---|---|---|
| `range/check` | `0 <= first <= last <= len`、長さ | core のみ。配列を受けない |
| `array/i64/copy` | 別配列への半開区間 copy | `range/check` |
| `array/i64/fill` | 半開区間 fill | `range/check` |
| `array/i64/reverse` | 半開区間 reverse | `range/check` |
| `array/i64/rotate` | 半開区間 rotate | `range/check`, `array/i64/reverse` |
| `array/i64/search/linear` | find/from、rfind、contains、count | `range/check` |
| `array/i64/search/binary` | lower/upper bound、equal range | `range/check` |
| `array/i64/compare/lex` | lexicographic compare | `range/check` |
| `array/i64/check/sorted` | is_sorted | `range/check` |
| `array/i64/extrema` | min/max/argmin/argmax | `range/check` |
| `array/i64/partition` | partition、sorted unique、compact | `range/check`, 必要な比較だけ |
| `array/i64/sort/insertion` | 小配列・小区間の in-place ascending sort | `range/check` |
| `array/i64/sort/heap` | ascending、worst-case `O(n log n)`、追加配列なし | `range/check` |
| `array/i64/sort/merge` | stable ascending sort、作業配列 `O(n)` | `range/check`, `array/i64/copy` |
| `array/i64/select` | nth/select/top-k | `range/check`, `array/i64/partition` |
| `array/i64/prefix_sum` | 長さ `n+1` の prefix sum、range sum | `range/check` |
| `array/i64/difference` | difference、range add の復元 | `range/check` |
| `array/i64/permutation` | next permutation、inverse、compose | `range/check`, `array/i64/reverse` |
| `array/i64/compress` | 座標圧縮 | sort 一種 + binary search |
| `array/i64/sliding_min` | 固定比較の sliding minimum | `ds/deque_i64` |
| `array/i64/sliding_max` | 固定比較の sliding maximum | `ds/deque_i64` |

さらに element type ごとに実体を分ける。Vaak には現在 generic type parameter も第一級関数も無いので、
`array/i64/search/binary`、`array/str/search/linear` のような型別 module が正直である。公開名が衝突
しない module namespace が入れば、利用側で短い別名を選べる。

上の表で `i64` を明記したのは、見かけだけの generic module を作らないためである。`u8`、`str`、
`f64` 等は実際に使うものから別 module として足す。`window` のように predicate callback が必要な名前は
基礎 module にせず、`sliding_min` のように演算まで固定する。`array/all` のような facade は、再 export の
意味と unused module の除去が決まってから置く。

依存 DAG の規律は次とする。

- 下層は上層を import しない。特に compare/check/extrema は sort を知らず、`range/check` は配列を知らない。
- 一つの操作に stable/in-place/allocating が混ざるなら module か名前で分ける。
- `compress -> sort` の向きだけを許し、便利 API から基礎へ逆辺を作らない。
- module ごとの試験に加え、依存 graph 自身が DAG であり canonical 順が再現する試験を置く。

細粒度 module は**読み込み・link・到達可能性の単位**であり、参照実装の hot loop から小関数を毎要素
呼ぶことを強制しない。`swap` や整数比較を共通関数にして sort の内側から呼ぶと frame 費用が乗るため、
固定演算は loop に直接書く。STEEL が確実に inline できる場合との速度差を測り、source 上の重複と実行時
抽象化の費用を混同しない。

### 5.1 API の性能規律

- 読み取りは `xs : T array alias`、破壊は `var xs : T array alias`。値引数による深い複製を既定にしない。
- subarray alias は作らず、`(xs alias, first, last)` の半開区間で渡す。
- `range/check` 自体は `is_valid(len, first, last) -> u1` のような scalar predicate にし、query/mutation
  それぞれが自分の失敗契約を選べるようにする。
- copy は、一つの可変 alias だけを取る overlap-safe `copy_within` と、source/read alias + destination/write
  alias の `copy_between` を分ける。同じ root を二つの alias 引数へ渡せない現行規則と衝突させない。
- in-place と allocating を名前・module で分ける。
- **空の半開区間は範囲外ではない。** sum/prod は identity、`count` は 0、`is_sorted` は true、
  min/argmin/find は paradox と、操作ごとに契約を書く。Vaak の 0 周 loop は paradox なので、identity を
  返す API は loop の外で空を先に処理する。
- 不正な読み取り query は paradox を返せる。一方、値を返さない破壊的関数は成功時も外界面が paradox
  なので、失敗と同じ経路へ載せられない。範囲違反を runtime error にするか、`u1` 等の status を返すかを
  API ごとに決める。「すべて paradox」で一括しない。
- 各 API に前提、空入力、計算量、追加領域、整数の折り返しを併記する。
- literal を alias 引数へ直接渡せないので、delimiter や探索鍵は loop 外で名前へ束縛する。便利 API が
  必要なら値渡し wrapper を別名で足し、hot API と混ぜない。

### 5.2 `str` も「文字列一式」に戻さない

`str` は byte 列を包む型（C-77）なので、分割軸を名前に出す。

| module | 単位 |
|---|---|
| `string/byte/range` | byte index の slice/copy |
| `string/byte/search` | byte sequence の find/rfind/contains |
| `string/byte/split` | delimiter byte sequence、空 delimiter の契約を明記 |
| `string/ascii/case` | ASCII lower/upper/equal-ignore-case |
| `string/ascii/trim` | ASCII whitespace の集合を明記 |
| `string/utf8` | valid/code-point count/境界。書記素とは呼ばない |
| `string/algo/z`, `string/algo/prefix_function`, `string/algo/suffix_array` | 競プロ向け algorithm |

組み込み `str` method を無制限に増やすのではなく、衝突しない module-qualified free function を基本にする。
module 機構が入る前の単一 source library は、この分割を section comment と public prefix で先取りし、後で
機械的に割れるようにする。

## 6. AC Library から借りる設計上の教訓

参照した一次資料：

- <https://github.com/atcoder/ac-library>
- <https://atcoder.github.io/ac-library/production/document_en/>
- <https://atcoder.github.io/ac-library/production/document_en/segtree.html>
- <https://atcoder.github.io/ac-library/production/document_en/lazysegtree.html>
- <https://atcoder.github.io/ac-library/production/document_en/string.html>
- <https://atcoder.github.io/ac-library/production/document_en/scc.html>

AC Library は全入り口を持ちながら、実体は fenwick tree、segment tree、DSU、文字列、数学、畳み込み、
flow、SCC、2-SAT へ分けている。各操作が制約・計算量・空区間の意味を公開契約にしている点を借りる。
一方で C++ template/function pointer をそのまま Vaak へ写さない。

Vaak での初期 inventory は次の順がよい。

array/string のような組み込み型 algorithm は module-qualified free function、DSU/Fenwick/segment tree の
ように library 自身が named struct を所有するものは同じ module の member function とする。後者は
`var self` で破壊、読み取り self で query と分けられ、C-20 の「receiver は複製しない」を活かせる。

### 層 A：現行機能だけで純 Vaak 実装しやすい

- `ds/dsu_i64`: union by size + path compression。整数配列一本で表せる。
- `ds/fenwick_i64`: point add / range aggregate。整数配列一本で表せる。
- `ds/heap_i64`: binary min/max heap。配列の push/pop で stack 専用型は不要。
- `ds/deque_i64`: ring buffer。先頭 remove の `O(n)` を避けるため、stack より優先度が高い。
- `ds/bitset_u32`: `u32 array` と bit 演算。**現行 Vaak に `u64` は無い。** S-23 の
  `count_ones` / `leading_zeros` 等を利用できる。
- `graph/csr`: offsets + edges の structure-of-arrays。graph module の共通土台。
- `graph/scc`: CSR と明示 stack で再帰を避ける。
- `graph/two_sat`: SCC の上に literal 番号と含意辺だけを足す。
- `string/z`, `string/prefix_function`: byte / integer array で `O(n)`。
- `math/gcd`, `pow_mod`, `inv_mod`, `crt`, `floor_sum` の幅別 module。

ACL の `groups()` や `scc()` は入れ子配列を返すが、初期 API はそこを必須にしない。DSU は
`leaders : i64 array`、SCC は `group_count : i64` と `group_of : i64 array` を返す構造体を基礎にする。
必要なら `groups` を作る allocating facade を上層に置く。SCC の component 番号は縮約 graph の
topological order という契約を持たせ、同点の並びも Vaak 側では決定的にする。

### 層 B：演算別の静的特殊化が欲しい

- `ds/segtree/i64/sum|min|max|gcd`
- `ds/lazy_segtree/i64/range_add_sum|range_add_min|range_assign_min|range_assign_max`
- sparse table / disjoint sparse table
- Dijkstra、LCA、heavy-light decomposition

generic callback を毎 node で呼ぶ形は参照実装で高い。まず固定演算版を実装し、STEEL の直接呼び出しと
木を辿る実装の inline `switch` を測る。module specialization はその必要性を確認してから設計する。

#### callback 無しの segment tree をどう切るか

最初の公開版は operation ごとに hot loop を複製する。たとえば sum 版は `op(a,b)` を呼ばず `a + b` を
loop 本体へ直接書き、min 版は S-23 の `a.min(b)` を直接書く。共通化するのは bounds、木の index 計算、
試験 vector であって、実行時 callback ではない。

ACL の `max_right` / `min_left` は predicate まで template 引数にするが、Vaak では用途を含む名前へ分ける。
例：`sum_max_right_le(first, limit)`（要素非負が前提）、`max_max_right_lt(first, target)`。
predicate の単調性と identity に対する真を契約へ書く。任意 predicate 版は第一段に置かない。

lazy segment tree は monoid と action の直積だけ固定版が増える。網羅しようとせず、実需要のある
`range_add_sum` 等だけを独立 module にする。build-time source generation は比較 probe としてはよいが、
任意式を受け取る generator を公開仕様にすると template/macro 機構を別名で導入することになるため、
module 意味論とは切り離す。

比較する三形は次である。

1. 演算別 module（hot loop に演算を直書き）。
2. `op_tag : u8` + `switch`（一実装だが combine ごとに分岐）。
3. 固定名 `op` を呼ぶ skeleton（関数 frame の基準値を取るためだけ）。

参照実装 / VM では 1 を既定候補とし、STEEL では 1 と、build 時に生成して LLVM が inline した版を比べる。
同じ API 名が backend によって別の意味を持つことは許さず、特殊化は link 前に決める。

### 層 C：backend の primitive または新しい数値設計が欲しい

- NTT convolution / arbitrary-mod convolution
- modint（modulus の静的・動的 identity と overflow 中間幅が要る）
- maxflow / min-cost flow の大規模 workload
- suffix array の線形版

純 Vaak の参照実装を置けても、STEEL の optimizer が認識できる primitive、または型・modulus ごとの
特殊化を用意しないと実用速度になりにくい。参照実装と accelerated backend は同じ試験ベクトルで揃える。

NTT 用の剰余値は当面 `i64` に正規化して持つ。現行に `u64`/`i128` が無く、`u32 * u32` は中間値が
先に折り返すためである。modint を値型として足す議論と、固定 modulus の関数 module を足す議論は分ける。

### 6.1 実装順

1. `range/check` と `array/i64/search/*`。空区間・paradox・alias の API 規律をここで固める。
2. DSU、Fenwick、heap、deque。現行の配列と S-23 だけで完結し、参照実装の frame/添字費用も測れる。
3. `segtree/i64/sum|min|max`。callback 無しの特殊化三形を比較し、module specialization の必要性を判断する。
4. CSR、SCC、2-SAT。再帰を明示 stack に直し、flat result API の書き味を確認する。
5. lazy segment tree、Dijkstra/LCA/HLD。固定演算・固定用途で需要のある組だけを足す。
6. NTT/modint、maxflow/min-cost flow、suffix array。数値幅または backend acceleration の方針後に進める。

「AC Library にある順」ではなく、Vaak の意味論と実装費用を一段ずつ測れる順である。

## 7. sum 型 / match 構文を足さない再現

この文書でいう `array_sum_i64` や Fenwick の sum は集約演算であり、**sum 型ではない**。

複数形の値は既存機能だけで次のように表す。

- `struct` に `tag : u8` と全 payload 欄を置く。
- constructor 相当の自由関数で tag と使う欄を揃える。
- 消費側は `switch(value.tag)` を使う。
- 再帰 AST は arena または parallel arrays の整数 handle を payload にする。
- 不正な tag / 欄の組は constructor 規約、`is_valid -> u1`、`switch` の unmatched → paradox で検出する。

これは**表現規約**であって型安全な直和ではない。現行には module-private field / opaque constructor が
無いため、利用側は tag を直接書き換え、使わない payload との不整合を作れる。module 第一段で privacy まで
同時に足さないなら、`make_*` / `is_valid` の規約と debug 用検査で守る。`wrap` を variant ごとに使っても、
異なる wrap 値を一つの引数・配列へ入れる共通型が無いので、この穴は埋まらない。

`T | paradox` は組み込みの「値が無いかもしれない」には使えるが、paradox はセルに入らず情報も運ばない。
したがって `Option<T>` 相当の返り方には使えても、payload を持つ複数 variant や失敗理由の区別にはならない。

再帰 AST を**現行機能だけ**で作る基準形は、`tags : u8 array` と payload の parallel arrays、子を指す
`i64` index である。本文中の「arena」は将来の一級 arena 提案と区別し、現時点ではこの index convention を
指す。一級 arena が採用された場合だけ、同じ配置を typed handle に置き換える。走査関数はこれらを
`alias` で受け、node ごとに配列全体を複製しない。

測るべき比較は、(a) struct array + tag switch、(b) tag/payload の parallel arrays、(c) tag を関数へ渡す、
(d) switch を hot loop 内へ直接置く、の四つである。木を辿る実装では集合体 index の深い複製と named
function frame が支配し得るため、構文追加の是非ではなく library のデータ配置を決める測定になる。

測定では次を分ける。

- `xs[i].tag` と直接読む場合と、`let x := xs[i]` で構造体全体を束縛（深く複製）する場合。C-20 では
  前者の access は複製しないはずだが、現行参照実装の `field` は base が単純な名前でないと一度
  `need_value` し、`step_get` は構造体を clone する。差が出ないなら library の結論より先に nested path
  access の実装穴として扱う。
- scalar payload だけの場合と、配列 payload を含む場合。
- tag が一様、交互、偏った分布の場合（branch prediction の差）。
- parse/check を除いた prepared 相当の反復実行と、CLI 全体時間。
- 参照実装、VM、STEEL の同じ checksum。10 回以上の paired run で中央値とばらつきを残す。

予測だけで「疑似直和は安い」とは決めない。AoS は locality がよい一方、現在の `Value::Struct` は欄を名前で
探し、集合体 payload の clone は深い。SoA は invariant を二つ以上の配列へ分散する一方、scalar access だけで
済む。参照実装では SoA が勝つ可能性が高く、STEEL では AoS が勝つ可能性もあるため、backend 共通の結論を
先に置かない。

## 8. 次の probe

1. 10〜20 module の DAG、diamond、declaration-only SCC、同名 export、欠落 module、同一 ID で内容相違を
   resolver mock で検査する。異なる module の同じ byte offset が正しい source 名で診断されることも含める。
2. source concatenation と linked/prepared AST / `Program2` を 1,000 回走らせ、resolve、parse、check、compile、
   host layout、hot run、writeback を別々に測る。S-22 の read/write 集合を再利用する。
3. content/interface/artifact の三 cache で、(a) private body だけ変更、(b) signature 変更、(c) `flow` body
   変更、(d) target/opt level 変更の invalidation 表を試験にする。
4. `range/check`、`array/i64/search/linear`、`array/i64/search/binary` を純 Vaak で書く。空区間、負、境界、
   duplicate key の契約を参照実装・VM・STEEL で揃える。
5. DSU、Fenwick、固定演算 segment tree を純 Vaak で書き、参照実装・VM・STEEL を比較する。segment tree は
   直書き、tag + switch、named `op` call の三形を同じ workload で測る。
6. tagged value の AoS/SoA、直接 access/深い束縛、scalar/aggregate payload を参照実装・VM・STEEL で測る。
7. Claude 側の module 案と、identity、source map、cycle、initialization、visibility/coherence、cache、host layout
   の差だけを照合する。構文案の優劣は、その差が揃ってから比較する。

## 9. 未解決のまま残すもの

- import/export/re-export/alias の表面構文。wildcard を第一段へ入れるか。
- import を Vaak の「paradox を産む宣言式」にするか、実行領域の外にある module header とするか。
- `Span` に `SourceId` を足すか、linked side table にするか。
- `ValueType::Named(String)` を内部 `TypeId` へ移す際の host/rtex API 移行。
- named type method の所有規則と、組み込み型 extension を free function だけにするか。
- 循環 wrap を拒否する規則を core に置くか、module linker の保守的制限に留めるか。
- field/function privacy と疑似 variant の invariant を同時に扱うか。
- `PreparedInterp` の immutable plan / mutable runner の具体形、`Send`/再入の契約。
- cache の保存場所・上限・破損検証。SCC key の具体的 encoding。
- library の invalid range を paradox、status、runtime error のどれにするかという API 別の表。
- generic を持たないまま固定版をどこまで増やすか。source generation を公開機能にするかは測定後に限る。

いずれもこの文書では決定しない。probe の結果と Claude 側の案を `decisions.md` へ持ち込む前に比較する。
