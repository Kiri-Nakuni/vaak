# Vaak portable bytecode v0 独立設計案

> **状態：設計 probe。決定でも実装済み仕様でもない。**
> `docs/vaak/decisions.md` に新しい `S-n` を足さず、木を辿る参照実装の意味を
> portable artifact へどう保つかだけを調べる。以下の `MUST` / `SHOULD` は、
> **この案を採る場合の wire 契約**を表し、Vaak の言語意味論を新しく決める語ではない。

この文書は Rust 内部の `Program2` を保存する案ではない。Rust VM と、将来の pure C# / Java VM、
さらに Unity/.NET や Lua 等を host に使う adapter が、同じ検証済み artifact と同じ host schema を
共有するための独立した portable bytecode v0 を定義する。

意味の基準は常に [`src/interp.rs`](../src/interp.rs) の木を辿る実装である（S-5）。
Rust VM、C# VM、Java VM のどれかが参照実装と違えば、参照実装の結果が勝つ。
この文書は TeX、Unity、Lua 等の host 固有の意味を Vaak へ入れない。

## 1. 変えてはならない境界

1. **内部 layout を wire にしない。** `usize`、pointer、Rust enum の判別値、`Vec` / `Box` /
   `HashMap` / allocator / arena の番地、C# object reference、Java reference、Lua stack index、
   `lua_State*`、Lua GC pointer を一 byte も artifact や wire value に出さない。
2. **検証前に実行しない。** decoder と構造 verifier が artifact 全体を受理して immutable な
   `VerifiedProgramV0` を作るまで、host value を読まず、host function を呼ばず、VM の cell を作らない。
3. **意味を二度決めない。** 算術、paradox、深い複製、alias、collection の順序は参照実装と
   共通の conformance corpus から決める。portable VM 用の「似た意味」を足さない。
4. **host は snapshot で入る。** alias は値でなく束縛の形態（C-48）であり、wire value に alias tag を
   持たせない。host の native memory や object を直接指さない。
5. **既定で再入しない。** v0 の host call は同期 leaf call である。Vaak → Lua → Vaak、
   Vaak → C# → Java → Vaak のような相互 callback cycle は既定で拒否する。
6. **誤りより前を巻き戻さない。** C-2 / S-22 に従い、実行時エラーまでに変わった host binding の
   after-state / patch を error と一緒に返す。command buffer を成功後だけ commit する方式は、
   host 作用をまだ起こしていない別 protocol として分ける。
7. **位置を落とさない。** paradox は発生 span を運び、消費されず値を要求された場所ではなく
   発生点を primary diagnostic とする。S-16 のように後段で `Span::NONE` へ落ちる実装を許さない。

根拠となる既存決定は C-14（paradox）、C-20（束縛と access）、C-48（alias）、C-87（alias root）、
C-90（arena）、C-95（host snapshot）、S-5（参照実装）、S-8 / S-15 / S-22（部分 read/writeback）、
S-11（host function と再入）、S-16（領域と誤り位置）、S-21（可変 alias）である。
一次記録は [`docs/vaak/decisions.md`](vaak/decisions.md) にある。

## 2. 現在の Rust 内部表現をそのまま保存できない理由

| 内部表現 | portable wire にできない理由 | v0 の置換 |
|---|---|---|
| `vm::Op` | Rust enum の discriminant・padding・variant layout は契約でない | 固定 `u8` opcode と opcode ごとの固定 operand schema |
| `Program2` | `Vec`、複数の `HashMap`、Rust の登録順・hash seed、private compiler stateを含む | section directory と type/constant/function/code table |
| `Value` | `Box<Vec<_>>`、`BTreeMap`、`HashMap`、16-byte という現在の最適化に依存 | 型付き canonical `WireValueV0` |
| `CellId` / `Place` | arena 番号と Rust object graph に依存 | verifier 内だけの abstract cell/place。artifact は slot と path opcode を持つ |
| `Span { start, end }` | 単一 source しか識別できない | `SourceId + byte start/end` の span table |
| `HostLayout` | Rust の `String` / `ValueType` と `Arc` identity に依存 | 順序を固定した host schema と SHA-256 fingerprint |
| `RtErr { String, Span }` | 文言と allocation が runtime 固有 | stable error domain/code、span、引数、call provenance |
| `usize` の長さ・添字 | 32/64 bit process で幅が変わる | wire は `u16/u32/u64` の決めた幅だけ |

portable decoder は一度 wire を検証済みの host-owned IR へ写してよい。Rust は `Vec`、C# は array、
Java は primitive array、Lua adapter は Lua table を内部で使ってよいが、それらは wire 契約ではない。

## 3. version を三つに分ける

| version | 守るもの | 変更例 |
|---|---|---|
| **format version** | header、section directory、整数幅、record encoding | section record の形、offset 幅 |
| **core ABI version** | opcode、stack effect、値、paradox、alias、host call の意味 | opcode の意味、wire value の意味 |
| **adapter schema version** | snapshot / patch / command / host error envelope | partial array snapshot、command schema |

compiler version は provenance と cache key には入れるが、互換判定の代わりにしない。同じ compiler version
でも ABI が違えば拒否し、異なる compiler version でも同じ canonical format/ABI を出せる。

v0 は `format 0.0`、`core ABI 0.0`、`host-schema 0.0`、`snapshot/patch 0.0` と表す。
major が違えば拒否する。minor が新しい artifact は、未知の required feature、critical section、opcode が
一つもなく、既知 section の reserved field が全て 0 の場合だけ受理できる。
既存 section の record schema を伸ばす変更は minor では行わず、新しい non-critical section に分ける。

## 4. scalar encoding と header

### 4.1 scalar

