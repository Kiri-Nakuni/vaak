# 作業の分担

**この版方は二人以上の担い手で進めている。** 衝突しないように、まず担当と意味論の
所有者を確かめること。

| | 担当 | 触る場所 |
|---|---|---|
| **Claude** | Vaak の言語意味論、決定記録、STEEL | `src/` `docs/` `examples/` `editors/` |
| **Codex** | PraTeX 埋め込みの additive API | `src/embedding.rs`、対応する試験と引き継ぎ文書。必要な支援変更だけ |
| **Codex** | [PraTeX](https://git.trap.jp/Suima/vaak-rtex)（別版方） | PraTeX の中だけ |

Codex の枝は **`codex2/pratex-embedding-api`**（基点 `codex2/main`）。以後も
`codex2/` 以下で作業する。Claude の枝と優先順位をこの枝から書き換えない。

PraTeX 埋め込みのための追加 API は進めてよい。ただし、既存の Vaak プログラムの意味、
参照実装・VM・STEEL の結果、C-n/S-n の解釈を変える必要が出たら実装を止め、Claude に
判断を求める。合意前に新しい S-n を作らない。

## PraTeX 側から来る依頼

`src/vaak.rs`（PraTeX 側）が Vaak の API を使っている。
**API を変えたら PraTeX 側に知らせること。**

**S-11** の呼べるホスト名（`HostItem::Fn` / `HostFn`）と、**S-15/S-22** の部分読み書き・
実行時エラー時の writeback は実装済みである。今回必要なのは、parse/check/type-check/compile
を一度だけ行い、順序と型を固定した host layout と再利用可能な `Runner` を一つの公開契約で
包むこと。`tex.print` の意味は Vaak に埋め込まず、PraTeX が host function として供給する。

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
- `cargo test --release --locked --no-fail-fast` が全部通ること（変更前 baseline は
  **612 passed、0 failed**、2026-08-23）。prepared embedding checkpoint は同日
  **679 passed、0 failed**。

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

### Codex の PraTeX 埋め込み枝

1. `HostLayout` を順序・名前・型まで含む不変な descriptor にする。
2. `PreparedProgram` が parse/check/type-check/VM compile を一度だけ行った結果を所有する。
3. `EmbeddingRunner` が既存 `vm::Runner` を再利用し、host 値の個数・型と host function の
   layout 不一致を実行前に拒否する。
4. 参照実装と既存 VM API の意味を変えない回帰試験を置く。
5. Vaak 側を独立 commit・push してから、PraTeX 側を別 commit で新 API へ移す。

この slice に named entry point、中断・再開、phase ABI、WASM ABI、`tex.print` の TeX 意味論は
入れない。それらは host layout の上に後から足す。

### Claude の意味論・STEEL 枝

進行中の優先順位は `claude_memo.md` と `docs/vaak/decisions.md` を一次資料とする。Codex の
埋め込み枝から STEEL の残件や言語仕様を先回りして変えない。

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
cargo test --release --locked --no-fail-fast   # 全部（prepared embedding checkpoint 679通過）
cargo build --release         # vaak / vaak-lsp / steel / portable
./target/release/steel examples/vaak/01-探索.vaak   # LLVM IR 経由で実行ファイル
```

**STEEL は `clang` を要る。** 無ければ `--emit-ir` で IR だけ出せる。
