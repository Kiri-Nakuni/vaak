# IRON VAAK language-neutral exchange schema v0

状態: implementation前のwire/schema設計  
調査日: 2026-08-24

## 目的

`SettingsSnapshot`、`SettingsPatch`、`GameCommand`、`Diagnostic`を、C#、Vaak、Lua C API、managed Lua、
その他の埋め込みscript runtimeのどれにも従属しないbyte列として交換する。

このschemaはVaakの値型やUnityのobject modelを拡張しない。各adapterが自分のruntime表現からcopyし、
境界を越えた後はruntime固有pointer/index/objectを一つも保持しない。

## 基本規律

1. messageは連続したcaller-owned byte列。receiverはcall中だけborrowし、保持するならcopyする。
2. 数値は固定幅、little-endian。wire上にnative pointer、`size_t`、C enum、C `bool`、paddingを置かない。
3. UTF-8はbyte length付きで、NUL終端ではない。位置はUnicode scalar indexでなくUTF-8 byte offset。
4. すべてのoffset/lengthはmessage先頭からの`u64`。加算overflow、範囲外、必須alignment、重複を検査する。
5. recordは固定幅。可変長dataはpayload sectionへ置く。一 recordごとのFFI callを作らない。
6. absentはrecordの不在で表す。`null`、Unity fake-null、Lua `nil`を共通値型へ足さない。
7. `f32/f64`はcanonical IEEE bit列。Vaakへ渡す値ではNaN/∞を拒否する。host-only schemaが許す場合は
   type descriptorの明示featureが必要で、Vaak値へ暗黙変換しない。
8. map/property/commandの意味は`SchemaManifest`が持つ。wire decoderがUnity property名を知ることはない。

## 共通envelope

### header: 32 byte

byte offsetごとの定義であり、C structのmemory imageではない。

| offset | width | field |
|---:|---:|---|
| 0 | 4 | magic: ASCII `IVX0` |
| 4 | 2 | schema major: `0` |
| 6 | 2 | schema minor |
| 8 | 2 | message kind |
| 10 | 2 | message flags |
| 12 | 4 | header bytes（v0は32） |
| 16 | 8 | total bytes |
| 24 | 4 | logical record count |
| 28 | 4 | section count |

header直後に24-byteのsection descriptorが`section count`件並ぶ。

### section descriptor: 24 byte

| offset | width | field |
|---:|---:|---|
| 0 | 2 | section kind |
| 2 | 2 | flags（bit 0: required） |
| 4 | 4 | record width。raw payloadは0 |
| 8 | 8 | section offset |
| 16 | 8 | section byte length |

規則:

- section startは8-byte alignment。record widthが非0ならlengthはwidthの倍数。
- section table自身、各section、total bytesは相互に範囲内でなければならない。
- v0ではsection範囲を重ねない。payload sharingはoffset参照で行い、同じbyteを別sectionに見せない。
- unknown optional sectionはskipできる。unknown required sectionはmessage全体を拒否する。
- 同じsingleton sectionの重複を拒否する。複数可否はmessage kindごとに決める。
- decoderは`total bytes`より後を読まない。trailing bytesを別messageとして解釈しない。

### message kind

| id | kind |
|---:|---|
| 1 | `SchemaManifestV0` |
| 2 | `SettingsSnapshotV0` |
| 3 | `SettingsPatchV0` |
| 4 | `GameCommandBatchV0` |
| 5 | `DiagnosticBatchV0` |
| 6 | `CapabilityGrantV0` |
| 7 | `RunRequestV0` |
| 8 | `ExecutionReportV0` |

## stable identity

wire上のidentityはすべてhost-owned scalarでありpointerではない。

| identity | width | 規律 |
|---|---:|---|
| `schema_id` | 128 bit | schema内容のcanonical hashまたはregistry ID。生成法はmanifestで宣言 |
| `session_id` | 128 bit | host session。再起動後に再利用しない |
| `run_id` | 128 bit | 一回のscheduler request |
| `transaction_id` | 128 bit | 一つのPatch batch。再送時も同じ |
| `entity_id` | 64 bit | host object tableのslot+generation相当。0はglobal settings |
| `property_id` | 32 bit | manifest内だけで安定したproperty |
| `command_id` | 32 bit | manifest内だけで安定したcommand |
| `capability_id` | 64 bit | grant tableのindex+generation。authority object/pointerではない |
| `diagnostic_id` | 64 bit | 一batch内で一意。0は「無し」 |