- 整数と IEEE 754 bit pattern は **little endian の固定幅**。varint、native endian、native `bool` を使わない。
- wire scalar は `u8/u16/u32/u64/i32/i64/f32bits/f64bits` のみ。
- 真偽 field は `u8` の `0` または `1`。それ以外を拒否する。
- offset、length、ID は特記しない限り `u32`。`0xffff_ffff` だけを `NO_ID` / `NO_TARGET` に予約する。
- string record は `byte_length:u32 + UTF-8 bytes`。NUL 終端でなく、Unicode normalization をしない。
  Vaak identifier の code point 列を勝手に NFC/NFD へ変えない。
- padding と reserved field は全て 0。non-zero を拒否する。

固定幅を選ぶのは、C#/Java/Lua adapter ごとに LEB128 実装差や overlong encoding を持ち込まず、
canonical byte 列を一意にするためである。容量は resource profile で先に制限する。

### 4.2 32-byte header

| offset | 幅 | field | v0 |
|---:|---:|---|---|
| 0 | 8 | magic | `56 41 41 4b 42 43 00 00` (`VAAKBC\0\0`) |
| 8 | 2 | format major | `0` |
| 10 | 2 | format minor | `0` |
| 12 | 2 | core ABI major | `0` |
| 14 | 2 | core ABI minor | `0` |
| 16 | 4 | header flags | v0 は `0` |
| 20 | 4 | exact file length | 実 byte length と一致 |
| 24 | 4 | section directory offset | 4-byte aligned |
| 28 | 2 | section count | v0 ceiling 以下 |
| 30 | 2 | header size | `32` |

header 自身に checksum は置かない。配布 manifest が artifact 全 byte の SHA-256 と size を持つ。
壊れた入力は checksum の前後どちらでも構造 verifier が拒否する。

### 4.3 section directory

一 record は 20 byte である。

```text
kind:u16 flags:u16 offset:u32 length:u32 item_count:u32 reserved:u32
```

- directory は `kind` 昇順。v0 の section は重複不可。
- payload は 4-byte aligned、区間は重ならず、directory 自身とも重ならない。
- section 間 padding は 0。最後の section 後に余分な byte を置かない。
- `flags & 1` は `CRITICAL`。未知 critical section は拒否、未知 non-critical section は範囲だけ検査して飛ばす。
- 受理した未知 non-critical section は core VM から見えない opaque bytes として保持する。旧 reader が
  canonicality を判定したふりをせず、decode→encode では directory record と payload をそのまま保存する。
- v0 emitter は下表の順で一つずつ出す。

| kind | section | 必須 |
|---:|---|:---:|
| `0x0001` | `FEATURES` | ○ |
| `0x0002` | `STRINGS` | ○ |
| `0x0003` | `SOURCES` | ○ |
| `0x0004` | `SPANS` | ○ |
| `0x0005` | `TYPES` | ○ |
| `0x0006` | `CONSTANTS` | ○ |
| `0x0007` | `FUNCTIONS` | ○ |
| `0x0008` | `CODE` | ○ |
| `0x0009` | `HOST_SCHEMA` | ○ |
| `0x000a` | `ACCESS_PLAN` | ○ |
| `0x000b` | `RESOURCE_PROFILE` | ○ |
| `0x000c` | `ENTRY_POINTS` | ○ |
| `0x8001` | `EMBEDDED_SOURCES` | 任意・non-critical |
| `0x8002` | `DIAGNOSTICS` | 任意・non-critical |
| `0x8003` | `BUILD_PROVENANCE` | 任意・non-critical |

未知 opcode は section と違って長さも stack effect も分からないため、**常に artifact 全体を拒否する**。

## 5. table

### 5.1 `FEATURES`

各 record は次を持つ。

```text
feature_name:StringId
min_major:u16 min_minor:u16
flags:u16                 // bit 0 = REQUIRED、他は 0
parameter_bytes_length:u32
parameter_bytes:[u8]
```

feature 名は逆 DNS または `vaak.core.*` の UTF-8 名である。未知 required feature は拒否する。
unknown optional feature は診断用 metadata としてだけ無視でき、code/type/host schema がそれに依存してはならない。

**feature と capability は別である。** feature は VM が理解する機構、capability は host が与える権限である。
capability は `HOST_SCHEMA` の各 host item に付け、prepare 時に grant と照合する。

### 5.2 `STRINGS`

UTF-8 byte 列を bytewise 昇順に並べ、重複を拒否する。`StringId` は table index の `u32`。
空文字列も通常 record とし、存在する場合は先頭になる。名前の比較は decoded host language の文字列比較でなく
UTF-8 byte 列で行う。

### 5.3 `SOURCES` / `SPANS`

`SourceId 0` と `SpanId 0` は `NONE` であり、物理 table には sentinel record を置かない。
最初の source/span record の ID は 1、すなわち `physical_index + 1` である。それ以外の source record は次を持つ。

```text
display_name:StringId
canonical_source_id:StringId
byte_length:u32
sha256:[u8;32]
embedded_source_id:u32     // NO_ID なら非埋込み
```

span record は `source_id:u32, start:u32, end:u32`。UTF-8 の byte offset であり、行・桁は表示時に source から
計算する。`start <= end <= byte_length` を検証する。埋込み source がある場合は length/hash/UTF-8 を再検証する。

`canonical_source_id` の multi-module 規則は module 設計と一緒に決める必要がある。この案は wire 上で opaque な
UTF-8 identity を運ぶところまでに留め、filesystem path や URL を意味論へ固定しない。

### 5.4 `TYPES`

primitive `TypeId` は固定する。

| TypeId | type |
|---:|---|
| 1 | `u1` |
| 2 | `u8` |
| 3 | `u16` |
| 4 | `u32` |
| 5 | `i32` |
| 6 | `i64` |
| 7 | `f32` |
| 8 | `f64` |
| 9 | `str` |

`f80` は現行でも STEEL 方言だけで木を辿る実装と VM に無いため、core ABI v0 に入れない。
将来入れるなら required feature と別 TypeTag を要し、既存 reader に別の型として誤認させない。

composite record は `TypeId >= 0x100` とし、child TypeId は自分より小さくなければならない。
これで型 graph は一 pass で検証でき、循環を表せない。

