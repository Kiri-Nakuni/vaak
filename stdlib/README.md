# Vaak文字列ライブラリ

`string.vaak`は、既存の`str`と配列操作だけで書いた任意選択のライブラリである。
中核の無名標準ライブラリ（S-3）を変更せず、`str_*`自由関数として提供する。

Vaakにはまだmodule/import機構がない。埋め込むホストは
`vaak::stdlib::STRING`を利用者ソースの前へ置いてからparse/check/compileする。

```rust
let app = r#"
    let input := "  Apple,banana,APPLE  ";
    let trimmed := str_trim_ascii(input) ?? "";
    let fields := str_split(trimmed, ",") ?? new str array(0, "");
    str_join(fields, " | ")
"#;

let source = format!("{}\n{}", vaak::stdlib::STRING, app);
let program = vaak::parser::parse(&source)?;
```

動くVaak側の例は[`examples/文字列.vaak`](examples/文字列.vaak)にある。単独の
プログラムではなく、`string.vaak`を前置きして使う利用者ソースである。

## API

入力本体は`alias`で受け、走査前の深い複製を避ける。needle、separator、
replacementは値で受ける。これは小さい文字列を一度複製する代わりに、文字列
リテラルを直接渡せ、`str_find(source, source)`もC-87へ抵触せず書ける設計である。
返される文字列・配列は新しい所有値である。

| 関数 | 結果 |
|---|---|
| `str_eq(source, other)` | バイト列が同じか |
| `str_starts_with(source, needle)` | 前方一致 |
| `str_ends_with(source, needle)` | 後方一致 |
| `str_contains(source, needle)` | 部分列を含むか |
| `str_find(source, needle)` | 最初のバイト位置。無ければparadox |
| `str_find_from(source, needle, from)` | `from`以降の最初の位置 |
| `str_find_byte(source, byte, from)` | `from`以降の一バイト検索 |
| `str_rfind(source, needle)` | 最後の位置。無ければparadox |
| `str_rfind_from(source, needle, from)` | `from`以下の最後の位置 |
| `str_slice(source, from, upto)` | バイト半開区間`[from, upto)` |
| `str_trim_ascii(source)` | 両端のASCII空白を除く |
| `str_split(source, separator)` | 空欄と末尾の空欄を保持して分割 |
| `str_join(parts, separator)` | 文字列配列を連結 |
| `str_replace_all(source, old, replacement)` | 左から右への非重複置換 |
| `str_repeat(source, count)` | `count`回連結 |
| `str_ascii_lowercase(source)` | ASCII大文字だけを小文字化 |
| `str_ascii_uppercase(source)` | ASCII小文字だけを大文字化 |
| `str_eq_ignore_ascii_case(source, other)` | ASCIIだけ大小を無視して比較 |
| `str_utf8_valid(source)` | RFC 3629の妥当性検査 |
| `str_is_utf8_boundary(source, index)` | 妥当なUTF-8の符号位置境界か |
| `str_slice_utf8(source, from, upto)` | UTF-8妥当性と両境界を検査してslice |

## 端の契約

### 空needle

- `starts_with`、`ends_with`、`contains`は`true`。
- `find`は`0`、`find_from`は`from`を返す。
- `rfind`は`source.len()`、`rfind_from`は`from`を返す。
- `split`の空separatorと`replace_all`の空oldは、零幅一致をどのように
  列挙するかを暗黙に決めないためparadox。

### 添字と回数

- 位置はすべてバイト添字。
- `find_from`と`rfind_from`の`from`は`0..=source.len()`だけが有効。
  範囲外と負数はparadox。
- `find_byte`の`from`も同じ範囲だけが有効。見つからなければparadox。
- `rfind_from`の`from`は検索開始位置を含む。needleが空でなければ、末尾を
  越える有効な位置は最後の候補位置へ丸める。
- `slice`は`0 <= from <= upto <= len`だけが有効。それ以外はparadox。
- `repeat`の負回数はparadox。0回と空のsourceは空文字列。非空sourceでは
  `count * source.len()`を`i64`で表せなければ、周回を始める前にparadox。

### UTF-8

`str`の表現と通常APIはバイト単位である。検索・分割・置換・大小変換は
Unicodeの正規化や大小対応を行わない。ASCII大小変換は128以上のバイトへ
触れないため、妥当なUTF-8入力を壊さない。

`str_slice`は指定されたバイトを忠実に複製するため、符号位置の途中を切れば
結果は不正なUTF-8になり得る。この費用と判断を隠さない。妥当性を保つ必要が
あるときは`str_slice_utf8`を使う。この関数は入力全体を検査するためO(n)である。

## 実装上の性質

- 検索は素朴な照合で、最悪O(source × needle)。巨大な検索には将来、前処理済み
  matcherまたは外部WASMを使う余地がある。
- CSV/JSON lexerの区切りには`str_find_byte`を使える。これは一バイトのneedleを
  作らず、候補位置ごとの`str__match_at`関数frameも持たない。
- 出力を作る操作は`.push()`で構築する。現行の幾何増加により償却線形だが、
  `reserve`が加われば既知長の`repeat`や`join`をさらに調整できる。
- `str_utf8_valid`は過長符号化、UTF-16 surrogate範囲、U+10FFFF超過を拒む。
- 第一級callback、sum型、match構文は必要としない。
- 参照実装、VM、STEEL nativeの同一ソース試験は`tests/string_library.rs`にある。