Unity `GetInstanceID()`、`GCHandle`、managed object address、`lua_State*`、Lua registry ref、Lua stack indexを
これらのIDとして転用しない。host tableがgenerationを検査してからobjectへ解決する。

## SchemaManifestV0

manifestはprepare時に固定し、runごとに送り直さない。最低限次を宣言する。

- schema ID/major/minorとcanonical hash algorithm
- property ID → stable name、value type、read/write、entity scope
- command ID → stable name、payload type、idempotency/ordering class
- structured type ID → field ID、field type、required/optional、defaultの有無
- Vaak host layout slot → property/aggregate/command buffer mapping
- required/optional feature setとhard resource maxima

### mandatory value types v0

| tag | payload |
|---:|---|
| 1 `U1` | 1 byte、0/1だけ |
| 2 `U8` | 1 byte |
| 3 `U16` | 2 byte LE |
| 4 `U32` | 4 byte LE |
| 5 `I32` | 4 byte two's complement LE |
| 6 `I64` | 8 byte two's complement LE |
| 7 `F32` | 4 byte IEEE 754 bits LE |
| 8 `F64` | 8 byte IEEE 754 bits LE |
| 9 `UTF8` | arbitrary bytes、strict UTF-8 |
| 10 `BYTES` | arbitrary bytes |
| 11 `FIXED_ARRAY` | manifestがelement type/countを宣言 |
| 12 `SEQUENCE` | child value descriptor列。max count/depth必須 |
| 13 `RECORD` | manifest type ID + field/value descriptor列 |
| 14 `MAP` | key/value descriptor列。key typeはmanifestで固定 |

`RECORD/MAP/SEQUENCE`はpayload内の`ValueNodeV0` tableで表す。

### ValueNodeV0: 32 byte

| offset | width | field |
|---:|---:|---|
| 0 | 4 | type tag |
| 4 | 4 | type ID / flags |
| 8 | 8 | scalar bits、またはpayload offset |
| 16 | 8 | payload length、またはfirst child index |
| 24 | 8 | child count / reserved |

規則:

- scalarはoffset参照を持たず、unused欄0。
- UTF8/BYTES/FIXED_ARRAYはpayload offset/lengthを持つ。
- SEQUENCE/MAP/RECORDは同messageのValueNode sectionをindexで参照する。
- graphでなくtree/DAGとして検証し、cycleを拒否する。最大depth/node数をhard limitにする。
- MAP keyはmanifestの許可型だけ。canonical fixtureではkey encoding昇順に並べる。
- RECORD field順はmanifest順。未知optional fieldはpreserve/skip policyをmanifest minorが定める。
- Unity `Vector3`、`Color`、Quaternion等はABI tagにせず、host manifestのRECORDとして表す。
- Lua tableはschemaに従ってSEQUENCE/RECORD/MAPのどれか一つへ写す。shapeをruntime推測しない。

## SettingsSnapshotV0

main-thread hostがあるrevisionで読んだ設定のimmutable copyである。

### meta section

| field | width |
|---|---:|
| schema ID | 16 |
| session ID | 16 |
| snapshot revision | 8 |
| record count | 4 |
| flags/reserved | 4 |

### SnapshotRecordV0: 48 byte

| offset | width | field |
|---:|---:|---|
| 0 | 8 | entity ID |
| 8 | 4 | property ID |
| 12 | 4 | type ID/tag |
| 16 | 8 | property revision |
| 24 | 8 | value node index、scalar inlineならbits |
| 32 | 8 | payload/aux |
| 40 | 4 | flags |
| 44 | 4 | reserved=0 |

規則:

- `(entity ID, property ID)`は一意でcanonical昇順。
- snapshot全体revisionとproperty revisionを両方持ち、apply時のstale checkに使える。
- snapshotはread capabilityで許可されたpropertyだけを含む。見えない値をplaceholder/nullで埋めない。
- inputはadapterから変更不能。runtime内で変更したい場合は自分の値へcopyする。

## SettingsPatchV0

host状態へまだapplyされていない変更案である。**message一件が一transaction候補**で、recordを個別applyしない。

### meta section

| field | width |
|---|---:|
| schema ID | 16 |
| session ID | 16 |
| run ID | 16 |
| transaction ID | 16 |
| base snapshot revision | 8 |
| record count | 4 |
| flags/reserved | 4 |

### PatchRecordV0: 48 byte

