# 作業の逐次記録

**最終更新：2026-08-21。** 新しいものが上。

## いまの枝

| 版方 | 枝 | 状態 |
|---|---|---|
| `~/Documents/mydsl` | `speculative` | **ここで作業中。** `main` は古い |
| `~/Documents/rtex` | `vaak` | `\directvaak` / `\vaakdef` まで。**GPLv3** |

## いまの担当（2026-08-21 に入れ替えた）

| | 担当 | 枝 |
|---|---|---|
| **Claude（自分）** | **Vaak（この版方）** | `speculative` |
| Codex | rtex（別版方） | `etex-latex` |

**rtex には触らない。** `AGENTS.md` に両方の分担が書いてある。
リモートは `origin`（`git.trap.jp`）。**枝を進めたら push すること。**

## 依頼された順（これを上から）

1. **S-11：ホストが呼べる名前も見せられる** ← **いまここ。rtex が待っている**
2. **STEEL 第四段**。**`f80` を足すこと**（LLVM の `x86_fp80`）
3. その他（下の一覧）

## 依頼の一覧と状態

| # | もの | 状態 |
|---|---|---|
| 1 | 決定への移行（C-95 / C-96 / C-97） | **済** |
| 2 | LSP | **済** |
| 3 | Zed 拡張（tree-sitter 文法つき） | **済** |
| 3b | VS Code 拡張 | **済** |
| 4 | LICENSE（MIT / 有村陽大。rtex は tyti 氏） | **済** |
| 5 | STEEL vaak | **第一段・第二段 済**（S-12 / S-17 / S-18）。**C の 1.12×** |
| 5b | STEEL 第三段：集合体 | **済**（S-19）。配列・`str`・深い複製・`alias`・場の解放 |
| 5d | STEEL：写像・構造体・入れ子の集合体・ループ本体の解放 | 未 |
| 5c | STEEL：`outward`・動く段数の `$repeat`・ホスト界面 | 未 |
| 6 | e-upTeX 移植可否 | **済**（八段の段取り） |
| 7 | e-upTeX 移植 | 段 0・1a 済 ＋ **e-TeX の大半**（`\protected` `\ifdefined` `\ifcsname` `\unless` 問い合わせ群） |
| **9** | **LaTeX2e が動くか** | **進行中。** latex.ltx が 115 → 1148 行目まで。次は pdfTeX の実用命令 |
| 10 | LuaTeX のコールバック再現 | 方針は **S-11**。未着手 |
| 10b | **S-15 の穴埋め：`read_at` / `write_at`** | 未。動く添字が 1340 ns |
| 10c | **`aakdef` の逐語読み**（balanced text 一級対応） | 未 |
| 11 | pdfTeX を参考に PDF 直接出力 | 未 |
| 12 | Portable vaak（WASM） | **済**（S-13） |
| 13 | rtex の名前空間 | **Phase 0〜7 済**（枝 `namespace` → `full`） |
| 13b | 名前空間 Phase 8（TRIP・アラインメント） | 未 |
| 14 | **rtex vaak の差し込み範囲＋ほぼゼロ費用か測る** | 未 |
| 15 | 人間向けリファレンス＋付録 | 未（最低優先度） |
| 16 | 設計質問二つ | **済**（S-14） |

## 記録

### 2026-08-21（続き 5）

- **担当を入れ替えた。** Codex が rtex（pdfTeX・e-upTeX・kpathsea が重い）、
  Claude が Vaak。`AGENTS.md` を両方に置いて push した
- 引き継ぐ前に rtex を一段進めた：`\expanded` `\detokenize` `\unexpanded` と
  **引用符つきファイル名**（`\openin\@inputcheck"expl3.ltx" `——
  これが無いと `\IfFileExists` が必ず偽になる）。
  `latex.ltx` は **expl3-code.tex の 7866 行目**まで。rtex 155 通過