| TypeTag | payload |
|---:|---|
| `0x10` | `array(element:TypeId)` |
| `0x11` | `map(key:TypeId, value:TypeId)` |
| `0x12` | `hash(key:TypeId, value:TypeId)` |
| `0x13` | `struct(identity:StringId, field_count:u16, fields...)` |
| `0x14` | `wrap(identity:StringId, base:TypeId)` |

struct field は宣言順に `name:StringId, type:TypeId, bind_kind:u8, primary_span:SpanId` を持つ。
`bind_kind` は `0=var, 1=let, 2=const`。alias は値の型ではないので TypeTag にしない。
named type の identity を single source / future module 間でどう作るかは未決事項であり、wire は opaque identity と
field schema の一致だけを検証する。

### 5.5 `CONSTANTS`

各 record は `type_id:u32, encoded_length:u32, WireValueV0 bytes`。値は後述の canonical wire value である。
同じ `(TypeId, bytes)` を二つ置かず、`TypeId` の数値順、その中で value bytes の bytewise 昇順に並べる。
code は `ConstId:u32` で参照する。

composite constant は pointer graph でなく値 tree である。decoder は明示 work stack で検証し、host call stack を
使う再帰 decoder にしない。struct は schema の field 順、map は Vaak の鍵順、hash は挿入順を保存する。
duplicate key、型不一致、非有限 float、上限超過を拒否する。

### 5.6 `FUNCTIONS`

function record は少なくとも次を持つ。

```text
identity:StringId
kind:u8                    // 0=top, 1=free, 2=method
flags:u8                   // v0 reserved=0
parameter_count:u16
slot_count:u16
declared_max_stack:u16
return_type:TypeId         // NO_ID = paradox-only outer face
code_first_instruction:u32
code_instruction_count:u32
primary_span:SpanId
parameter descriptors[]
slot descriptors[]
```

parameter descriptor は `slot:u16, type:TypeId, bind_kind:u8, is_alias:u8`。
slot descriptor は `type:TypeId, bind_kind:u8, is_alias:u8, lexical_scope:u32, declaration_span:SpanId`。
`is_alias` は cell の束縛形態を示すだけで、値型へ伝播しない。method の receiver も parameter 0 として同じ表へ入る。
`return_type=NO_ID` は `->` の無い関数の paradox-only 外界面であり、unit 型を新設するものではない。

function ID はこの table の index。table 順を abstract artifact の一部とし、canonical compiler は
top を先頭、その後を linked declaration の canonical source orderで並べる。`HashMap` iteration は使わない。

### 5.7 `HOST_SCHEMA`

登録時の**一列の順序をそのまま保存する。** value と function を別々に sort しない。

```text
entry_ordinal:u32
name:StringId
kind:u8                    // 0=value, 1=function
flags:u8                   // v0 function は LEAF_SYNC のみ
reserved:u16
capability_name:StringId   // 空文字列なら core host binding
capability_major:u16 capability_minor:u16
payload...                 // value TypeId または function signature
```

value index と function index は、それぞれ先頭から同 kind を数えた番号である。record に native pointer や
delegate/function object を入れない。function signature は parameter TypeId 列と optional return TypeId。
`ret = NO_ID` は unit でなく、**値を置かず call span の paradox になる**という S-11 の意味を保つ。

name の exact UTF-8 byte 列は全 kind を通じて重複不可。prepare 時に host が提示した `(order, name, kind,
type/signature, capability version)` と完全一致させ、canonical bytes の SHA-256 を `host_schema_digest` とする。

### 5.8 `ACCESS_PLAN`

S-8 / S-22 の `host_reads`、`host_writes`、`host_touched` を保存するが、producer の申告を信用しない。
verifier が code から再計算し、record と byte-for-byte 一致するときだけ host adapter に渡す。

value host slot ごとに次を持つ。

```text
read_mode:u8       // 0=unused, 1=full, 2=partial-array
write_mode:u8      // 0=unused, 1=full, 2=partial-array
index_count:u32
indices:[i64]      // 昇順・重複なし
```

`.len()` だけを見る partial snapshot でも実 length は必要である。中間 field、動く index、alias、破壊的 method、
解析不能な経路は保守的に `full` へ倒す。読み過ぎは許せるが読み落としは許さない。

### 5.9 `RESOURCE_PROFILE` / `ENTRY_POINTS`

resource profile は verifier が計算できる値と producer 申告を両方持つ。計算値が申告を超えれば拒否する。
動的上限は host policy と照合し、満たせない artifact は**実行前**に拒否できる。

v0 の entry point は現行 `PreparedProgram` と同じ top-level 一つだけである。`ENTRY_POINTS` は
`kind=top, function_id, name="main"` の一 record を持つ。named entry、phase ABI、中断再開をこの文書から
先回りして足さない。

## 6. 固定 opcode

opcode は `u8` 一つで、operand は下表の順に固定幅で続く。jump/deferred target は byte offset でなく
**同じ function 内の instruction ordinal `u32`**。decoder は先に instruction boundary table を作る。

