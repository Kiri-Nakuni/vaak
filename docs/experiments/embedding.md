# DSL が通常計算を Vaak へ委譲する埋め込み実験

## 対象

これはゲーム用 scripting や汎用 plugin API の提案ではない。TeX のように、固有の仕事には強いが、
配列を走査して計算し、複数の更新を組み立てる処理は書きにくい DSL をホストとして想定した。
ホスト状態は `count : i32 array`、`dimen : i64 array`、文書へ送る文字列は `emit` として模した。

実行例は [`examples/embedding_tex.rs`](../../examples/embedding_tex.rs)、Vaak 側は
[`tex_batch.vaak`](../../examples/experiments/tex_batch.vaak) と
[`tex_callback.vaak`](../../examples/experiments/tex_callback.vaak) に分けた。

## 推奨する境界：snapshot → 計算 → command buffer → commit

非 callback 版は次の順に動く。

1. ホストが `count` / `dimen` の snapshot を純粋な Vaak 値へ写す。
2. 一度だけ組み立てた VM program が通常計算を行う。
3. Vaak は `count_set` / `dimen_set` / `emit` を表す `Command array` を返す。
4. ホストは命令列全体を状態のコピーへ適用して検証する。
5. すべて通った場合だけ、元の状態と一括交換する。

Vaak が命令を三つ作った後で `1 / 0` に失敗する経路も入れた。この場合は返り値が無いため、
ホストは適用を始めない。さらに、返った命令列の二番目が範囲外だった場合も、ホスト側が
状態のコピーを捨てるので一番目だけが残ることはない。

これは Vaak 自体へ transaction や巻き戻しを足したものではない。C-2 の「巻き戻さない」は
変わらず、**作用をまだホストへ渡していない**ため、ホストが batch 全体を原子的に扱える。

## 同期 HostFn との違い

比較版は同じ計算の後、`tex_count_set`、`tex_dimen_set`、`tex_emit` を同期 HostFn として順に呼ぶ。
三つを呼んだ後で同じ実行時エラーを起こすと、callback が既に変更したホスト状態は残る。

| 境界 | 後続の Vaak 失敗 | 向く処理 |
|---|---|---|
| command buffer | ホストが適用しなければ作用なし | 複数更新、検証してからの commit |
| 同期 HostFn | それ以前の作用は残る | 即時問い合わせ、逐次作用が意味そのものの場合 |

したがって、DSL が苦手な通常計算の委譲では command buffer を既定にするのが扱いやすい。
HostFn は「今この場でホストへ尋ねないと次を計算できない」境界へ限定する。

## ホスト値を関数へ隠して渡さない

C-96 により、ホスト値は最上位からだけ見える。Vaak 側の関数は次のように `alias` 引数で受ける。

```text
fn make_batch (count_snapshot : i32 array alias,
               dimen_snapshot : i64 array alias,
               should_fail : u1 alias) { ... } -> Command array;

make_batch(count, dimen, fail)
```

これにより、関数がどのホスト状態へ依存するかと、大きい snapshot を関数境界で深く複製しないことが
署名に現れる。`fn f () { count[0] }` と直接読む形は、検査器が C-96 を示して拒む。

ここでの `alias` はホスト実体への生の参照ではない。VM へ入れた snapshot のセルを同じ実行内で
共有するだけであり、batch 版の実行中に元の TeX 状態へ作用する道は無い。

## compile once / run many

`BatchVm::compile` は解析、名前・型検査、`compile_with_host` を一度だけ行い、`Program2` と
`Runner` を保持する。`BatchVm::plan` は実行ごとに新しい snapshot だけを渡す。
試験では同じ `BatchVm` を異なる二状態で走らせ、結果が混ざらないことを確認した。
実行例では同じ program を 10,000 回走らせる所要時間も表示する。

この分離は「TeX の一回の処理中に同じ Vaak 定義を何度も呼ぶ」形に対応する。ソースの解析や
バイトコード化を callback ごとに繰り返さず、`Runner` の arena・stack・frame 容量も再利用できる。