- **f80 の依頼**：STEEL に `x86_fp80` を足す。
  プローブは「ホスト方言は基底型を足せる（例: 31/63bit 整数、f80）」と言っている——
  **STEEL は方言である**という形で入れるのが筋。
  ただし **Rust に f80 が無いので、木を辿る参照実装と差分試験できない。**
  そこをどう扱うか決めること（`S-n` に書く）

### 2026-08-21（続き 4）

- **STEEL 第三段（S-19）**：記憶模型は C-90 の表がそのまま設計だった。
  バンプ確保器＋領域を出るとき外へ出る一つの値を印の下へ写す（C-14 が一つに縛る）。
  配列・`str`・深い複製・`alias`・`.len()`・添字の読み書き。**289 通過**
  - 限界：関数の中のループが場を伸ばす。尽きたら終了コード 70
  - 範囲外への**書き込み**は誤りにした（代入は元々 paradox を産むので区別がつかない）
- **rtex e-TeX**：latex.ltx が実際に使う命令を数えてから入れた。
  `\protected`（81 箇所）`\ifdefined` `\ifcsname` `\unless` `\eTeXversion`
  `\currentgroup*` `\currentif*` `\lastnodetype` `\tracing*`。**149 通過**
  - **latex.ltx は 115 行目 → 1148 行目まで進んだ**
  - **重要**：現代の LaTeX2e は素の e-TeX では動かない。
    `\pdffilesize` / `\filesize` / `\luatexversion` / `\kanjiskip` のどれかが要る。
    **次は pdfTeX の実用命令**（文字列・ハッシュ・乱数。組版に触らない）

### 2026-08-21（続き 3）

- **rtex 名前空間 Phase 6（`\usingnamespace`）** と枝 **`full`**（Vaak ＋ 名前空間）
  - 途中で二つ誤りを見つけた：一文字の制御綴が探索から漏れていた／
    `*lib\~` と `*lib~` が鍵で衝突していた
  - **入れ子を許した**（一度「Nested は誤り」を入れて撤回）
- **STEEL 第二段**：浮動小数・`E -> T`・`|>`・`flow`/`$repeat`
  - **S-17**：`|>` が検査器で糖衣として扱われていなかった（C-15 違反）
  - **S-18**：整数リテラルは浮動小数を名乗らない
- **`\directlua` を実測**（TeX Live 2026）。改行喪失・`--` が全部食う・
  `%` で Runaway・`#`・活性文字。**LuaTeX は知っていて直さないと決めている**
  （`⟨general text⟩` であることが機能。展開可能なので逐語は原理的に不可能）
- **`\vaakinput 名前.vaak`** を足した。字句器を通らない。試験 10 本

### 2026-08-21（続き 2）

**依頼者の指示した順**：S-16 → 名前空間 → STEEL 二段 → rTeX のチューニング。

- **S-16 を直した。** `if` の分岐は領域（C-20）なので空になれば paradox（C-14 規則2）。
  VM は `else` の無い側にだけ積んでいた。合流点で高さが揃わず `;` が下の値を食っていた。
  `RegionBegin`/`EndRegion` で挟むだけ。`switch` の腕も同じく挟んだ。**251 通過、保留 0**
- **rtex 名前空間 Phase 0〜3**（枝 `namespace`、107 通過）
  - Phase 0：catcode 16
  - Phase 1：鍵を `(Option<NamespaceId>, Vec<u8>)` に。**`levels.rs` は無改変**——
    同じ番号空間に載せるだけで群も `\global` も付いてくる（試験で確認）
  - Phase 2：`scan_control_sequence` を「範囲を返す」形に割り、`^^` 置換を共有。
    `LexError` を新設（runaway を運ぶため）
  - Phase 3：`\namespace`。`get_x_token` は使えない——
    終わりを知らせるのが `\csname` 自身だから。**global に作られないことを試験で確認**
- **STEEL を C / Rust と比べた。** fib(35)：C 24.0 / Rust 25.0 / **STEEL 27.0 ms**