| offset | width | field |
|---:|---:|---|
| 0 | 8 | entity ID |
| 8 | 4 | property ID |
| 12 | 2 | operation (`SET=1`, `REMOVE=2`) |
| 14 | 2 | flags |
| 16 | 8 | expected property revision |
| 24 | 4 | value type ID/tag |
| 28 | 4 | capability table index |
| 32 | 8 | value node/payload offset |
| 40 | 8 | value aux/length |

規則:

- v0は`SET`と`REMOVE`だけ。increment/append等のread-modify-write命令を足さない。
- `REMOVE`はmanifestがremovableと宣言したpropertyだけ。値payloadは無し。
- 同じtransaction内の同じkey重複は拒否する。last-write-winsを暗黙採用しない。
- hostは全recordのschema、capability、entity generation、expected revision、resource limitを検査する。
- shadow stateで全recordを適用し、全件成功後だけUnity main threadの実体へcommitする。
- apply失敗時は0件applyし、`Diagnostic`を返す。
- transaction IDでduplicate deliveryを検出できる。保持期間/再送policyはhost manifestが定める。
- Vaak runtime errorまでにVaak内で変わったafter-stateをPatchへmaterializeしてもよい。applyするかは
  ExecutionReportを受け取ったhost policyであり、Vaak内rollbackとは呼ばない。

## GameCommandBatchV0

property更新でないhost actionの提案である。例はspawn request、audio cue、UI intentだが、
Unity APIやobject pointerをpayloadへ入れない。

### CommandRecordV0: 56 byte

| offset | width | field |
|---:|---:|---|
| 0 | 8 | sequence number |
| 8 | 4 | command ID |
| 12 | 4 | capability table index |
| 16 | 8 | target entity ID（0可） |
| 24 | 8 | idempotency key high |
| 32 | 8 | idempotency key low |
| 40 | 8 | payload value node/offset |
| 48 | 8 | payload aux/length |

規則:

- sequenceはbatch内でstrict increasing。並べ替えない。
- commandごとにmanifestが`idempotent` / `at-most-once` / `host-deduplicated`等を宣言する。
- synchronous returnを要求しない。応答が必要なら次のsnapshot/runへcorrelation ID付きdataとして入れる。
- command適用中にVaak/Luaへcallbackしない。
- PatchとCommandを一つのhost transactionに含められるかはhost policyの未決事項。schemaは別sectionで保持する。

## DiagnosticBatchV0

### DiagnosticRecordV0: 64 byte

| offset | width | field |
|---:|---:|---|
| 0 | 8 | diagnostic ID |
| 8 | 8 | cause diagnostic ID（0は無し） |
| 16 | 4 | stable code |
| 20 | 2 | severity |
| 22 | 2 | origin |
| 24 | 4 | source ID |
| 28 | 4 | flags |
| 32 | 8 | UTF-8 byte span start |
| 40 | 8 | UTF-8 byte span length |
| 48 | 8 | message payload offset |
| 56 | 8 | message payload length |

severity:

| id | 意味 |
|---:|---|
| 1 | info |
| 2 | warning |
| 3 | error |
| 4 | fatal boundary failure |

originは少なくとも次を区別する。

- FFI decode/version/limit
- Vaak prepare: parse/check/type-check/compile
- Vaak runtime/host contract
- C# adapter
- Lua C API adapter
- managed Lua adapter
- generic embedded script adapter
- Unity validation/apply
- cancellation/deadline
- internal panic

規則:

- messageは表示文で、program resultではない。
- code/origin/source/spanをmachine-readableに保ち、adapterが文字列だけへ潰さない。
- cause graphはacyclicで同batch内IDだけを指す。cycle/存在しないcauseを拒否する。
- prepareの`Span`は現行どおりinput UTF-8 byte offsetへ写す。
- Lua stack traceback等はsize上限を掛けたmessage/attachmentとし、Lua objectを保持しない。
- panic payload、managed exception object、Lua error objectを直接保持しない。sanitized copyだけにする。

## CapabilityGrantV0

authorityはC#/application hostにある。wireのgrantはhost tableに対するbounded referenceである。

### GrantRecordV0: 40 byte

| offset | width | field |
|---:|---:|---|
| 0 | 8 | capability ID |
| 8 | 8 | generation/session epoch |
| 16 | 4 | capability kind（read/write/command） |
| 20 | 4 | target ID（property/command/scope table index） |
| 24 | 8 | max records/uses |
| 32 | 8 | max bytes |

delegation規則:

- child grantのtarget集合はparentのsubset、上限は同じ以下、expiryは同じ以前。
- adapterはIDを作れない。host-issued grantをcopy/attenuate requestするだけ。
- native decode時とUnity main-thread apply時の二回検査する。
- capability tableはsession終了、scene epoch変更、explicit revokeでgenerationを進める。
- IDが漏れてもhost tableのscope/generation検査を飛ばせない。pointer capabilityにしない。