## 得られた設計判断

- 通常計算の入力は snapshot、出力は値として検証できる command buffer にする。
- batch の適用可否は最上位外界面を受け取るホストが決める（C-31 / C-95）。
- ホスト値への依存は必ず関数の `alias` 引数へ書く（C-96）。
- 同期 HostFn は巻き戻らない逐次作用であることを API の契約として見せる。
- compile と run を別の段にし、`Program2` / `Runner` をホストが保持する。

この実験には core の構文・意味論・実装変更は要らなかった。

## 二車線を混ぜない

TeX engine が提供する埋め込み経路は、用途の違う二車線として扱う。

| 車線 | 境界 | 呼出し頻度 | 主な用途 |
|---|---|---:|---|
| **内蔵 Vaak** | rtex 等と同一 process の Rust 内部 API | 高くてよい | node access、hook、軽い逐次 policy |
| **外向き WASM** | linear memory と import/export ABI | 低く抑える | 独立させたい重い kernel、大きな一括処理 |

**node list と line breaking の主案は内蔵 Vaak 車線である。** WASM 用の bulk snapshot を
主案へ置き換えない。host は段落または組版 phase ごとに一度だけ Vaak へ入り、以後の制御 loop は
Vaak が持つ。Vaak は整数 handle で native `NodeOps` を高頻度に pull し、named hook/policy を呼ぶ。
同一 process なので node ごとの完全な serialize/deserialize は無い。

## node list と line breaking の native probe

[`tex_linebreak_nodeops.vaak`](../../examples/experiments/tex_linebreak_nodeops.vaak) は、単純化した
box/glue node list を greedy に行分割する。構成は次の通りである。

```text
rtex/host
  -- paragraph phase を一回開始 --> Vaak control loop
                                      |
                                      +-- node_hook(hook_id, NodeHandle)
                                      |     native HostFn / NodeOps
                                      |     21 logical calls/node
                                      |
                                      +-- Line[start, end, natural][]
  <-- validated replace result ------+
  -- 全体検証後だけ line list を交換
```

組版では node ごとの論理 hook を無くすこと自体が目標ではない。一 node につき 21 hook が必要なら、
21 回とも行う。重要なのは、rtex と Vaak の同一 process 内部 API または Vaak の通常 named function で
完結させ、node ごとに外向き WASM ABI を越えないことである。

実験の `NodeOpsLineBreaker` は現在の `HostFns::call(index, &[Value])` をそのまま内部 NodeOps として使う。
node index は probe 上の `NodeHandle` であり、host arena の node 自体を Vaak 値へ複製しない。
host→Vaak の entry は一段落一回、NodeOps 呼出しは最低 `21 × node_count` 回である。この二つは別に数える。

返り値は host の line list を直接書き換えず、`Layout { lines, logical_hook_calls }` という replace 案にする。
host は start/end が連続して全 node を一度ずつ覆うこと、natural width が元 node 幅の再計算と一致すること、
target width を越えないことを全件検証する。全て通った後だけ既存 line list と交換する。

## 外向き WASM 車線と三段 bridge

比較用の [`tex_linebreak_bulk.vaak`](../../examples/experiments/tex_linebreak_bulk.vaak) は、node arena を
`kind[] / width[] / penalty[]` の SoA snapshot として一括で受ける。21 logical hooks/node は消さず、
probe 内の通常 `policy_hook` 呼出しへ移す。外側 ABI crossing を phase entry と結果返却だけにする
**データ形を現行 native VM 上で模したもの**であり、実際の WASM module でも WASM timing でもない。

これは native node path の代替案ではなく、次の三段 bridge が必要な場合の外部車線モデルである。

```text
TeX engine
  -> 内蔵 Vaak が NodeHandle/NodeOps で必要範囲を走査・集約
  -> 重い独立 kernel を WASM へ一回だけ bulk call
  -> Vaak/host が patch・replace result を検証
  -> TeX engine が commit
```