### 2026-08-21（続き）

- **C-97**：`true` / `false` / `bool` / `E -> T`。
  `->` は **C-30 で決まっていたのに構文解析器が知らなかった**
- **例を書いた**（`examples/vaak/`、5 本＋ README＋試験）。
  **例が VM のバグを二つ見つけた**
  - **直した**：`1 + (2)` が落ちていた。括弧の領域が自分の底を持っていなかった
    （`Op::RegionBegin` を足した）
  - **未解決（S-16）**：`$repeat` の動的な段数＋`??`＋`+=` の三つが揃うと落ちる。
    `tests/examples.rs` に `#[ignore]` で残してある
- **S-15**：C-95 の費用を測った。**撤回しない**——界面は 22ns/添字で VM の四分の一。
  動く添字だけが高い（512 要素で 1340ns）。塞ぐなら `read_at` / `write_at`
- **Zed**：`vaak-lsp` が見つからない件を直した（WASI の砂場で `std::fs` が効かない）。
  `cargo install --path .` が要る。**ただし色分けはまだ出ないかもしれない**——
  Zed が LSP の意味トークンを使わない可能性があり、tree-sitter 文法が要るかもしれない
- **rtex**：`VAAK_DEBUG=1` で静的エラーの中身が標準エラーに出るようにした
- **名前空間**：`.claude/worktrees/review-latest-repo-changes-3b7687/NAMESPACE_ROADMAP.md`
  に**詳細な設計がある**（Phase 0〜8）。実装差分は入っていない（作業木は綺麗）
- **権利**：rtex は tyti 氏に全部帰属（依頼者の寄与は本人が無いものと認めた）

### 2026-08-21（続き）

- **STEEL を C / Rust と比べた。** fib(35)：C 24.0ms / Rust 25.0ms / **STEEL 27.0ms**。
  生成された機械語がほぼ同じ。**`i1` 由来の命令が 0**——paradox の対が消えている。
  実行ファイルは 15,880 バイト（C が 16,040、Rust が 3.9MB）
- **VS Code 拡張**（`editors/vscode/`）と **Zed の tree-sitter 文法**。
  Zed は LSP の意味トークンで色を付けないので文法が要った。
  **鍵語の一覧は一つ**——`tests/grammar.rs` が三箇所を突き合わせる
- **C-97**：`true` / `false` / `bool` / **`E -> T`**（C-30 で決まっていたのに未実装だった）
- **例を五本**（`examples/vaak/`）。**例が VM のバグを二つ見つけた**:
  - `1 + (2)` が落ちていた（括弧の領域が自分の底を持っていなかった）→ `RegionBegin` で直した
  - **S-16 は残っている**
- **S-15**：C-95 の費用は添字一つあたり 22 ns。**撤回しない。**
  動く添字だけが 1340 ns で、塞ぐなら `read_at` / `write_at`

### 2026-08-21

- **rtex e-TeX 段 1a**：`\numexpr` `\dimexpr` `\glueexpr` `\muexpr`。枝 `etex-expr`
  - 掛けと割りを溜めてから一度に行う——**中間結果を 32 ビットに落とさない**
  - `\numexpr 7*8/3\relax` = 19（`\multiply\divide` を並べると 18）
  - 内部量として実装（`InternalCommand::Expr`）。値が要る場所ならどこでも
  - `\dimexpr 4Q*2\relax` が通る——段 0 の和文単位と組み合わさる
  - 試験 12 本。rtex 全体で 84 通過

- **S-14**：設計質問二つに回答。
  - 剥がされたホスト別名の検出責任 → **問い自体が消えていた。**
    C-95 が写しを渡す形にしたので危険が構造的に無い。
    プローブの paradox 発生源から「剥がされたホスト別名」を消した
  - 基底型の追加費用 → **効くのは数ではなく幅。**
    8 バイトに収まればいくつ足しても無料。超えれば `Box` に入る

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
