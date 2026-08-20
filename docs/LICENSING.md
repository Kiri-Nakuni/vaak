# 権利関係

## この版方（リポジトリ）は MIT

**権利者：有村陽大 (Arimura Akihiro)。** [LICENSE](../LICENSE) にある。

スタンドアロンの Vaak——本体・言語サーバ・Zed 拡張・STEEL・Portable——は
すべてこれに含まれる。

## rtex は **GPL-3.0** である。向きが決まっている

**確認した。** `~/Documents/rtex/LICENSE` は GNU General Public License v3 の全文である。
著作者は tyti 氏および Nakuni Kiri 氏。

これは**一方通行**を意味する。

| | 可否 | なぜ |
|---|---|---|
| **Vaak (MIT) → rtex (GPLv3)** | **できる** | MIT は GPL と両立する。rtex に組み込んだ全体は GPLv3 として配る |
| **rtex (GPLv3) → Vaak (MIT)** | **できない** | 取り込んだ時点で Vaak 全体が GPLv3 になる |

### したがって守るべき規律

> **rtex のコードを一行もこちらへ写さない。**

`\directvaak` / `\vaakdef` の実装（`rtex/src/vaak.rs`）は**あちら側にある**。
こちらにあるのはホスト界面（C-95）だけで、**rtex を知らない。**
この分離は設計上の都合であると同時に、**権利上の要請でもある。**

同じ理由で、rtex の枝で得た知見をこちらへ持ち込むときは、
**測定結果と設計判断だけを運び、コードは運ばない。**

## e-upTeX について——**取り込む前に読むこと**

**「e-upTeX は BSD だから大丈夫」と判断してはいけない。**

CTAN の `uptex` 項の license は `Free license not otherwise listed` である。
本体は TeX Live の `Build/source/texk/web2c/uptexdir` にあり、
系譜は **pTeX → upTeX → e-upTeX**（e-TeX を合流させたのは北川弘典氏）。

| | 権利 |
|---|---|
| `uptexdir` 本体 | **一括りにできない。** pTeX 由来・upTeX 由来・e-TeX 由来が混ざる |
| `ptexdir` の `COPYRIGHT` | ASCII MEDIA WORKS ＋ Japanese TeX Development Community。**独自の再配布条項** |
| `texjporg/uptex-base` | **BSD-3-Clause**（format・文書・見本であって本体ではない） |

### 結論：**コードを移植せず、仕様から書き直す**

rtex 自身が「TeX82 を Rust で書き直したもの」であって tex.web の翻訳ではない。
**同じやり方を踏襲する**——`uptexdir` のソースではなく、
upTeX / e-TeX の**振る舞いの記述**を見て書く。

これは権利の問題を避けるためだけではない。
**rtex の中で一貫した実装になる**からでもある。

### 派生物の権利

**e-upTeX を足した rtex は GPLv3 のままである。** 枝を切るときもそれで通す。

## その他

`legacy/reod-tier1` は前身であり、同じ権利者のものである。