| opcode | name | operands |
|---:|---|---|
| `0x01` | `CONST` | `const:u32` |
| `0x02` | `PARADOX` | `span:u32` |
| `0x03` | `LOAD` | `slot:u16, span:u32` |
| `0x04` | `REF` | `slot:u16, span:u32` |
| `0x05` | `STORE` | `slot:u16, span:u32` |
| `0x06` | `UPDATE` | `slot:u16, binop:u8, span:u32` |
| `0x07` | `STORE_EXACT` | `slot:u16, span:u32` |
| `0x08` | `ALIAS` | `destination:u16, source:u16, span:u32` |
| `0x09` | `FREEZE` | `slot:u16, span:u32` |
| `0x0a` | `DECLARE` | `slot:u16, span:u32` |
| `0x0b` | `POP` | — |
| `0x0c` | `DUP` | `span:u32` |
| `0x10` | `JUMP` | `target:u32` |
| `0x11` | `JUMP_IF_VALUE` | `target:u32, span:u32` |
| `0x12` | `JUMP_IF_FALSE` | `target:u32, span:u32` |
| `0x13` | `JUMP_IF_PARADOX` | `target:u32, span:u32` |
| `0x14` | `NEED_U1` | `span:u32` |
| `0x15` | `NEED_VALUE` | `span:u32` |
| `0x16` | `BIN` | `binop:u8, span:u32` |
| `0x17` | `UN` | `unop:u8, span:u32` |
| `0x18` | `COERCE` | `type:u32, span:u32` |
| `0x19` | `DEPTH` | `span:u32` |
| `0x1a` | `RET` | `span:u32` |
| `0x1b` | `CALL` | `function:u32, argc:u16, span:u32` |
| `0x1c` | `METHOD` | `name:u32, argc:u16, span:u32` |
| `0x1d` | `MUT_METHOD` | `receiver_slot:u16, name:u32, argc:u16, span:u32` |
| `0x1e` | `HOST_CALL` | `host_function:u16, argc:u16, span:u32` |
| `0x20` | `MAKE_ARRAY_LITERAL` | `element_type:u32, count:u16, span:u32` |
| `0x21` | `MAKE_ARRAY_FILL` | `element_type:u32, span:u32` |
| `0x22` | `MAKE_U8_ARRAY_ONE` | `span:u32` |
| `0x23` | `MAKE_MAP_LITERAL` | `key_type:u32, value_type:u32, count:u16, span:u32` |
| `0x24` | `MAKE_STRUCT` | `type:u32, field_count:u16, span:u32` |
| `0x25` | `INDEX` | `span:u32` |
| `0x26` | `FIELD` | `name:u32, span:u32` |
| `0x27` | `LOAD_INDEX` | `slot:u16, span:u32` |
| `0x28` | `LOAD_FIELD` | `slot:u16, name:u32, span:u32` |
| `0x29` | `LOAD_LEN` | `slot:u16, span:u32` |
| `0x2a` | `STORE_INDEX` | `slot:u16, span:u32` |
| `0x2b` | `UPDATE_INDEX` | `slot:u16, binop:u8, span:u32` |
| `0x2c` | `PLACE_ROOT` | `slot:u16, span:u32` |
| `0x2d` | `PLACE_FIELD` | `name:u32, span:u32` |
| `0x2e` | `PLACE_INDEX` | `span:u32` |
| `0x2f` | `LOAD_PLACE` | `missing_is_paradox:u8, span:u32` |
| `0x30` | `LOAD_PLACE_LEN` | `span:u32` |
| `0x31` | `STORE_PLACE` | `span:u32` |
| `0x32` | `UPDATE_PLACE` | `binop:u8, span:u32` |
| `0x33` | `SET_INDEX` | `span:u32` |
| `0x34` | `SET_FIELD` | `name:u32, span:u32` |
| `0x40` | `BREAK` | `stages:u32, outward_bits:u64, has_payload:u8, span:u32` |
| `0x41` | `CONTINUE` | `stages:u32, outward_bits:u64, deferred_target:u32, span:u32` |
| `0x42` | `BREAK_DYNAMIC` | `flags:u8, extra_stages:u32, extra_outward:u64, deferred_target:u32, operator_span:u32, span:u32` |
| `0x43` | `REGION_BEGIN` | `span:u32` |
| `0x44` | `REGION_END` | `span:u32` |
| `0x45` | `BLOCK_BEGIN` | `span:u32` |
| `0x46` | `BLOCK_END` | `span:u32` |
| `0x47` | `LOOP_BEGIN` | `span:u32` |
| `0x48` | `LOOP_BODY` | `target:u32, span:u32` |
| `0x49` | `LOOP_END` | `span:u32` |
| `0x4a` | `NFOR_BEGIN` | `slot:u16, span:u32` |
| `0x4b` | `NFOR_NEXT` | `target:u32, span:u32` |

内部 `MakeArray(u16::MAX)` の sentinel と host-touch 用 constant ID は wire に出さない。
`MAKE_ARRAY_FILL` と `ACCESS_PLAN` に分離する。`Place` の root も verifier が opcode 列から追い、
`LOAD_PLACE` 等へ内部解析用 root/constant ID を埋め込まない。

`binop` の固定値は `01 add, 02 sub, 03 mul, 04 div, 05 mod, 06 shl, 07 shr, 08 bit_and,
09 bit_xor, 0a bit_or, 0b lt, 0c le, 0d gt, 0e ge, 0f eq, 10 ne`。
短絡 `&&` / `||`、`??`、`|>` は control flow / call へ lower 済みでなければならず、`BIN` tag にしない。
`unop` は `01 neg, 02 pos, 03 not`。未知 tag は拒否する。

`CONTINUE` / `BREAK_DYNAMIC` の `deferred_target` は無しなら `NO_TARGET`。`BREAK_DYNAMIC.flags` は
bit 0 が `has_payload`、bit 1 が `resume`、bit 2--7 は 0 とし、未知 bit を拒否する。

### 6.1 実行意味を固定する gate

上表は番号と operand schema の registry であり、Rust `match` 文を仕様にしない。採用時には各命令の
pop/push shape、paradox face、cell/place permission、stage/region effect を machine-readable な一表にし、
compiler・verifier・disassembler・Rust/C#/Java VM の fixture をその表から照合する。特に次を不変条件とする。

- operand stack の通常の式結果は `T` だけでなく `T ⊕ paradox(origin_span)` になり得る。分岐合流は
  compatible な value face を照合し、paradox face の和を取る。paradox-only と `T ⊕ paradox` を
  Rust enum の同じ variant に押し潰さない。
- `STORE` / `DECLARE` / `STORE_EXACT` は value face を cell に保存するが、実行時に paradox face が選ばれたら
  cell へ sentinel を保存せず、origin span の「消費されなかった paradox」になる。
