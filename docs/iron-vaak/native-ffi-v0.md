# IRON VAAK Unity/.NET native FFI v0 独立設計

状態: implementation前の設計checkpoint  
基点: `7c5ccd706e4dc466dad45baf275c0b550c8bc777`  
調査日: 2026-08-24

## 2026-08-24 safe core scaffold checkpoint

`crates/iron-vaak-ffi`に、既存`vaak::embedding`公開APIだけを使うsafe core scaffoldを置いた。
このcheckpointは設計文書の全C ABIを完成したものではなく、raw pointerをsafe sliceへ変換した後の
再利用可能な中核である。

- packageは`iron-vaak-ffi`、成果物種別は`rlib` / `cdylib` / `staticlib`。
- crate全体を`#![forbid(unsafe_code)]`で検査する。世代・kind付き64-bit tokenをopaque handleとして
  registryへ結び、zero/staleの反復destroyをno-opにする。runnerはpreparedを`Rc`で保持するため、
  preparedを先にdestroyしても既存runnerは有効である。
- `HostLayoutEntryV0`は56 byte、`ValueNodeV0`は32 byte、snapshot/patch recordは各48 byte、
  ABI/call/diagnostic envelopeは各64 byteに固定し、RustとC11/C++17 headerの`sizeof`/`offsetof`で固定した。
- sourceとlayout名はstrict UTF-8、全offset/lengthはoverflow/range check後にcopyする。Unity safe profileは
  scalar、UTF8/BYTESのhost value slotだけで、`HostFn`を作る入口は無い。
- snapshot/patchは一property一callにせず、共通header、section table、固定幅record、value node、payloadを
  一つのlittle-endian byte列としてdecode/encodeする。C structのmemory imageをwireへcastしない。
- runnerは`Idle / Running / ReportReady / Poisoned`を持つ。同じrunnerの重複mutable accessは`BUSY`、
  panicはrunner内外の`catch_unwind`で`INTERNAL_PANIC`へ変換し、そのrunnerをpoisonする。
- runtime errorでも既存C-2/S-22のafter-stateをPatchとしてreportへ保持する。これをUnityへapplyするか
  discardするかは決めず、native scaffold自身はhost状態へ作用しない。
- Patchへ入るcapability table indexはprepare時HostLayoutからcopyするだけであり、authority objectではない。
  grantのscope/generation/resource limit検査はまだhost apply側の未実装gateである。

raw pointerを受けるC export shimはまだ無い。可変長P/Invoke入力をRust sliceへ変換するには小さな
監査済み`unsafe`または別interop層が必要であり、safe Rustだけというこのcheckpointの条件から外した。
したがって現段階の`cdylib/staticlib`を配布可能なv0 ABI完成品とは呼ばない。

## 結論

Unity/.NETの主経路は、**C ABIの同期bulk call**とする。C#は入力をcaller-owned byte列へ写し、
native側は呼出し中だけborrowする。返り値はnative runnerが所有するbyte列をC#が明示copyして受け取る。
managed object、Unity object pointer、delegate、GCHandle、Lua state/stack、Rustの参照・`Vec`・`String`は
ABIへ出さない。

Vaakの既存境界は次のように保つ。

- `PreparedProgram`: parse/check/type-check/VM compile済みの不変物。prepare once。
- `EmbeddingRunner`: arena/stack/frame容量を再利用する一実行系列の可変物。run many。
- host値: 実行前にcopyし、実行後にafter-stateを得る（C-95）。
- runtime error時にも、Vaak内でそれ以前に生じたafter-stateを失わない（C-2/S-22）。
- programの最上位結果とdiagnostic/errorは別経路（C-31）。

Unityへの変更はVaak実行中に行わない。`SettingsPatch` / `GameCommand`として返し、Unity main threadで
全件検証してからhost transactionとしてapplyする。これはVaakへrollbackを足すものではない。

## 対象外

- Unityの`GameObject`、`Component`、scene、frame lifecycleをVaakの型や意味論にすること。
- nativeからmanaged callbackを呼び、さらにVaakやLuaへ再入すること。
- LuaをIRON VAAKの必須runtimeにすること。
- WASMをUnity/.NETの通常経路にすること。WASMはWeb/sandboxed kernel向けの別車線である。
- v0で同期`HostFn`を公開すること。現行S-11第一段は再入安全な中断・再開を持たない。
- 現APIに無いVM instruction fuelや安全なin-flight interruptionを、timeoutと称して装うこと。

