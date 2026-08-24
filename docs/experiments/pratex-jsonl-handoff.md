# PraTeX向け UTF-8 JSON / JSONL codec 引き継ぎ

日付: 2026-08-25

返信先: `rtex/docs/validation/AUTOCHECKDATABASE/to-vaak/20260825-010859-utf8-jsonl-standard-library-request.md`

実装branch: `codex3/stdlib-json-jsonl`

実装checkpoint: `1bb004b252183ad4b3706a81470733ec3657d466`

## 結論

PraTeX固有schemaとfile capabilityを持たないpure Vaak標準ライブラリとして、UTF-8 JSONと
JSON Lines codecを追加した。Rust hostは次の順でsourceを一度だけ連結し、その全体を
`PreparedProgram`へ渡す。

```rust
let source = format!(
    "{}\n{}\n{}\n{}",
    vaak::stdlib::STRING,
    vaak::stdlib::JSON_UTF8,
    vaak::stdlib::JSONL_UTF8,
    application,
);
```

単一JSONだけなら`STRING`と`JSON_UTF8`まででよい。parse/check/type-check/compileをrecordごとに
繰り返す使い方は想定しない。

## 初版で固定した選択

- documentは再帰valueでなく、追加順のflat node/edge表
- object keyは入力・追加順を保存し、duplicate keyを拒否
- serializerはcompactで決定的。BOMを出さない
- number表現は`i64`だけ。小数・指数はcode 109、整数範囲外はcode 108
- `-0`は`0`へ正規化
- parse失敗は空documentと`root == -1`、serialize失敗は空bytes
- error offsetは0-based byte、lineとcolumnは1-basedでcolumnもbyte単位
- input、depth、node、decoded string/key、edge、outputを別budgetで制限

JSONL readerはowned chunkを`feed`し、LF、CRLF、終端LFなしを扱う。UTF-8 scalar、escape、numberの
途中でchunkを分けられる。空・空白だけのrecordはcode 400で、黙ってskipしない。record番号、
record開始の絶対byte位置、record内offset、絶対error位置を同時に返し、error後はterminalになる。
record、全feed総量、未読bufferを別budgetで制限する。

公開API、kind/status値、全error codeは[`stdlib/README.md`](../../stdlib/README.md)を正本とする。

## 検証

- JSON 11件: UTF-8、全escape、surrogate、i64両端、duplicate、位置、各budget、原子性
- JSONL 13件: UTF-8/escape/number途中のchunk、LF/CRLF/終端LFなし、位置、各budget、terminal state
- representative JSONL sourceをSTEELでnative実行
- source単独依存、全stdlib name collision、STEEL IR compileを既存library試験へ追加
- 全体: 833件中826 passed、既知のSTEEL native 6 failed、1 ignored

既知の6件はgraph 3、heap 1、ASCII I/O 1、string 1で、従来からのSTEEL alias array共有問題である。
JSON / JSONLの新規試験に失敗はない。

readerの毎record suffix copyと、未完recordを毎chunk先頭から再探索する案は二次的に増加したため
棄却した。raw値と再現commandは[`stdlib/BENCHMARK.md`](../../stdlib/BENCHMARK.md)へ残した。

## PraTeX側へ残す境界

このcheckpointはpath、file handle、network、process、PraTeX build manifest schemaを追加していない。
PraTeX/orchestratorが承認済みbytesをfeedし、生成bytesを全部検査した後だけpublishする。

次はこのcodecと別のhost capability契約として扱う。

- path解決、symlink、overwrite、atomic publish、file mode
- read/write/cancelのhost failure分類
- RunEpochへ結び付いたhandleと権限失効
- build manifest、diagnostic、source-mapのschema/version

現行readerには実行途中を止めるcancel primitiveは無い。同期bounded byte codecの範囲だけを完了とし、
PraTeX live-node phaseの`MaySuspend`、fuel、cancelとは接続しない。

## 権利境界

PraTeXの連絡から運んだのは要求、不変条件、failure条件だけである。GPL-3.0のsource、test、schema実装を
Vaakへ転記せず、codecとfixtureはMIT repository内で独立に作成した。