- `CALL` は非 alias 引数を深く複製して新しい callee cell へ置き、alias 引数だけ caller の cell identity を渡す。
  同じ root が alias 引数どうし、または可変 receiver と alias 引数へ二度届けば call span で拒否する。
- `RET` は value だけを caller の領域へ移す（実装上の move は可）。cell/place/alias identity を返さない。
- `HOST_CALL` は引数値を深く snapshot 化して adapter へ渡し、host native object を VM cell にしない。
  no-return signature は call span の paradox を一つ置く。
- map は Vaak の鍵順、hash は live entry の挿入順を観測意味に含める。VM native dictionary の反復順を使わない。

この詳細で参照実装と相違が見つかったら opcode の表を正当化せず、参照実装と既存決定へ戻す。

## 7. `WireValueV0`

value record は常に期待 TypeId と一緒に decode する。tag を Rust `Value` discriminant と同じ番号にしない。

| type | canonical payload |
|---|---|
| `u1` | `u8` の 0/1 |
| `u8/u16/u32/i32/i64` | 固定幅 little endian |
| `f32/f64` | 有限な IEEE bit pattern。NaN/±Inf は拒否 |
| `str` | `length:u32 + raw bytes`。UTF-8 を要求しない |
| `array` | `count:u32 + element values` |
| `map` | `count:u32 + key/value`。Vaak の鍵の全順序で昇順、重複なし |
| `hash` | `count:u32 + key/value`。挿入順、重複なし |
| `struct` | schema の field 順の value。field 名を値ごとに反復しない |
| `wrap` | base value 一つ |

一般 value の `-0.0` は bit pattern を保存する。map/hash key だけは C-99 の単調 key 変換と
`-0.0 == +0.0` の同一性を全 runtime で揃える。非有限値は paradox を生む演算結果であり、cell/constant/wire value
として保存できない。

decoder は depth と node count を明示 work list で数える。C# object graph、Java serialization、Lua table の
identity/shared reference/metatable は wire value の一部ではない。同じ value を二回参照すれば二つの深い値である。

## 8. host snapshot / patch / command

bytecode artifact と run ごとの値を同じ blob にしない。run envelope は別 magic と schema version を持つ。

snapshot / patch / command は共通の 24-byte header を先頭に持つ。

```text
magic:[u8;8]              // `VAAKRUN\0`
adapter_schema_major:u16  // v0 は 0
adapter_schema_minor:u16  // v0 は 0
message_kind:u16          // 1=snapshot, 2=patch, 3=command
flags:u16                 // v0 は 0
exact_message_length:u32
reserved:u32              // 0
```

major、未知 message kind、未知 flag、length 不一致は native value へ decode する前に拒否する。
新しい minor が既存 message record の意味を変えず、未知 field を安全に飛ばすには、artifact と同様に
別 length-delimited section を足す。native struct の末尾を伸ばしただけの encoding は使わない。

### 8.1 `SnapshotV0`

```text
host_schema_sha256:[u8;32]
execution_id:[u8;16]
entry_count:u32
entries[]
```

entry は host value index と次のいずれかである。

- `FULL(type_id, WireValueV0)`
- `PARTIAL_ARRAY(type_id, real_length:u32, sorted(index:i64, value)[])`
- `LENGTH_ONLY(type_id, real_length:u32)`

`ACCESS_PLAN` が許さない partial form は拒否する。欠けた index を零や empty に見せない。実 length と範囲外
paradox を保つ。snapshot は host native object の写しであり、alias はこの snapshot から作る VM cell を指す。
snapshot header を含む canonical bytes の SHA-256 を `snapshot_digest` とし、対応する patch が別 snapshot へ
誤適用されないよう execution ID とともに照合する。

### 8.2 `PatchV0`

run は成功・paradox・runtime error・resource error のいずれでも
`OutcomeV0 + PatchV0 + ErrorEnvelopeV0?` を返す。patch entry は次のいずれかである。

patch body は `host_schema_sha256`、`execution_id`、`snapshot_digest`、outcome、entry count を先に持つ。

- `REPLACE(value_index, WireValueV0)`
- `SET_ARRAY_ELEMENT(value_index, index:i64, WireValueV0)`

index は昇順、同じ index は一度だけ。元 snapshot と同じ値は patch に出さない。
runtime error の前に起きた write は patch に残す。adapter は mutable host binding の patch を適用してから
error を返すことで C-2 / S-22 を保つ。apply 途中の native host failure は host adapter error であり、
どこまで apply したかを provenance に残す。

### 8.3 `CommandBufferV0`

複数 native object を原子的に変えたい場合は、Vaak の host binding を直接変更せず、version 付き command 値を
通常の返り値として作る。host は command 全体を validate し、copy へ apply してから交換する。

これは C-2 の巻き戻しを変更しない。Vaak 実行中には host 作用がまだ起きていないからである。
command opcode/schema は capability ごとに version を持ち、portable core opcode へ追加しない。
command body は `execution_id`、capability name/version、command schema digest、command count と
length-delimited command record を持ち、未知 required command は commit 全体を拒否する。

## 9. C# / Java / Lua に依存しない host adapter

各 adapter が実装する概念的な面は次だけである。

```text
bind(host_schema, granted_capabilities) -> bound-layout | error
snapshot(access_plan)                   -> SnapshotV0 | error
call(host_function_index, WireValueV0[]) -> HostCallResultV0
apply(PatchV0)                          -> ApplyResultV0
validate_and_commit(CommandBufferV0)    -> CommitResultV0
```

- C# delegate、Java interface object、Lua function/closure は `bind` 後の adapter table にだけ置く。
  artifact は host function index と signature しか持たない。
- Lua C API の stack index、registry reference、userdata pointer、string pointerを保存しない。Lua stack 上の値は
  call 中だけ `WireValueV0` へ copy/range-check し、帰る前に stack height を復元する。
