# stdlib source の明示依存

Vaak にはまだ module/import 構文がないため、任意選択ライブラリは必要な source を
利用者 program の前へ並べて使う。`scripts/resolve-stdlib.py` は source 冒頭の comment に
書かれた依存だけを読み、依存を先にした決定的な順序を返す。Vaak の本文や識別子から
暗黙の依存を推測しない。

## header 契約

新しい source は先頭の `%{ ... }%` comment に次の一行を置く。

```text
依存: stdlib/string.vaak, stdlib/codec/json_utf8.vaak
```

- 名前は `stdlib/` からの相対pathで、接頭辞 `stdlib/` は付けても省いてもよい。
- 依存が無ければ欄を省くか `依存: なし` と書く。
- 同じ依存の重複、二つ以上の依存欄、絶対path、`..`、`.vaak` 以外は誤りである。
- 既存の `前置き依存:` と直後の `- path.vaak` 箇条書きも同じ契約として読む。
- 冒頭commentの後に現れる記述は依存欄として読まない。

例えば JSON Lines の source は次の順序になる。

```text
string.vaak
codec/json_utf8.vaak
codec/jsonl_utf8.vaak
```

## 解決規則

解決対象とその推移的依存だけを DAG として扱う。同時に置ける source が複数あれば、
UTF-8 byte 列として小さい相対pathを先にする。この tie-break は filesystem の列挙順や
locale に依存しない。存在しない target・依存、cycle、重複 target は成功順序を返さない。

```bash
python3 scripts/resolve-stdlib.py array/i64/compress.vaak
python3 scripts/resolve-stdlib.py --json codec/jsonl_utf8.vaak
python3 scripts/resolve-stdlib.py --concat codec/jsonl_utf8.vaak > program-prefix.vaak
python3 scripts/resolve-stdlib.py                 # 全source
```

通常出力は一行一source、`--json` は同じ順序の JSON array、`--concat` は各source末尾を
一改行へ正規化して source 間へ空行を一つ置く。診断は `[E123]` の安定codeを標準errorへ
出し、終了status 2を返す。

## 検証

```bash
python3 scripts/test_resolve_stdlib.py
python3 scripts/resolve-stdlib.py --root stdlib
```

試験fixtureは missing dependency、cycle、重複、tie-break、旧header、
`STRING -> JSON_UTF8 -> JSONL_UTF8` を固定する。
