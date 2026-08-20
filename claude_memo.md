# 作業の逐次記録

**最終更新：2026-08-21。** 新しいものが上。

## いまの枝

| 版方 | 枝 | 状態 |
|---|---|---|
| `~/Documents/mydsl` | `speculative` | **ここで作業中。** `main` は古い |
| `~/Documents/rtex` | `vaak` | `\directvaak` / `\vaakdef` まで。**GPLv3** |

## 依頼の一覧と状態

| # | もの | 状態 |
|---|---|---|
| 1 | 決定への移行（C-95 ホスト界面 / C-96 ホスト名の可視範囲） | **済** |
| 2 | LSP（高優先度） | **済**。`src/lsp.rs` `src/json.rs` `src/bin/vaak-lsp.rs` |
| 3 | Zed 拡張に LSP を繋ぐ | **済**。`editors/zed/`。文法定義は持たない |
| 4 | LICENSE（MIT / 有村陽大） | **済**。`LICENSE` `docs/LICENSING.md` |
| 5 | **STEEL vaak**（LLVM IR スタンドアロン） | **第一段完了**（S-12）。整数・制御・関数。枝 `steel`、`speculative` に併合済み |
| 6 | e-upTeX 移植可否 | **済**。`rtex/docs/euptex-port-notes.md` に八段の段取り |
| 7 | e-upTeX 移植 | 未 |
| 8 | 寸法の鍵語 H / Q / zw / zh | **済**。rtex 枝 `jdimen`、試験 7 本 |
| 9 | LaTeX2e が動くか（7 が済んだら） | 未 |
| 10 | LuaTeX のコールバック再現（意味論を変えずに） | 方針は **S-11**。実装は凍結解除待ち |
| 11 | pdfTeX を参考に PDF 直接出力（OTF は後回し、HarfBuzz 不要） | 未 |
| 12 | Portable vaak（WASM） | **済**（S-13）。WASI と素の WASM の二つ。枝 `portable` |
| 13 | rtex の名前空間を掴んで枝を切る | **飛ばした**。`main` は 2 コミットで名前空間の作業が無い |
| 14 | rtex vaak の差し込み範囲を増やす＋ほぼゼロ費用か測る | 未 |
| 15 | 人間向けリファレンス＋付録（最低優先度） | 未 |
| 16 | 別 CLAUDE からの設計質問二つに答える | 未 |

## 記録

### 2026-08-21

- **S-13 / Portable vaak**：WASI 版と素の WASM 版。`src/portable.rs` `src/bin/portable.rs`
  - 素の側は C の呼び出し規約だけ（`wasm-bindgen` を入れない）
  - **入口を作ったら食い違いが出た**：`if (0)` を型検査器が通し評価器が落としていた。
    落とす側に寄せた（`u1` は真偽であって数ではない）
- **rtex 和文寸法**：`Q` `H`（0.25mm ちょうど）と `zw` `zh`（いまは `em` で代用）。試験 7 本
- **rtex e-upTeX 評価**：八段の段取り。段 1〜3 で e-TeX 相当 → LaTeX2e を試せる。
  段 8（縦組）だけ別格。**コードは移植せず仕様から書き直す**
- **rtex の名前空間**：`main` は 2 コミットしかなく、その作業は入っていない。飛ばした

- **S-12 STEEL vaak**：LLVM IR を吐くようにした。`src/steel.rs` `src/bin/steel.rs`
  - 値は `(i1 ok, iN v)`。`-O2` で `i1` が消える——**paradox の費用は残らない**
  - `fib(30)`：参照 5178ms → **3ms**
  - 差分試験 34 本。**`! 0` の食い違いを見つけた**（参照はビット反転。STEEL を直した）
  - まだ扱わない：配列・写像・構造体・`str`・浮動小数・`alias`・`flow`・
    `outward`・被演算子つき `continue`・`new`・`|>`・メンバ関数・ホスト界面
  - 使い方：`steel prog.vaak` → 実行ファイル、`--emit-ir` で IR

- **C-96**：ホストが見せる名前は関数の中からは見えない。
  理由は C-87（`alias` 引数は同じセルに二つ届かない）が破れるから。
  検査器が理由を言うようにした。試験 197 通過
- **C-95**：ホスト界面を決定に移した。S-4 / S-6 を吸収。
  `трейт` というキリル文字の誤記を直した。契約 4 を言い切りに
- **LICENSE**：Vaak は MIT（有村陽大）。**rtex は GPL-3.0 だった**——
  向きが一方通行になる。`docs/LICENSING.md` に規律として書いた
- **e-upTeX の権利**：`uptexdir` は pTeX/upTeX/e-TeX 由来が混ざる。
  `ptexdir` の COPYRIGHT は独自条項。BSD-3-Clause なのは `uptex-base` だけ。
  **コードを移植せず仕様から書き直す**方針
- **Zed 拡張**：tree-sitter 文法を持たない。色分けは意味トークンから
- **LSP**：依存ゼロ。JSON も自前（代用対の組み直しまで）

### それ以前（性能）

- rtex の呼び出し **1340 → 570 ns**（100000 回、`\count0=42` の土台を引いた値）
  - `\vaakdef` で TeX 側 960 → 120 ns（`\def` の展開と同じ。これ以上下がらない）
  - `Value` 72 → 16 バイト（S-9）。加算一回 92 → 61 ns
  - 控えの場所の使い回し（`[i32;256]` を二つ、毎回 4 KB 写していた）
  - `Runner`（場・積み・枠を持ち続ける）
- 参考：`\number\count1` が 260 ns、`\def\z{42}` の展開が 70 ns。
  **展開する形の床はホストが決める**