- Lua table の反復順を map/hash の順序として使わない。adapter が schema に従って array、鍵順 map、挿入順 hash を
  明示的に構築する。Lua number を `i64/f64` へ黙って丸めず、型と範囲を検査する。
- C# `long`、Java `long`、Lua integer の native layout は wire 幅を決めない。全て little-endian fixed scalar へ写す。
- host error は native exception / Lua error object を wire にせず、stable domain/code/message bytes/details へ写す。
- handle が必要なら host schema 固有の `(arena identity, epoch, slot, generation)` 等を整数/struct value として明示し、
  pointer や GC reference を handle と呼び替えない。

Lua は必須 runtime でも Vaak の意味論でもない。Lua adapter conformance は snapshot/patch/call schema の試験であり、
pure Lua VM を portable bytecode v0 の合格条件にはしない。

### 9.1 再入 gate

v0 の host function flag は `LEAF_SYNC` だけである。adapter は `execution_id` と call-chain token を持ち、
active chain に同じ Vaak runner/runtime identity が現れたら `HOST_REENTRANCY_FORBIDDEN` で call span に失敗する。

- 同じ `Runner` への直接再入を拒否する。
- 別 runner を経由して元へ戻る A → B → A cycle も token で拒否する。
- host call が別の Vaak run を必要とする場合は、外側 run が終わってから queue する。
- 将来の suspend/resume は別 required feature と adapter ABI minor を要する。v0 の同期 call を暗黙に再入可能へ
  変更しない。

## 10. error provenance

文言を runtime 間の比較契約にしない。error envelope は次を持つ。

```text
domain:u16              // decode, verify, static, runtime, host, resource, adapter
code:u32                // stable code
phase:u16
primary_span:SpanId
secondary_spans:SpanId[]
message_key:StringId?   // artifact内診断だけ。runtimeはcodeから表示してよい
message_args:WireScalar[]
call_frames:(function_id, call_span)[]
host_provenance?: {
  host_entry_ordinal:u32,
  host_name:StringId,
  invocation_ordinal:u64,
  adapter_schema_version:u32,
  native_domain_utf8:bytes,
  native_code_utf8:bytes
}
```

paradox slot は `origin_span` を持つ。`NEED_VALUE` がそれを消費できず失敗するとき、primary は
`NEED_VALUE` の span でなく `origin_span` である。consumer span は secondary にできる。
host signature/return schema error は `HOST_CALL` span を primary、host item と adapter error を provenance にする。

diagnostic text は locale ごとに変えてよい。cross-runtime gate が比較するのは domain/code/span/provenance/value と
writeback であり、日本語/英語の文章そのものではない。

## 11. 構造 verifier

一つの大きな `deserialize` で Rust object を作らず、次の順に段階を分ける。

1. **envelope**：magic/version/file length、checked integer arithmetic、directory の順序・範囲・重なり・padding。
2. **tables**：UTF-8 string、source/hash/span、TypeId DAG、constant value tree、全 ID range。
3. **schema**：function/slot/host signature、named type field、capability、resource profile。
4. **decode**：各 function の code を opcode ごとの既知長で走査し instruction boundary table を作る。
5. **CFG**：jump/deferred target が同じ function の boundary、到達不能域を含め全 instruction を検査。
6. **structured state**：region、block、loop、nfor、freeze scope、frame の begin/end と全 path の合流を照合。
7. **typed abstract stack**：式の外界面 `outer(value-face:T?, may-paradox)`、
   `cell-ref<T,root-set>`、`place<T,root-set>` を区別し、stack height/value face を fixed point まで検査する。
   合流では compatible な value face と paradox face の和を取り、cell 自体には paradox を保存しない。
8. **alias**：alias destination の束縛形態、type、lexical lifetime、permission を検査する。call の alias parameter と
   method receiver は root-set を集め、同じ root が二つ届く bytecode を拒否する。合流は root-set の和へ倒す。
9. **effects**：host read/write/partial index plan を code から再計算し `ACCESS_PLAN` と一致させる。
10. **limits/capabilities**：算出 max stack、slot、stage、call edge、constant/source sizeを profile/grant と照合。
11. **seal**：成功後だけ `VerifiedProgramV0` を作り、実行 API は raw bytes を受け取らない。

alias root/lifetime の証明に必要な metadata を slot/scope tableだけで再計算できるか、producer の proof tableを
追加して verifier が照合すべきかは実装 probe が必要である。proof table を採っても producer を信用せず、
少なくとも root-set と control-flow consistency を verifier が再計算する。

### 11.1 必ず拒否する例

- truncated/overlapping section、offset+length overflow、重複 section、non-zero reserved/padding。
- overlong/invalid UTF-8 name、範囲外 ID、型循環、constant の型違い・duplicate map key・non-finite float。
- unknown opcode/binop/unop、opcode operand の末尾切れ、function 外 jump、instruction 中央 jump。
- branch 合流で stack height/value face が incompatible、region/stage 不整合、RET shape 不一致。
- cell/place を value として STORE、値として alias を collection/struct に入れる、短命 cell への alias。
- alias root collision、const/frozen cell への write、署名と argc/return type の不一致。
- host schema/access plan/capability/resource profile の不一致。

拒否時には host snapshot/read/call/write を一度も行わない。同じ不正 artifact を Rust/C#/Java verifier が
少なくとも同じ error domain/code と byte offset で拒否する corpus を持つ。

## 12. canonical encoding

同じ abstract artifact は一つの byte 列だけを持つ。

- little endian fixed width、最短/最長の別表現なし。
- section は kind 順、record count と exact payload length が一致、reserved/padding は zero。
- string は UTF-8 byte 順で deduplicate。
- composite type は child-first structural key 順、named type は opaque identity と declaration order を key にする。
- constant は `TypeId` 数値順、その中で canonical `WireValueV0` bytes 順に deduplicate。
- source/module/function/field の semantic order を `HashMap` / filesystem enumeration 順から作らない。
- map は key の Vaak 全順序、hash は挿入順。duplicate key は emitter/reader とも拒否。
- float は有限 bit patternを保ち、key だけ C-99 の canonical keyを使う。
- diagnostic/build provenance を除いた core sections に timestamp、absolute build path、random ID を入れない。
- decoder → canonical encoder の round trip は byte-for-byte 同一でなければならない。