## 一次資料から置く制約

1. MicrosoftはC ABIを.NET interopの通常targetとし、P/Invoke署名はnative署名と一致させ、
   resource lifetimeには`SafeHandle`、可能ならblittable structを使うよう勧める。
   `bool`は既定marshallingがCの`bool`と一致しないため、ABIでは使わない。
   [Native interoperability best practices](https://learn.microsoft.com/en-us/dotnet/standard/native-interop/best-practices)、
   [Native interoperability ABI support](https://learn.microsoft.com/en-us/dotnet/standard/native-interop/abi-support)
2. UnityのIL2CPPはmanaged assemblyをC++へAOT変換しplatform compilerでnative binaryにする。
   JITやruntime code generationを前提にせず、P/Invoke exportをbuild時に固定する。
   [Introduction to IL2CPP](https://docs.unity3d.com/ja/current/Manual/il2cpp-introduction.html)、
   [スクリプトの制限](https://docs.unity3d.com/ja/current/Manual/scripting-restrictions.html)
3. Unity 6のcross-platform managed plugin基線は.NET Standard 2.1で、.NET Core targetのpluginは
   Unityではsupportされない。Unity facadeは`DllImport`を使う.NET Standard 2.1 assemblyとし、
   .NET 7+の`LibraryImport`版は一般.NET packageだけに置く。
   [.NETプロファイルのサポート](https://docs.unity3d.com/ja/current/Manual/dotnet-profile-support.html)、
   [P/Invoke source generation](https://learn.microsoft.com/en-us/dotnet/standard/native-interop/pinvoke-source-generation)
4. Rustの`repr(Rust)`、Rust ABI、`Vec`/`String`のlayoutは公開ABIにしない。C互換structは`repr(C)`を使い、
   panic/foreign exceptionを通常C ABIからunwindさせない。
   [Rustonomicon FFI](https://doc.rust-lang.org/nomicon/ffi.html)、
   [Rust Reference: unwinding](https://doc.rust-lang.org/reference/items/functions.html#unwinding)

## 層の分離

```text
Unity scene / objects                     generic .NET application
        |                                           |
        | main-thread snapshot/apply                | application adapter
        v                                           v
IronVaak.Unity (optional UPM)        IronVaak.Managed (.NET Standard 2.1)
        |             language-neutral byte envelopes             |
        +------------------------+--------------------------------+
                                 |
                      iron_vaak_v0_* C ABI
                                 |
                   iron-vaak-ffi (Rust cdylib/staticlib)
                                 |
              PreparedProgram / EmbeddingRunner / Vaak VM
```

Unity adapterは`UnityEngine`を知るが、managed core codecとnative crateは知らない。
native crateはC ABIとVaak値への変換だけを知り、Unity/Luaの型を一つも持たない。

## ABI profile

### exportとcalling convention

- export名はすべて`iron_vaak_v0_`で始め、C linkage、default C calling conventionを使う。
- Windows x86をsupportする場合だけmanaged宣言に`CallingConvention.Cdecl`を明示し、native側も一致させる。
  x64/Arm/Arm64ではplatform canonical conventionになる。
  [Unmanaged calling conventions](https://learn.microsoft.com/en-us/dotnet/standard/native-interop/calling-conventions)
- C++ headerには`extern "C"`を付ける。C++ exceptionは境界へ入れない。
- symbol visibilityはexport一覧だけに絞り、Rust symbolや既存`vaak` cdylib surfaceを配布契約にしない。
- ABIのscalarは`uint8_t/uint16_t/uint32_t/uint64_t/int32_t/int64_t/float/double/size_t`とopaque pointerだけ。
  C/C++ `long`、enum、bitfield、`bool`、`wchar_t`を使わない。
- UTF-8はNUL終端でなく`pointer + uint64_t byte_length`。embedded NULを許す。
- wire messageはcanonical little-endian。C structをwire byte列へcastせず、offsetごとにdecodeする。

### version negotiation

最初のcallは必ず`iron_vaak_v0_abi_query`である。majorが一致しなければ他のexportを呼ばない。

```c
typedef struct IronVaakAbiInfoV0 {
    uint32_t struct_size;
    uint16_t abi_major;       /* 0 */
    uint16_t abi_minor;
    uint64_t supported_features;
    uint64_t required_alignment;
    uint64_t max_wire_bytes;
    uint64_t reserved[4];
} IronVaakAbiInfoV0;

uint32_t iron_vaak_v0_abi_query(IronVaakAbiInfoV0 *out_info);
```

規則:

- callerは`struct_size`を設定し、その他を0にして渡す。
- nativeはcallerが示した範囲だけを書く。未知の末尾欄を読まない。
- `abi_major`不一致は拒否する。minorはfeature negotiationで扱い、required feature不足を明示拒否する。
- reservedは送信側0、受信側0を要求する。将来minorで意味を与えるまでは非0を拒否する。

### statusを二層にする

C関数の戻り値は**transport status**だけである。

| status | 意味 |
|---|---|
| `OK` | callを受理した。programの成否は`ExecutionReport`を見る |
| `INVALID_ARGUMENT` | null、length、alignment、handle kind等が不正 |
| `ABI_MISMATCH` | major/required feature/struct sizeが不一致 |
| `MALFORMED_WIRE` | bounds、overflow、overlap、tag、UTF-8等が不正 |
| `LIMIT_EXCEEDED` | source/message/record/depth/diagnostic上限超過 |
| `STALE_HANDLE` | release済み、世代違い、別kindのhandle |
| `BUSY` | 同一runnerの同時callまたは再入 |
| `POISONED` | 内部panic後に再利用できないrunner |
| `BUFFER_TOO_SMALL` | copy先不足。required byte数を返し、部分copyしない |
| `INTERNAL_PANIC` | Rust panicをFFI内で捕捉した。unwindは外へ出ない |

parse/check/type-check/compile/runtime error、top-level value/アーカーシャはtransport statusへ混ぜない。
version付き`Diagnostic`と`ExecutionReport`へ入れる。これによりC-31の二経路を保つ。

## handleと所有権

### opaque types

```c
typedef struct IronVaakPreparedV0 IronVaakPreparedV0;
typedef struct IronVaakRunnerV0 IronVaakRunnerV0;
```

C callerは内容を知らず、managed callerにはpointerを公開しない。

| handle | native内部 | 性質 |
|---|---|---|
| `IronVaakPreparedV0*` | `Arc<PreparedProgramRecord>`を包む所有物 | immutable。runner作成時にstrong refを取る |
| `IronVaakRunnerV0*` | 一つの`EmbeddingRunner`、出力buffer、busy/poisoned state | mutable、同時実行不可、再入不可 |

### lifecycle

```text
prepare(source, manifest) -> Prepared
Prepared + limits -> Runner
release Prepared          # Runnerは内部strong refにより有効
repeat:
  runner_run(snapshot, request)
  runner_report_size / runner_report_copy
  runner_clear_report または次回run
release Runner
```

契約:

- `prepare`はsource、binding manifest、必要なschemaをnative-owned memoryへcopyする。
- `runner_new`はpreparedをretainする。callerはrunnerより先にpreparedをreleaseできる。
- `runner_run`へ渡すpointerはcallの間だけborrowする。返る前に必要なbyteをcopy/parseし、保持しない。
- outputはrunnerが所有し、size/copy APIだけで読む。内部pointerを返さない。
- `runner_report_copy`は全体成功か0 byteのどちらか。部分copyしない。
- 次のrunは前reportを置き換える。保持したいcallerは先にcopyする。
- release、run、copyを同時に呼ばない。managed facadeはrunnerごとのserial gateを持つ。
- managed facadeは`SafeHandle`派生でprepared/runnerを包み、P/Invoke中のlifetimeをruntimeに保持させる。
  raw `IntPtr`をpublic APIへ出さない。
- C APIでのdouble release/use-after-releaseはcaller contract違反である。debug buildでは世代付きregistryを
  optionとして使い`STALE_HANDLE`へできるが、production ABIの安全性を無効pointer dereferenceに依存させない。

### thread contract

- 一つのrunnerへ同時に入らない。状態は`Idle -> Running -> ReportReady -> Idle`だけ。
- `Running`中の`run/copy/release`は`BUSY`。native/managed callbackは無いため正常経路で再入しない。
- preparedの論理内容はimmutableだが、異なるthreadの複数runnerから共有できるという公開保証は、
  `PreparedProgram`の`Send + Sync`と全依存をcompile-time gateで確認してから有効にする。
- runnerのthread migrationもv0で先に約束しない。Unity wrapperはrunnerを専用serial executorに束縛する。

## draft export surface

これは設計用の最小surfaceであり、実装commit前にheader fixtureでsize/alignmentを固定する。

```c
typedef struct IronVaakBytesInV0 {
    uint32_t struct_size;
    uint32_t flags;
    const uint8_t *ptr;
    uint64_t len;
} IronVaakBytesInV0;

typedef struct IronVaakBytesOutV0 {
    uint32_t struct_size;
    uint32_t flags;
    uint8_t *ptr;
    uint64_t capacity;
    uint64_t written;
    uint64_t required;
} IronVaakBytesOutV0;

uint32_t iron_vaak_v0_prepare(
    const IronVaakBytesInV0 *source_utf8,
    const IronVaakBytesInV0 *binding_manifest,
    const IronVaakBytesInV0 *limits,
    IronVaakPreparedV0 **out_prepared);

void iron_vaak_v0_prepared_release(IronVaakPreparedV0 *prepared);

uint32_t iron_vaak_v0_runner_new(
    const IronVaakPreparedV0 *prepared,
    const IronVaakBytesInV0 *limits,
    IronVaakRunnerV0 **out_runner);

void iron_vaak_v0_runner_release(IronVaakRunnerV0 *runner);

uint32_t iron_vaak_v0_runner_run(
    IronVaakRunnerV0 *runner,
    const IronVaakBytesInV0 *settings_snapshot,
    const IronVaakBytesInV0 *run_request);

uint32_t iron_vaak_v0_runner_report_copy(
    IronVaakRunnerV0 *runner,
    IronVaakBytesOutV0 *out_report);

uint32_t iron_vaak_v0_runner_clear_report(IronVaakRunnerV0 *runner);
```

設計上の注意:

- `BytesIn/Out`はP/Invoke call frame中だけのborrowであり、message recordそのものではない。
- `ptr == NULL`は`len/capacity == 0`の場合だけ許す。
- `uint64_t`からnative `usize`へ変換する前にrange checkする。
- `out_*`は成功時だけ非nullを書き、失敗時はnullのままにする。
- `prepare`診断も、失敗専用のglobal `last_error`ではなくnative-owned `PrepareReport`を返す形へ
  header fixtureで詰める。thread-local/global last errorは採らない。

## prepare once / run many

### prepare

1. source/schema/layoutのsize上限、UTF-8、wire boundsを検査する。
2. host layoutに関数slotが含まれていないことをUnity safe profileで確認する。
3. 既存`prepare`を一度だけ呼ぶ。
4. parse/check/type-check/compile診断を`Diagnostic`へ変換する。`Span`はUTF-8 byte offsetのまま保持する。
5. prepared recordへsource hash、layout hash、schema major/minor、feature setを固定する。

### run

1. runnerがIdle、request version/capability/limitsがpreparedと一致することを確認する。
2. `SettingsSnapshot`をbinding manifestに従ってVaak `Value`へcopyする。
3. 同じ`EmbeddingRunner`で実行する。runごとにsourceをparse/compileしない。
4. top-level outcome、after-state、runtime diagnosticを別々に回収する。
5. after-stateを入力と比較し、変更分を`SettingsPatch`へする。GameCommandはscriptが作った
   command bufferを全件schema検証してからreportへ入れる。
6. native-owned `ExecutionReport`を完成させてからstateをReportReadyへ移す。

runtime errorでもafter-stateをreportへ含められるようにする。Unity adapterがerror時にPatchをapplyしないなら、
それは**まだUnityへ作用を渡していないhost staging policy**であり、Vaak内状態のrollbackではない。

## panic、exception、diagnostic

- public exportの最外周を`catch_unwind`で囲み、Rust panicを`INTERNAL_PANIC`へ変換する。
  [catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)
- FFI配布crateはこの契約を実現できるpanic profileでbuildする。`panic=abort` buildは
  recoverable `INTERNAL_PANIC`を約束しないので、同じartifact名で混在させない。
- panic payloadやRust backtraceをそのまま利用者へ出さない。test/dev artifactだけに内部incident IDを付ける。
- panicしたrunnerは`Poisoned`にし、report copyとrelease以外を拒否する。preparedまでpoisonしない。
- managed exceptionをnativeへ渡さない。managed facadeはstatus/reportをcopyした後にmanaged exceptionへ
  変換してよいが、program result/アーカーシャをexceptionにしない。
- Lua error/`longjmp`はLua adapter内のprotected callで止め、Rust/.NET frameを越えさせない。
- diagnosticはglobal/thread-local `last_error`でなく、一回のprepare/runにcorrelation ID付きで所有させる。

## Unity main-thread handoff

Unityは多くのAPIをmain thread以外から安全に呼べない。Unity自身もbackground処理の後に
`Awaitable.MainThreadAsync`で戻す形を示している。
[Awaitableの完了と継続](https://docs.unity3d.com/ja/current/Manual/async-awaitable-continuations.html)

IRON VAAK Unity adapterは次の順序を守る。

```text
Unity main thread
  1. Unity objectからSettingsSnapshotへcopy（stable entity/property IDへ変換）
  2. immutable snapshotをworker queueへ渡す

IRON VAAK serial worker
  3. prepare済みrunnerをrun
  4. Patch/Command/Diagnosticをmanaged-owned bytesへcopy
  5. schema/capability/resourceをhost dataとして検査

Unity main thread
  6. snapshot revisionを再確認
  7. 全Patch/Commandをshadow stateへ適用して検証
  8. 全件成功時だけUnity objectへcommit
```

native codeは`UnityEngine.Object*`、instance IDをpointer代わりにした値、native texture pointer等を
受け取らない。entity IDはhostが世代管理する論理IDで、scene unload/object destroy後のstale generationを
main-thread adapterが拒む。

## reentrancy禁止

v0 safe profileにはnative→managed callbackが無い。状態機械は次だけを許す。

```text
Idle -> RunningVaak -> ReportReady -> Idle
Idle -> RunningAdapter(Lua等) -> PlanReady -> Idle
Idle -> ApplyingOnUnityMainThread -> Idle
```

禁止遷移:

- `RunningVaak -> RunningVaak`
- `RunningVaak -> RunningLua`
- `RunningLua -> RunningVaak`
- `Applying -> RunningVaak/RunningLua`

別runtimeを使う必要がある場合は、最初のruntimeが完全にreturnし、出力をlanguage-neutral messageへcopyし、
schedulerが次のjobを開始する。property getter/setterやcallback一件ごとの往復をしない。

## capabilityとresource limit

runner作成時に固定するhard limit:

- source/layout/schema/input/output/diagnosticの最大byte数
- messageごとの最大section/record/value node数と最大nesting depth
- runner一つの最大report byte数とlive report一件
- Patch/Command最大件数、UTF-8最大byte数
- process/hostごとのlive prepared/runner数（managed hostでも制限）

requestごとのcapability grant:

- read可能なproperty ID集合
- write可能なproperty ID集合
- 発行可能なcommand ID集合
- schema ID/version、entity scope、generation
- Patch/Command/byte数のより厳しいlimit

delegationは**attenuationだけ**である。C# hostがVaak adapterへ渡したgrantのsubsetをLua adapterへ渡せるが、
adapterはcapabilityをmintできない。capability IDは権限そのものではなく、host-owned tableを引くindex+generationで、
raw pointerやsecretをmessageへ入れない。apply時にmain-thread hostが再検証する。

現行VMにはinstruction fuelとcooperative cancellation checkpointが無い。そのためv0で確実に行えるのは:

- queueにいるrunを開始前にcancelする。
- nativeへ入った後のmanaged cancellationは結果をdiscardする印であり、runnerを並行releaseしない。
- wall-clock deadlineを観測しdiagnosticへ残すが、実行を強制停止しない。
- input/output/memory cardinalityをhard limitで拒否する。

`max_vm_steps`、in-flight cancellation、deadline checkpointはrequired feature bitとして後付けし、
意味論/APIの合意前は`UNSUPPORTED_REQUIRED_FEATURE`とする。

## platform link / 配布形

| target | native artifact / link | managed import |
|---|---|---|
| Windows Editor/Player | `iron_vaak.dll`、x64/arm64別 | `DllImport("iron_vaak")` |
| Linux Editor/Player | `libiron_vaak.so`、arch別 | `DllImport("iron_vaak")` |
| macOS Editor/Player | `.bundle`または`.dylib`/framework、universalまたはarch別 | `DllImport("iron_vaak")` |
| Android IL2CPP | `libiron_vaak.so`をABI別に同梱 | `DllImport("iron_vaak")` |
| iOS IL2CPP | static `.a` / static framework / XCFrameworkをappへlink | `DllImport("__Internal")` |

UnityはPlugin Inspectorでtarget platform/CPU architectureを指定する。native pluginはEditor session中に
unloadできないため、handle releaseとlibrary unloadを同一視しない。
[プラグインのインポートと設定](https://docs.unity3d.com/ja/current/Manual/plug-in-inspector.html)

iOSではUnityが`__Internal`を案内し、AppleはiOSのstandalone third-party `.dylib`を認めずframework内へ置く。
[Unity iOS native plugin](https://docs.unity3d.com/ja/current/Manual/ios-native-plugin-create.html)、
[Apple: Placing content in a bundle](https://developer.apple.com/documentation/bundleresources/placing-content-in-a-bundle)

AndroidはNDKのABIごとに`.so`を作る。最初のgateは`arm64-v8a`とemulator用`x86_64`で、
配布時はAABのABI splitと16 KiB page-size compatibilityを検査する。
[Android ABIs](https://developer.android.com/ndk/guides/abis)、
[16 KB page sizes](https://developer.android.com/guide/practices/page-sizes)

## crate / NuGet / UPM構成

名称はpackage registry取得可能性を確認するまで仮である。

```text
vaak                         existing MIT semantic/core crate
iron-vaak-ffi                MIT, cdylib + staticlib, only C ABI
IronVaak.Managed             .NET Standard 2.1 codec/SafeHandle/facade, no Unity ref
IronVaak.NativeAssets        NuGet RID-specific native assets
IronVaak.Unity               UPM package, Unity main-thread adapter + plugin importer metadata
IronVaak.ScriptAdapters.*    optional; Lua C API / chosen managed Lua / others
```

一般.NET向けNuGetはnative libraryを`runtimes/{rid}/native/`へ置く。
[NuGet: native files](https://learn.microsoft.com/en-us/nuget/create-packages/native-files-in-net-packages)
UnityはNuGet native asset selectionへ依存せずUPM packageでplatform/CPU別pluginを配る。
UPM packageは`Runtime/`、`Tests/Runtime`、`Samples~`、license/noticeを持つ。
[Unity package layout](https://docs.unity3d.com/ja/current/Manual/cus-layout.html)

managed facadeはreflectionを必須にせず、P/Invoke methodを静的に参照する。IL2CPP code stripping gateでは
`[Preserve]`/link metadataを最小化し、実Playerでexportが残ることを確認する。
[Unity managed code stripping](https://docs.unity3d.com/ja/current/Manual/managed-code-stripping-configure.html)

## compatibility fixture

### ABI / codec P0

- C headerをC11/C++17/Rust/C#で読み、全structの`sizeof`/`offsetof`をgoldenと照合する。
- zero length/null、最大長、`u64 -> usize` overflow、offset+length overflow、section overlapを拒否する。
- unknown optional sectionはskipし、unknown required sectionは拒否する。
- UTF-8の不正列、NULを含む正しい列、非BMP文字、byte spanを照合する。
- C# `bool`/enum/`long`がP/Invoke署名に一つも現れないことをsource gateにする。
- export一覧をgolden化し、`iron_vaak_v0_*`以外の意図しないpublic symbolを検出する。

### handle / run P0

- preparedを先にreleaseしても既存runnerが走る。
- 同じpreparedからrunnerを二つ作り、状態が混ざらない。
- 同じrunnerを1万回走らせ、prepare回数1、結果のcross-run contamination 0。
- run中の再入/同時runを`BUSY`にし、process crashやdeadlockにしない。
- output copy不足で0 byte書込み、required sizeが返る。再copyで同じhashになる。
- test-only panic injectionが`INTERNAL_PANIC`とpoisoned runnerになり、unwindがC#へ出ない。
- parse/check/type-check/compile/runtimeの各diagnostic originとUTF-8 byte spanを固定する。
- runtime errorでもafter-stateをreportへ保持し、Unity未applyのfixtureではhost stateが変わらない。

### Unity/platform P0

- Editor MonoとPlayer IL2CPPで同じsource/snapshot/report hash。
- Windows x64、macOS arm64+x64、Linux x64、Android arm64+x86_64、iOS device/simulator。
- iOS `__Internal` link、Android ABI packaging、Android 16 KiB page、macOS code signingを実機で確認。
- background run中にUnity APIを一度も呼ばず、Patch applyだけmain threadで行う。
- scene revision競合、destroy済みentity generation、型違いPatchで全件0 apply。
- managed code stripping High相当でもfacade/exportが残る。

### package P1

- NuGet RIDごとに正しい一つのnative libraryだけをresolveする。
- UPM import metadataがEditor/各Player platformを誤って混ぜない。
- Lua adapter packageを入れないprojectでLua symbol/dependencyが0。
- LICENSE/Third Party Notices/native symbol file/SHA-256/SBOMをartifactごとに生成する。

## 未決事項（意味論所有者またはhost policyの合意が必要）

1. Unity safe profileで`HostFn`を永久に禁止するか、S-11第二段のsuspend/resume完成後に
   明示capabilityとして許すか。
2. VM step budget/cancellation checkpointがVaak programの観測可能な結果へ与える扱い。
3. `PreparedProgram`のcross-thread共有と`EmbeddingRunner`のthread migrationを公開保証にするか。
4. runtime error時のafter-state/PatchをIRON VAAK Unityの既定policyでapplyするかdiscardするか。
   native reportはどちらの情報も失わない。
5. top-level value/アーカーシャをUnity facadeがどう解釈するか。C-31によりhost policyである。
6. 同じsnapshotへVaak/Lua/他adapterが作ったPatchのconflict orderingとGameCommandのdelivery guarantee。
7. instruction budgetをcore VMに置くか、外側runnerだけに置くか。実装前に参照実装/VM一致を要する。

## 一次資料一覧

- Microsoft: [Native interoperability best practices](https://learn.microsoft.com/en-us/dotnet/standard/native-interop/best-practices)
- Microsoft: [Native library loading](https://learn.microsoft.com/en-us/dotnet/standard/native-interop/native-library-loading)
- Microsoft: [SafeHandle](https://learn.microsoft.com/en-us/dotnet/fundamentals/runtime-libraries/system-runtime-interopservices-safehandle)
- Microsoft: [Calling conventions](https://learn.microsoft.com/en-us/dotnet/standard/native-interop/calling-conventions)
- Microsoft/NuGet: [Native files in packages](https://learn.microsoft.com/en-us/nuget/create-packages/native-files-in-net-packages)
- Unity: [IL2CPP](https://docs.unity3d.com/ja/current/Manual/il2cpp-introduction.html)
- Unity: [Native plug-ins](https://docs.unity3d.com/ja/current/Manual/plug-ins-native.html)
- Unity: [Plugin Inspector](https://docs.unity3d.com/ja/current/Manual/plug-in-inspector.html)
- Unity: [iOS native plugin](https://docs.unity3d.com/ja/current/Manual/ios-native-plugin-create.html)
- Unity: [Awaitable continuation/main thread](https://docs.unity3d.com/ja/current/Manual/async-awaitable-continuations.html)
- Unity: [.NET profiles](https://docs.unity3d.com/ja/current/Manual/dotnet-profile-support.html)
- Apple: [Placing content in a bundle](https://developer.apple.com/documentation/bundleresources/placing-content-in-a-bundle)
- Android: [Android ABIs](https://developer.android.com/ndk/guides/abis)
- Android: [JNI tips](https://developer.android.com/ndk/guides/jni-tips)
- Rust: [FFI](https://doc.rust-lang.org/nomicon/ffi.html)
- Rust: [`catch_unwind`](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)
