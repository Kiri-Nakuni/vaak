# 作業の分担

**この版方は二人以上の担い手で進めている。** 衝突しないように、まず担当を確かめること。

| | 担当 | 触る場所 |
|---|---|---|
| **Claude** | **この版方（Vaak／mydsl）** | `src/` `docs/` `examples/` `editors/` |
| Codex | [rtex](https://git.trap.jp/Suima/vaak-rtex)（別版方） | rtex の中だけ |

**枝は `speculative`。**

rtex 側は Codex が持っている——pdfTeX・e-upTeX・kpathsea 相当が残っており、
**そちらの方が重い。**

## rtex 側から来る依頼

`src/vaak.rs`（rtex 側）が Vaak の API を使っている。
**API を変えたら rtex 側に知らせること。**

そして **S-11**（ホストが呼べる名前も見せられる）は Vaak 側の実装が要る。
rtex が `tex.print` を欲しがっている——`\directvaak` がレジスタしか触れないため。

---

## 最初に読むもの

1. **[docs/vaak/decisions.md](docs/vaak/decisions.md)** — **決定の唯一の記録。**
   `C-n` は依頼者の承認済み。`S-n` は担い手が決めた分
2. [docs/vaak/probe.md](docs/vaak/probe.md) — **理由を書かない仕様書**
3. [docs/LICENSING.md](docs/LICENSING.md) — **rtex は GPL-3.0。向きが一方通行**
4. [claude_memo.md](claude_memo.md) — 逐次の作業記録

## 絶対に破らないこと

| | |
|---|---|
| **rtex のコードを写さない** | rtex は **GPL-3.0**、Vaak は **MIT**。写した時点で Vaak 全体が GPLv3 になる。運んでよいのは**測定結果と設計判断だけ** |
| **決定を二箇所で実装しない** | C-61 と C-93 で二度やった誤りである。`|>` の畳み込みを検査器が持っていなかった（S-17）のもこれ |
| **プローブに理由を書かない** | 純粋な反論をもらうため。`probe.md` は「何がそうであるか」だけを書く |
| **却下理由を捨てない** | 誤りは誤りとして残す。S-16 も S-18 も「一度入れて撤回した」経緯ごと書いてある |
| **`docs/vaak/1x-*.md` は提案である** | 決定は `decisions.md` にしかない。`C-n` を引くときは**その C-n を読んでから引く** |

## 実装の規律

### 木を辿る実装が参照実装である（S-5）

```
src/interp.rs   ← **これが正しい。** 迷ったらこちらに合わせる
src/vm.rs       ← バイトコード VM。参照実装と一致しなければならない
src/steel.rs    ← LLVM IR。同上
```

**食い違いは必ず参照実装の側が勝つ。** S-12（`! 0`）も S-13（`if (0)`）も S-16（分岐は領域）も、
すべて VM／STEEL の側が誤っていた。

差分試験は `tests/differential.rs` と `tests/steel.rs` にある。**新しい構文を足したら両方に足す。**

### 試験の書き方

- 試験の名前は**日本語**。何を確かめているかを書く（`fn 分岐が空になっても高さが揃う()`）
- **例は試験である。** `examples/vaak/` の五本は `tests/examples.rs` が
  参照実装と VM の両方で走らせている
- `cargo test --release` が全部通ること（いま **289 通過**）

### コミットの書き方

**日本語で、何をしたかではなく「なぜそうしたか」を書く。**

```
S-16 を直す：分岐は領域である

if の分岐は領域（C-20）なので、中身が空になれば外界面は paradox（C-14 規則2）。
VM は else の無い側にだけ Paradox を積んでいた。
…
```

決定に触ったら、**`docs/vaak/decisions.md` に `S-n` として書いてからコミットする。**

---

## いま積んでいること

**上から順に。** 判断が要るものは `S-n` に書いて枝を切る。

**0. S-11（ホスト関数）** — rtex が待っている。方針は決着済み:
言語の表面はホスト関数、界面の実装は中断・再開。最初は `tex.print`。

### 1. STEEL 第四段（`src/steel.rs`）

S-19 で配列と `str` まで入った。**まだ無いもの**:

| | 難しさ |
|---|---|
| **写像（`map`）** | **大。** 実行時の道具が要る（連想の表）。要素の型ごとに比較を出す |
| **構造体（`struct`）** | 中。LLVM の構造体型に落ちる。欄の位置は静的に決まる |
| **入れ子の集合体**（`i64 array array`） | 中。**深い複製が再帰する。** 型ごとに写す関数を出す |
| `alias` の局所束縛（`&=`） | 中 |
| `outward` | 大。フレームを越える脱出。関数の返り方が変わる |
| 動く段数の `$repeat` | 中 |

**先に入れ子の集合体を薦める。** 深い複製の再帰が要るので、写像と構造体の土台にもなる。

#### 場の解放（S-19 の残り）

いま**関数の境でだけ**印を戻している。関数の中のループが場を伸ばす:

```
nfor (i, 0, 1000000) { let a := new i64 array(10, 0); };   ← 尽きる（終了コード 70）
```

ループ本体で戻すには**逃げ出す値の解析**が要る。
**C-14 が「領域は高々一つの値」と決めているので、代入だけが逃げ道である**——
外の名前への代入を見れば済む。

### 2. `read_at` / `write_at`（`src/host.rs`、S-15）

ホストの集合体を**丸ごと写さずに要素だけ問う道**。

```rust
pub trait HostBinding {
    fn type_of(&self) -> ValueType;
    fn read(&self) -> Value;
    fn write(&mut self, v: &Value);

    // 足すもの。**既定は丸ごと読んでから取り出す**ので、
    // 答えられないホストは何もしなくてよい
    fn read_at(&self, i: usize) -> Option<Value> { … }
    fn write_at(&mut self, i: usize, v: &Value) -> bool { … }
}
```

**動く添字が 1340 ns 掛かっている**（S-15 で測った）。静的な添字は 22 ns なので、
そこだけが高い。`Program2::host_touched` が `None` を返す場合に効く。

### 3. 人間向けのリファレンスと付録（`docs/` に新規、最低優先度）

依頼者の指定:

- 本文は <https://doc.rust-jp.rs/book-ja/> を参考に、**より簡潔に**
- **付録**で `Akasha` という名称・`vaak` の語源・メンタルモデルに触れる
- 日本語

**`decisions.md` を読んでから書くこと。** 理由が全部そこにある。

---

## 版方の地図

| | |
|---|---|
| `src/lexer.rs` `src/parser.rs` | 字句と構文。**束縛力表が一次資料**（`docs/vaak/16-形式構文.md`） |
| `src/check.rs` | 名前・領域・脱出段の検査 |
| `src/types.rs` | 型検査 |
| `src/interp.rs` | **参照実装** |
| `src/vm.rs` | バイトコード VM |
| `src/steel.rs` | LLVM IR（STEEL vaak） |
| `src/host.rs` | ホスト界面（C-95） |
| `src/lsp.rs` `src/json.rs` | 言語サーバ。**依存を増やさない**（JSON も自前） |
| `src/portable.rs` | WASM の入口（S-13） |
| `editors/` | Zed と VS Code。**鍵語の一覧は一つ**（`tests/grammar.rs` が見張る） |

## 建て方

```bash
cargo test --release          # 全部（289 通過）
cargo build --release         # vaak / vaak-lsp / steel / portable
./target/release/steel examples/vaak/01-探索.vaak   # LLVM IR 経由で実行ファイル
```

**STEEL は `clang` を要る。** 無ければ `--emit-ir` で IR だけ出せる。