artifact hash は canonical file 全体の `sha256:<lowercase hex>`。SHA-256 の実装差を避けるため test vector も置く。

## 13. resource limits と exhaustion

次は v0 probe の**decoder hard ceiling 案**であり、言語の値域ではない。実測前に増減できるが、
採用後は format/ABI profile として version を付ける。

| 対象 | v0 ceiling 案 |
|---|---:|
| artifact 全体 | 64 MiB |
| section 数 | 64 |
| 一 section | 32 MiB |
| string 数 / total bytes / 一 string | 262,144 / 16 MiB / 1 MiB |
| source 数 / 一 source / embedded total | 4,096 / 16 MiB / 32 MiB |
| span 数 | 1,048,576 |
| type 数 / descriptor depth / descriptor node | 65,535 / 256 / 65,536 |
| constant 数 / value depth / total value node | 1,048,576 / 256 / 1,048,576 |
| 一 collection の要素 | 1,048,576 |
| function 数 | 65,535（top を含む） |
| 一 function の slot / parameter / operand stack | 65,535 / 65,535 / 65,535 |
| 一 function / 全体の instruction | 1,048,576 / 4,194,304 |
| host value / host function | 65,535 / 65,536 |
| 静的 region/stage nesting | 4,096 |
| diagnostic 数 / 一 message / message total | 256 / 64 KiB / 1 MiB |
| diagnostic call frame | 256 |

run policy は別に `fuel/instruction count`、call depth、arena/allocation bytes、collection growth、host call count、
patch bytes、diagnostic bytes、wall-clock cancellation を制限する。artifact は最低必要量を宣言できるが、
host ceiling を引き上げられない。

resource exhaustion は paradox にしない。`??` で捕まるようにすると言語意味が変わるため、
`RESOURCE_EXHAUSTED` runtime outcome とする。発生 instruction span と使用量/limit を返し、それ以前の host
writeback は C-2 どおり patch に残す。snapshot → command buffer 車線なら host commit 前なので作用は残らない。

## 14. 配布 manifest