WASM descriptor は一つの連続 buffer でも複数の SoA offset+length でもよい。各範囲が linear memory 内に
収まること、配列長が一致すること、borrow 中に memory growth や host arena の移動をしないこと、出力容量と
全 handle/range を commit 前に検証することが必要である。外部情報を node ごとに import するのではなく、
内蔵 Vaak が materialize して一回で渡す。

## 呼出し費用の測り方

`benchmark_vm_calls` は解析・検査・bytecode compile を計時外にし、`Program2` と `Runner` を再利用して
次を同じ反復数で測る。

1. 加算だけの空 loop
2. 一引数の Vaak 通常関数を呼ぶ loop
3. 一引数の native `HostCall`
4. 二引数の native `HostCall`

実行例はそれぞれ 1,000,000 反復を 5 標本取り、中央値を ns/iteration で表示する。合否は時間の大小に
依存させず、HostCall が指定回数だけ実際に起きたことだけを回帰試験にする。概算では
`HostCall2 × 21` を総費用、`(HostCall2 - empty hook loop) × 21` を二引数 native dispatch の
追加費用として見る。named function との差は符号も含めて別に比較する。

ここで測る HostCall は **native 同一 process** の値 slice + dynamic dispatch である。WASM call の
marshalling、sandbox transition、linear-memory validation は含まない。したがってこの数値を WASM import の
費用と同一視せず、外向き WASM 車線は呼出し回数を phase 単位へ下げる。

### 観測値

2026-08-22、Windows x86-64（Intel Family 6 Model 186）、Rust 1.98.0、release build の複数回観測である。
HostFn 名は計時前に `u16` index へ解決し、hot call は整数比較だけにした。call microbenchmark は
1,000,000 反復 × 5 標本の中央値。各 candidate の直前・直後に空 loop を走らせ、その平均を引いた
paired extra を主値にする。candidate の順も標本ごとに回転した。同時作業中で絶対値は振れたため、
複数回で観測した範囲を記し、時間は回帰試験の合否条件にしていない。

| 経路 | absolute ns / iteration | paired extra | absolute / local |
|---|---:|---:|---:|
| 空 loop + 加算 | 200–221 | — | — |
| Vaak 一引数 named function | 541–615 | 340–378 ns | 1.000 |
| native HostCall 一引数 | 293–303 | 75–90 ns | 0.493–0.541 |
| native HostCall 二引数 | 313–332 | 105–130 ns | 0.540–0.579 |

現在 VM では HostCall が通常 Vaak 関数より遅い、とは限らなかった。HostCall は compile 時に決めた index
から直接 `HostFns::call` へ行く一方、利用者関数は frame/cell を作るためである。したがって native NodeOps
をすべて Vaak 関数へ写し直す最適化は勧めない。

21 calls/node の paired 追加分は一引数 HostCall で約 1.6–1.9 μs、二引数で約 2.2–2.7 μs である。
実際の 9,999-node 行分割は 1 phase entry、210,524 native NodeOps calls。並行負荷により 162–319 ms と
振れた。比較用の SoA bulk 入力 simulation は 209,979 logical named hooks を probe VM 内で呼び
351–577 ms で、複数回とも native NodeOps 版の方が約 1.8–2.3 倍速かった。
**これは actual WASM timing ではなく**、どちらも同じ native VM 上である。

`node_hook(hook_id, handle)` は汎用二引数 API なので、Rust 側でも hook ID を分配する。実装時は
`node_width(handle)`、`node_kind(handle)` のように、compile 時に index が決まる一引数 NodeOps を用途別に
並べる方がよい。測定では一引数の paired 追加分が約 75–90 ns、二引数が約 105–130 ns であり、
静的に分かれた一引数 API は引数一個と Rust 側 dispatch の両方を減らせる。

結論は、内蔵 Vaak 車線では native `NodeOps` の高頻度呼出しを許容してよく、外向き WASM 車線ではこの
native 数値を根拠に細粒度 import を許可してはいけない、である。WASM は別途、実際の runtime と memory
descriptor で測る必要がある。