## RunRequestV0 / ExecutionReportV0

RunRequestはrun ID、schema ID、required/optional feature、capability grant、resource budget、
input snapshot hashを結ぶ。deadlineはabsolute wall-clock時刻をwireへ埋めず、hostのmonotonic scheduler policy IDで表す。

ExecutionReportは少なくとも次の独立sectionを持つ。

| section | program error時 |
|---|---|
| transport metadata | 常に |
| execution status | `completed` / `program_error` / `host_contract_error` / `cancelled_before_start` |
| top-level outcome | completed時にvalueまたはアーカーシャ。error時は無し |
| after snapshot / Patch | C-2に従い取得できた範囲を保持 |
| GameCommand batch | schema検証済みなら保持。apply可否はhost policy |
| Diagnostic batch | 常に可 |
| usage | input/output bytes、records、elapsed、将来feature時はVM steps |

`program_error`をC ABI transport errorにせずreportで返すため、after-state/diagnosticを同時に失わない。

## copy / borrow matrix

| 境界 | input | output | call後に保持してよいもの |
|---|---|---|---|
| C# → native FFI | managed/pinned byte列をcall中borrow | native runner-owned report | nativeはinput pointerを保持しない。C#はcopyしたreportだけ |
| native → C# | reportをcopy APIで読む | managed-owned byte列 | C# byte列。native internal pointerは禁止 |
| C# → Lua C API adapter | managed byte列をC shimがcall中borrow/copy | adapter-owned wire bytesをC#へcopy | Lua value/stack indexは禁止 |
| C# → managed Lua | immutable managed bytes/view | new managed bytes | managed Lua objectをnativeへ渡さない |
| scheduler → Unity apply | validated immutable Patch/Command | apply receipt/Diagnostic | stable IDとmanaged dataだけ |

## transaction / atomic apply

```text
decode entire message
  -> validate schema/version/ranges
  -> validate capability and budget
  -> resolve every entity ID+generation on main thread
  -> check every expected revision
  -> apply to shadow model
  -> validate cross-property invariants
  -> commit all Unity-visible changes
```

途中の一件だけapplyしてから失敗する経路を作らない。Unity API自体が不可逆作用を持つGameCommandは、
Patch transactionのcommit後にoutboxからdispatchするかをhost policyで決める。不可逆commandをatomicと偽らない。

## evolution

- major不一致は拒否。
- minor追加はoptional section/fieldだけ。既存fieldの意味、width、endiannessを変えない。
- unknown required feature/sectionは拒否し、黙ってdefaultにしない。
- canonical encoderはreserved=0、record sort規則、最短payloadを守る。同じlogical messageは同じSHA-256になる。
- decoderはcanonicalでないが意味上validなorderingを許すかどうかをmessage kindごとに決める。
  security/fixture用のstrict modeはcanonical以外を拒否する。
- schema manifest自体をhashし、prepared/run/reportが同じschema IDを参照する。

## malformed fixture P0

- header/section table短縮、total bytes過大/過小
- `offset + length` overflow、32/64-bit `usize` overflow
- section overlap、misalignment、record width不一致、重複singleton
- unknown optional/required section
- reserved nonzero、unsupported major/required feature
- invalid UTF-8、embedded NUL、non-BMP、0-length
- ValueNode cycle、深さ/node数超過、存在しないchild、map key型違い
- NaN/∞をVaak-bound propertyへ入れる
- duplicate snapshot key、duplicate Patch key、command sequence逆転
- stale entity/capability generation、scope外property/command
- Diagnostic cause cycle/unknown cause、message size超過
- canonical encode→decode→encode byte一致、全adapterでSHA-256一致

## 未決事項

1. `schema_id`をregistry-issued UUID系にするかcanonical manifest hashだけにするか。
2. ValueNodeのmandatory composite subsetをv0でどこまで実装するか。
3. Patchと不可逆GameCommandを同じdelivery transactionへ含めるか。
4. runtime error report中のPatch/CommandをUnity packageの既定でdiscardするか、明示policyにするか。
5. adapter複数のPatch conflictをreject、priority order、derived snapshotのどれで処理するか。
6. strict canonical decodeをproductionでも必須にするか、fixture/signature生成時だけにするか。
7. deadline/cancellation/VM fuel sectionは、core runnerの対応が決まるまでrequired featureとして保留する。