artifact 本体に配布場所・署名・license を詰めず、`vaak-package-v0.json` を隣に置く。JSON は UTF-8 の
[RFC 8785 JCS](https://www.rfc-editor.org/rfc/rfc8785.html) で canonicalize する。

最低 field は次である。

```json
{
  "schema": "org.vaak.package-manifest/0.0",
  "moduleId": "opaque-canonical-id",
  "artifact": {
    "mediaType": "application/vnd.vaak.bytecode.v0",
    "path": "program.vbc",
    "size": 1234,
    "digest": "sha256:...",
    "format": "0.0",
    "coreAbi": "0.0"
  },
  "hostSchemaDigest": "sha256:...",
  "requiredFeatures": [],
  "requiredCapabilities": [],
  "resourceProfile": {},
  "entryPoints": ["main"],
  "sources": [{"id": "...", "size": 0, "digest": "sha256:..."}],
  "compiler": {"name": "vaak", "version": "...", "semanticCommit": "..."},
  "license": "MIT"
}
```

`digest + size + mediaType` の組は OCI descriptor の考え方を借りるが、OCI runtime/image layoutを Vaak の
必須依存にはしない。license は artifact/source ごとの
[SPDX license expression](https://spdx.github.io/spdx-spec/v3.0.1/annexes/spdx-license-expressions/) とする。
Vaak repository 自体が MIT であることと、利用者 program の license を同一視しない。

manifest は source hash、host schema hash、feature/capability、format/ABI を cache key に含める。
timestamp と absolute path は既定で出さない。署名・transparency log・revocation は v0 の未決事項であり、
SHA-256 は改竄検出であって発行者認証ではない。

## 15. versioning と migration

- **format major**：旧 reader が構造を安全に飛ばせない変更。旧 artifact の再 compile を基本とする。
- **format minor**：未知 non-critical section の追加等、旧 reader が安全に無視できる変更。
- **ABI major**：既存 opcode/value/host/error の観測意味が変わる変更。binary-to-binary の黙示 migration をしない。
- **ABI minor + required feature**：既存意味を変えず opcode/type/capabilityを追加する変更。
- **adapter schema**：snapshot/patch/command envelope を独立 versioning する。

source が migration の第一経路である。binary migrator は同じ core ABI 内の mechanical format 変換だけを行い、
旧 artifactを旧 verifier/VMで、変換後を新 verifier/VMで differential corpusへ掛ける。変換後は canonicalize、
再検証、新 digest、新 manifestを必要とする。

unknown opcode を「NOP として飛ばす」、unknown type を bytes として渡す、unknown host capability を名前だけで
bind する、といった forward compatibility は禁止する。安全に意味を保てないものは明示的に拒否する。

## 16. cross-runtime conformance gate

### 16.1 golden artifact

各 fixture は次を一組にする。

1. Vaak source と source SHA-256。
2. canonical `.vbc` bytes と SHA-256。
3. 人間可読 disassembly。
4. verifier の算出 stack/slot/access/resource plan。
5. host schema、snapshot、host-call transcript、patch/command。
6. expected `OutcomeV0` と error provenance。

Rust decoder の decode→encode、C# decoder、Java decoder が同じ bytes/hashを出す。C#/Java writer をまだ
作らない段階でも reader が全 field と unknown/rejection corpus を同じに解釈する。

### 16.2 意味 fixture

最終値だけでなく event trace を比べる。

- **paradox**：`1/0 ?? 42`、分岐合流の `T ⊕ paradox`、paradox-only を `STORE` して発生点で失敗、
  空 region、host function `ret=None`。
- **S-16**：`if` branch が `;` で空になり、caller 側でなく branch の発生 span が残る。
- **alias**：value 引数の深い複製、可変 alias の writeback、alias rebind、receiver と alias argument の衝突。
- **部分 host access**：constant index、dynamic index、length-only、unchanged value は patch なし。
- **error writeback**：一項を書いた後に runtime error。Rust/C#/Java が同じ partial patch と error spanを返す。
- **host call**：順序・引数・return、no-return paradox、host error、schema mismatch、再入 attempt。
- **collection**：map の鍵順、hash の挿入順・remove後、`-0.0/+0.0` key、duplicate rejection。
- **numeric**：各幅の折返し、Euclidean division、幅以上の shift、非有限 float → paradox。
- **control**：nested region/block/loop/nfor、dynamic break、deferred continue、outward error。
- **source**：同じ byte offset を持つ複数 source と `SpanId 0`、UTF-8 multi-byte の行桁表示。

実行比較は次の順に増やす。

```text
木を辿る参照実装
  ↕ source-level differential
Rust Program2 VM
  ↕ portable compiler / verified adapter
Rust portable VM
  ↕ same .vbc / transcript / patch / provenance
pure C# VM
pure Java VM
```

Lua はこの VM 列へ必須で入れず、同じ `.vbc` を実行する Rust/C#/Java VM に対する host adapter fixture として
`Lua value ↔ WireValueV0`、stack balance、GC後 pointer非保持、error/reentrancy、patch/command commit を試す。

### 16.3 hostile corpus と fuzz

- valid artifact の各 byte/length/ID/opcode を mutate し、panic/OOM/host作用なしで拒否する。
- source generator を上限内で走らせ、参照実装と全 VM の value/error span/host transcript/writeback を比較する。
- decoder/verifier fuzz は小さい policy limitで行い、巨大宣言長だけで巨大 allocationをしない。
- verifier を通った artifactだけを differential executionへ渡す。
- runtime ごとの error text ではなく stable code と byte offsetを比較する。

## 17. 実装前 roadmap

1. **format corpus**：この案から encoder無しの手書き最小 blob、invalid corpus、disassembler schemaを作る。
2. **Rust decoder/verifier**：実行せず `VerifiedProgramV0` と canonical re-encodeだけを実装する。
3. **compiler bridge**：AST/check/type-check 済み sourceから portable tableを作り、内部 `Program2` と同じ
   differential fixtureへ掛ける。`Program2` の memory dump は一度も経由しない。
4. **Rust portable VM**：参照実装/既存 VMと event traceを揃える。
5. **pure C# reader/verifier/VM**：Unity/.NET object layoutを artifactへ漏らさない。
6. **pure Java reader/verifier/VM**：JVM class/object layoutを artifactへ漏らさない。
7. **adapter corpus**：C#/Javaに加えLua adapterのsnapshot/patch/command/error/reentrancyを揃える。
8. **manifest/cache/migration**：canonical hash、JCS manifest、corrupt cache、旧minor fixtureを試す。

各段で木を辿る実装が勝つ。言語意味の判断が必要になったら、実装せず未決事項として返す。

## 18. 未決事項

次はこの文書で確定しない。

1. future module の canonical `SourceId` と named type identity。既存 module probe と同時に決める必要がある。
2. alias root/lifetime verifier が全てを再計算するか、照合可能な proof tableを持つか。
3. host function error を現行 `Option<Value> + contract error side-channel` からいつ version付き resultへ移すか。
4. suspend/resume、MaySuspend host function、安全な再入をどの ABI versionで足すか。
5. named entry / phase ABI。v0 は top-level一つに閉じる。
6. decoder hard ceiling の最終値と、conforming runtime の最低保証 profile。上表は probe 値である。
7. `-0.0` の一般 value canonicalization。v0案は bit保存、keyだけ正規化とするが、全 member functionの観測を
   conformance fixtureで確かめる必要がある。
8. capability namespace の所有・衝突・失効、package署名、trust store、revocation。
9. multi-artifact link と bytecode-level import。v0 は一つの閉じた programだけを扱う。
10. sourceを含まないartifactから将来 ABI majorへ移す方針。基本は source再compileだが保存要件を決めていない。

これらを `decisions.md` の新しい `S-n` として先回りして決めない。

## 19. 参考にした公開仕様

- [WebAssembly binary format](https://webassembly.github.io/spec/core/binary/index.html) — section、固定 opcode、index。
- [WebAssembly validation algorithm](https://webassembly.github.io/spec/core/appendix/algorithm.html) — flat opcode列のtyped one-pass検証。
- [WebAssembly implementation limitations](https://webassembly.github.io/spec/core/appendix/implementation.html) — module/section/code/runtime resource limit。
- [Java Virtual Machine Specification, class file format](https://docs.oracle.com/javase/specs/jvms/se26/html/jvms-4.html) — magic、format version、constant pool、load前format checking。
- [ECMA-335](https://ecma-international.org/publications-and-standards/standards/ecma-335/) — runtime非依存metadata tableとCLI。
- [RFC 8785 JSON Canonicalization Scheme](https://www.rfc-editor.org/rfc/rfc8785.html) — manifestのcanonical JSON。
- [NIST FIPS 180-4](https://csrc.nist.gov/files/pubs/fips/180-4/final/docs/fips180-4.pdf) — SHA-256。
- [OCI content descriptor](https://github.com/opencontainers/image-spec/blob/main/descriptor.md) — media type、byte size、content digest。
- [SPDX license expressions](https://spdx.github.io/spdx-spec/v3.0.1/annexes/spdx-license-expressions/) — 配布物license metadata。
- [Lua 5.4 C API stack / pointer lifetime](https://www.lua.org/manual/5.4/manual.html#4.1) と
  [Lua 5.4 garbage collection](https://www.lua.org/manual/5.4/manual.html#2.5) — Lua stack/GC pointerをwireへ出さない根拠。