## 用語

| 用語 | この実験での意味 |
|---|---|
| logical hook | 組版 policy 上必要な node ごとの処理。最適化で勝手に消さない |
| phase entry | host が一段落・一組版段階について Vaak control loop を開始する一回の呼出し |
| NodeHandle | host arena の node を指す不透明 ID。probe では整数 index |
| NodeOps | handle から kind/width/penalty 等を答える native 内部 API |
| ABI crossing | 特に process/WASM の外向き境界を越える call/import |
| patch / replace result | host 状態へ直接作用せず返す、全体検証可能な更新案 |
| commit | 検証成功後だけ host の状態を一括交換する操作 |

## callback の結論と API の形

host → Vaak の event は、第一級 callback を登録する形ではなく、**phase-level prepared entry** として扱う。
host が解析・検査・compile 済み program と `Runner` を保持し、段落開始などの event で entry を一度呼ぶ。
環境を捕まえる第一級 closure は、参照なら C-48（別名は値に入らない）、写しなら C-33（深い複製）に
反するため、この言語には合わない。

Vaak → host は二級に分ける。

| 級 | 契約 | 例 |
|---|---|---|
| **Leaf NodeOps** | 同期、再入禁止、短時間、compile 時 index、native 同一 process | `node_width(handle)` |
| **MaySuspend** | TeX/Vaak 再入または外向き WASM を要する。VM を中断し、借りを切ってから再開 | 重い外部 kernel |

Leaf call の最中に host が同じ `Runner` へ再入してはいけない。再入が必要な操作を同期 `HostFn` へ混ぜず、
S-11 の第二段である suspend/resume 側へ送る。複数 policy も runtime の function list や closure array にはせず、
compile 時に順序を固定した named dispatcher にする。callee と HostFn index を hot loop の前に決められる。

production の `NodeHandle` は裸の配列添字ではなく、少なくとも arena epoch と slot generation を含む
opaque scalar にする。別 arena の handle と、削除・再利用後の stale handle は NodeOps が拒む。欄値の直接編集は
C-2 により後続失敗でも戻らないので局所的な即時作用に限り、topology 変更、複数 node 更新、原子的な行分割は
`Patch` / `BreakPlan` を返して全体検証後に commit する。

現行 API で不足しているもの:

- `HostFn::call -> Option<Value>` は「返り値なし」と host runtime error を区別できない。
  `Result<Option<Value>, HostError>` 相当か、MaySuspend の failure 結果が要る。
- `Host::run` は毎回 parse/check/compile する。低水準の `Program2` / `Runner` では実証できたが、host item の
  layout、prepared entry、入力 snapshot を束ねる公開 API がまだ無い。
- 同期 HostFn には suspend/resume と安全な再入がまだ無い（S-11 第二段）。
- `NodeHandle` は probe では `i64` に過ぎず、opaque type・epoch・generation の検査が無い。
- 高頻度 NodeOps 向けには、汎用二引数 `hook_id + handle` より静的な一引数 leaf API と、host error を
  作らない fast contract を明文化する余地がある。
- `HostBinding` の契約は「変わっていれば write」とするが、現在の `Host::run` は全 live binding へ
  無条件に write する。TeX の save stack のような書戻し手続を余計に走らせる。
- 公開 `Runner::run_with` は runtime error 時に `after` を捨てる。C-2 どおり途中変更を回収する低水準 API は
  crate 内にしかなく、外部 embedder が error と writeback を同時に受け取れない。
- `Program2::host_touched` は `Host::run` へ統合されておらず、read set と write set も分かれていない。
  未使用 binding まで snapshot/read/write し得る。
- `HostBinding::read_at` / `write_at` はまだ無く、動く添字の leaf access を集合体全体の往復なしで
  答える標準経路が無い。

これらは既存 API の監査結果であり、この実験枝では core を修正しない。
