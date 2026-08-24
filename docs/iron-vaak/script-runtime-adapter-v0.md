# IRON VAAK generic embedded script adapter v0

状態: implementation前のadapter contract  
調査日: 2026-08-24

## 2026-08-25 codex3 implementation override

以下は製品別adapterまでを見据えた初期設計記録である。現在の実装名は`IScriptPlanRuntime`、
`ScriptInvocation`、`ScriptPlan`、`ILuaPlanExecutor`、`LuaScriptAdapter`、`ScriptPlanCoordinator`である。
`ScriptPlanCoordinator`の実装済みv0 policyは、同一immutable Snapshotから得た全planをall-or-noneで扱い、
同じpropertyへの複数writeを値が同じでもrejectするstrict mergeである。将来のstaged mode、Command、
PUC-Lua C adapter、製品別managed Lua adapterは未実装であり、以下の未決事項を残す。

外部assemblyはpublic `ScriptPlan(runtimeId, succeeded, patch)`からgeneric runtimeのcanonical planを返せる。
Lua固有runtimeは`ILuaPlanExecutor`からowned Patch wireを返し、identity/canonical検査後に同じ合成経路へ入る。

## 結論

Unity内でVaakとLua等を併用するときも、runtime同士をcallbackで直結しない。

```text
C# host scheduler
  -> immutable SettingsSnapshot + attenuated CapabilityGrant
  -> one adapter runs to completion
  <- SettingsPatch + GameCommand + Diagnostic
  -> validate/copy/stage
  -> next adapter job (必要な場合だけ。前のcall stackは既に無い)
  -> Unity main-thread atomic apply
```

`C# -> Vaak -> Lua -> Vaak`のようなnested callは作らない。Vaak実行中のproperty getterがLuaを呼ぶ形、
Lua metamethodがC#を通じVaakへ戻る形、native callbackがmanaged delegateを呼ぶ形を禁止する。

Luaはoptional adapterの一つであり、Vaak意味論、IRON VAAK managed core、native FFIの依存ではない。

## 適用範囲

このcontractは次へ共通に適用する。

- PUC-Lua 5.4 C APIをC shimで埋め込むadapter
- C#だけで実装されたmanaged Lua runtime adapter
- Python/Wren/JavaScript等、別のgeneric embedded scripting runtime adapter
- test用fake adapter

各runtimeの言語意味論やobject modelはadapter内部だけに閉じる。

## 共通managed contract

概念上の最小interfaceは次である。実際のC# targetはUnity向け.NET Standard 2.1とする。

```csharp
public interface IIronVaakScriptAdapterV0 : IAsyncDisposable
{
    string AdapterId { get; }
    AdapterFeatureSetV0 Features { get; }

    ValueTask<ScriptPlanV0> EvaluateAsync(
        ScriptBatchV0 input,
        CancellationToken cancellationToken);
}
```

`ScriptBatchV0`はimmutable managed-owned bytesとschema/session/run IDを持つ。
`ScriptPlanV0`は新しいmanaged-owned `SettingsPatch` / `GameCommand` / `Diagnostic` bytesを持つ。

contract:

1. `EvaluateAsync`一回につきsnapshot一件をbulkで渡す。
2. adapterはinputを変更しない。保持する場合は明示copyし、budgetへ計上する。
3. outputにruntime object、delegate、pointer、stack indexを入れない。
4. adapterはhostが渡したcapabilityのsubsetだけを使用できる。新しいcapabilityを作らない。
5. adapterはUnity APIを呼ばない。Unity main-thread adapterだけがapplyする。
6. adapterは同じscheduler/runnerへ再入しない。別adapterを直接呼ばない。
7. errorをthrow/longjmpのまま境界へ出さず、provenance付き`Diagnostic`へcopyする。
8. cancellationはcontractで分類し、強制thread abortやrunnerの並行releaseをしない。

## scheduler state machine

一sessionのschedulerは次の一状態だけを持つ。

| state | 許される操作 |
|---|---|
| `Idle` | job enqueue、snapshot refresh、dispose |
| `RunningVaak` | native reportを待つだけ |
| `RunningLuaC` | C shim returnを待つだけ |
| `RunningManagedScript` | managed adapter returnを待つだけ |
| `ValidatingPlan` | wire/schema/capability/conflict検査 |
| `ReadyToApply` | main thread queueへ渡す |
| `Applying` | Unity main threadでshadow validate/commit |
| `Poisoned` | diagnostic取得とdisposeだけ |

どの`Running*`からも別`Running*`へ直接遷移しない。必ずreturnしてmanaged-owned messageへcopyし、
`Idle`または`ValidatingPlan`へ戻る。thread-localな`bridge_depth`を診断用に持ち、1を越えるentryは
`REENTRANT_CALL`で拒否する。ただしsecurityはthread-localだけに依存せずsession stateでも検査する。

## 複数runtimeの合成形

### A. 独立plan（推奨default候補）

同じimmutable snapshotを各adapterへ別jobとして渡し、全planがreturnした後にhostがmergeする。

```text
Snapshot R
  -> Vaak plan V
  -> Lua plan L
host validates V and L together
  -> conflictなら0 apply + Diagnostic
  -> conflict無しならone commit
```

利点はruntime順序が計算結果へ入りにくく、callback/reentryが無いこと。Patch conflict policyは未決事項で、
v0 schemaは黙ったlast-writer-winsを許さない。

### B. staged derived snapshot

一adapterのplanを実体へapplyせずshadow stateへだけ反映し、そのderived snapshotを次adapterへ渡す。

```text
Snapshot R -> Lua plan -> validate into shadow R' -> Vaak plan -> combined commit
```

各adapterのcall stackは完全に終了している。順序はmanifestで明示し、run reportへ残す。
実体へのcommitは最後の一回だけである。

### C. request/response next-tick

別runtimeの計算が必要なら`GameCommand`/request dataを返し、hostが次のscheduler turnで応答snapshotを作る。
同期callbackを追加しない。correlation/run IDで対応させる。

## property contract

- property accessは`SettingsSnapshot`のread、`SettingsPatch`のwriteだけ。
- getter/setterをFFI callbackとして公開しない。
- snapshotに無いpropertyは見えない。Lua `nil`やC# nullを「permission denied」の代用にしない。
- entity/property IDはmanifestで型付けされ、Unity object referenceではない。
- Patchはexpected revisionを持ち、main threadでstale/conflictを検査する。
- 複数property updateは一batch。adapterが一件ずつC#へcallしない。

## callback contract

v0 safe profileのcallbackは**0件**である。

| 要求 | v0での表現 |
|---|---|
| host eventでscriptを走らせたい | hostがprepared adapter jobをenqueue |
| scriptからhostへ作用したい | GameCommand batchを返す |
| hostから値を問い合わせたい | 次Snapshotへ入れる、または別turnのresponse data |
| 別script runtimeへ問い合わせたい | schedulerへrequest commandを返し、別jobにする |
| progress/diagnostic | bounded Diagnostic batchを返す |

同期`HostFn`を使う既存Vaak APIは言語として残るが、このUnity/generic adapter profileへ露出しない。
再入が要る`HostFn`を将来公開するなら、S-11第二段のsuspend/resumeと別feature negotiationが前提である。

## Lua 5.4 C API adapter

Lua manualによれば、C APIは`lua_State*`とvirtual stack/indexを使う。error/yieldはC `longjmp`を使う場合があり、
`lua_pcall`/`lua_pcallk`がprotected boundaryである。
[Lua 5.4 Reference Manual: C API](https://www.lua.org/manual/5.4/manual.html#4)

### isolation

```text
C# scheduler
  -> fixed C adapter ABI (wire bytes only)
  -> C-owned LuaAdapter instance / lua_State
       - save stack top
       - decode snapshot according to manifest
       - push one entry function and one input value
       - lua_pcall (yield disabled in v0)
       - encode returned plan completely
       - restore stack top
  <- wire bytes / Diagnostic
```

規律:

- `lua_State*`はC adapter instanceだけが持つ。Rust FFI、C#、wire messageへ出さない。
- stack indexは一C call内だけ。absolute indexもreturn後に保存しない。
- Lua value/table/function/userdata/threadをwireへ入れない。schemaに従い全てcopyする。
- Lua registry refやlight userdataをentity/capability IDに転用しない。
- Lua C APIをRust frameから直接呼んでunprotected `longjmp`を跨がせない。C shim内のprotected callで止める。
- v0 entryはyield不可。manualが示すようにyieldはC frameを`longjmp`で外しcontinuation APIを要するため、
  scheduler contractを別に決めるまで`lua_isyieldable`の可否に関係なく拒否する。
- entry前のstack topを記録し、成功/errorの両方で回復する。stack growth上限をbudgetにする。
- `lua_pcall`のerror object/tracebackをbounded UTF-8 Diagnosticへcopyし、stackから除く。
- Lua allocatorにper-session byte budgetを渡し、allocation failureをadapter originのdiagnosticへする。
- Lua debug hookでinstruction budgetを実装する場合はLua adapter固有featureであり、Vaak VM fuelと同一視しない。
- 一つの`lua_State`へ同時に入らない。adapterをsingle-thread serial executorへ束縛する。

Lua C APIのstackは呼出しごとに独立し、C側はstack capacity管理に責任を持つ。yieldはC `longjmp`を使う。
[Lua manual §4.1 Stack](https://www.lua.org/manual/5.4/manual.html#4.1)、
[§4.4 Error handling](https://www.lua.org/manual/5.4/manual.html#4.4)、
[§4.5 Yields](https://www.lua.org/manual/5.4/manual.html#4.5)

### error provenance

| Lua status | Diagnostic code/origin |
|---|---|
| `LUA_ERRRUN` | `SCRIPT_RUNTIME_ERROR` / `lua-c-api` |
| `LUA_ERRSYNTAX` | `SCRIPT_PREPARE_ERROR` / `lua-c-api` |
| `LUA_ERRMEM` | `SCRIPT_RESOURCE_EXHAUSTED` / `lua-c-api` |
| `LUA_ERRERR` | `SCRIPT_ERROR_HANDLER_FAILED` / `lua-c-api` |
| adapter decode/encode | `ADAPTER_CONTRACT_ERROR` / `lua-c-api` |
| yield attempt | `UNSUPPORTED_SUSPENSION` / `lua-c-api` |

Lua error objectのtype/to-string処理自体が失敗し得るため、最終fallbackは定数message+statusだけにする。

## managed Lua adapter

managed Lua実装には共通の標準APIが無いため、特定製品のobject modelをこのcontractへ入れない。

規律:

- adapter packageが対象製品を依存に持ち、IRON VAAK coreは持たない。
- `ScriptBatchV0`を製品のtable/valueへschema-driven copyし、返値もwireへcopyしてからruntime objectを解放する。
- delegate/event/property callbackからnative Vaakへ入らない。host interactionはplan returnだけ。
- managed exceptionはadapter内で捕捉し、type名/stack traceをsize制限してDiagnosticへcopyする。
- exception object、`GCHandle`、pinned array pointerをnativeへ保持させない。
- IL2CPP/AOTでdynamic code generation、`Reflection.Emit`、未保存generic specializationを要求しない。
- reflectionが必要な製品はUnity linker用preservationをadapter package側で明示し、実Player fixtureを持つ。
- runtimeがthread-affineならadapter executorも同じthreadへ固定し、`Features`で宣言する。
- cancellationは製品がsafe checkpointを持つ場合だけcooperative。無い場合はrun完了後discardである。

Unity IL2CPPはAOTで、reflection経由の到達をlinkerが推測できない場合にcodeがstripされ得る。
[Unity scripting restrictions](https://docs.unity3d.com/ja/current/Manual/scripting-restrictions.html)、
[Preserving code](https://docs.unity3d.com/ja/current/Manual/managed-code-stripping-preserving.html)

## other generic runtime adapter

新しいruntime adapterは次のmanifestを答える。

| field | 内容 |
|---|---|
| adapter ID/version | stable UTF-8 ID + semantic version |
| supported exchange major/minor | required/optional section |
| thread model | single-thread / serialized-migratable / dedicated main thread |
| suspension | none / cooperative（v0 schedulerはnoneだけrequired） |
| cancellation | before-start / discard-after / cooperative checkpoint |
| memory budget | hard/observed/unsupported |
| instruction budget | hard/observed/unsupported |
| deterministic mode | supported featureと制約 |
| required runtime/package | optional dependencyとlicense/notice |

unsupported featureをsilent fallbackしない。hostがrequiredとして要求した機能が無ければjob開始前に拒否する。

## ownership / lifetime

| object | owner | lifetime |
|---|---|---|
| SchemaManifest | C# host/session | prepared adaptersより長いimmutable copy |
| SettingsSnapshot bytes | scheduler | 全adapter jobがcopy/returnするまで |
| CapabilityGrant | host table | session/generationまで。adapterはborrowed subset |
| Vaak PreparedProgram | native SafeHandle | one or more Vaak runnersがretain |
| Vaak runner | one serial executor | run/copy/report、then dispose |
| Lua state | Lua C adapter | adapter disposeまで。外へ出さない |
| managed Lua runtime object | managed adapter | adapter内部。nativeへ出さない |
| Patch/Command/Diagnostic | producing adapter then scheduler | managed-owned copyとしてapply/report完了まで |

borrowed inputをasync suspension中に保持するadapterは、Evaluate開始時に自分のbudgetでcopyしなければならない。
pinningを長期ownershipの代用にしない。

## capability delegation

```text
Unity/application authority
  -> session grant G
     -> Vaak gets attenuated Gv
     -> Lua gets attenuated Gl
     -> other adapter gets Ga
```

- `Gv ∪ Gl ∪ Ga`がGを越えない。
- adapter Aはadapter Bへgrantを直接渡さない。schedulerだけがdelegationする。
- Patch/Command recordは使用したcapability indexを持つ。
- scheduler validateとUnity applyの両方でscope/generation/budgetを再検査する。
- revoke後のqueued jobは開始前にcancel。in-flight planはreturn後にstale generationとしてrejectする。

## resource budget

共通hard budget:

- input/output/diagnostic byte数
- record/value node/string/collection件数とnesting depth
- Patch/Command件数
- adapterごとのlive state/session数
- queue depthと一frameのmain-thread apply件数

runtime固有budget:

| runtime | memory | instruction/time | v0の安全な扱い |
|---|---|---|---|
| Vaak | input/output/value node hard limit | VM fuelは未実装 | before-start cancel、完了後discard、経過時間観測 |
| Lua C API | custom allocatorでhard byte limit可 | debug hookはoptional | `lua_pcall`内でadapter固有limit、yield禁止 |
| managed Lua | 製品依存 | 製品依存 | unsupportedを明示。thread abort禁止 |
| generic adapter | manifestで宣言 | manifestで宣言 | required feature不足なら開始しない |

wall-clock timeoutでnative handleを別threadからreleaseしない。process isolationが必要なuntrusted runtimeは、
in-process adapter v0とは別profileとして設計する。

## cancellation

| phase | behavior |
|---|---|
| queue中 | jobを除去し`cancelled-before-start` report |
| snapshot作成中 | main-thread hostが安全なcheckpointで停止 |
| Vaak native run中 | v0は強制停止しない。flagを立て、return後reportをdiscard |
| Lua protected call中 | adapterがsafe hook対応ならcooperative、無ければreturn後discard |
| validation前 | planをdiscard |
| Unity apply開始後 | atomic shadow validation前なら停止可。commit開始後はhost transaction policy |

Diagnosticはcancellation origin、request/run ID、観測phaseを持つ。program runtime errorと混ぜない。

## error provenance chain

例:

```text
Diagnostic 41: UNITY_APPLY_FAILED (entity revision conflict)
  caused by 32: PATCH_VALIDATION_FAILED (stale expected revision)
    caused by 18: LUA_PLAN_PRODUCED (adapter lua-c-api, run R)
```

各adapterは自分のoriginだけを発行し、他runtimeのerrorを自分のerror文字列へ潰さない。
hostがラップする場合は新Diagnosticを作り`cause_id`で結ぶ。

## fixture matrix

### 共通adapter contract P0

| fixture | fake | Vaak | Lua C API | managed Lua | other |
|---|---:|---:|---:|---:|---:|
| snapshot→empty plan | ✓ | ✓ | ✓ | ✓ | ✓ |
| scalar/UTF-8/record roundtrip hash | ✓ | ✓ | ✓ | ✓ | ✓ |
| malformed wire拒否 | ✓ | ✓ | ✓ | ✓ | ✓ |
| capability外read/write/command拒否 | ✓ | ✓ | ✓ | ✓ | ✓ |
| byte/record/depth limit | ✓ | ✓ | ✓ | ✓ | ✓ |
| cancellation before start | ✓ | ✓ | ✓ | ✓ | ✓ |
| in-flight cancel contract宣言 | ✓ | ✓ | ✓ | ✓ | ✓ |
| Diagnostic origin/cause維持 | ✓ | ✓ | ✓ | ✓ | ✓ |
| adapter無しcore起動 | ✓ | ✓ | — | — | — |

### reentrancy P0

- Vaak job中に同じrunnerをcallし`BUSY`。
- Lua C functionがschedulerへ同期entryを試み`REENTRANT_CALL`。
- managed Lua event/delegateがVaak entryを試み`REENTRANT_CALL`。
- Unity apply中にscript jobを同期開始せず、次turnへqueueする。
- A→B→A cycle requestをcorrelation graphで検出し、job開始前に拒否する。
- runtime return後に次runtimeを走らせるstaged modeは許可し、call depth最大1をtraceで確認する。

### Lua C API P0

- success/errorの両方でstack top一致。
- `lua_pcall`の`LUA_ERRRUN/ERRMEM/ERRERR`を別codeへする。
- error handler自身のerrorでもbounded fallback diagnosticが返る。
- yield/coroutine attemptをunsupportedとして拒否し、C/Rust/.NET frameを`longjmp`が越えない。
- Lua table cycle、metatable side effect、oversized string/table、allocator exhaustionを拒否する。
- registry ref/light userdata/stack indexがwireに現れないことをbinary/source gateにする。
- 同じLua stateへのconcurrent entry 0。

### managed Lua / IL2CPP P0

- Editor MonoとPlayer IL2CPPで同じplan hash。
- code stripping High相当でentry/schema converterが残る。
- runtimeが使うgeneric type/serializerをAOT Playerで全fixture実行する。
- managed exception/aggregate exception/cancellationをprovenance付きDiagnosticへする。
- pinned buffer/GCHandle countがrun後baselineへ戻る。
- managed Lua packageを除いたbuildにassembly/native symbol依存が無い。

### composition P0

- Vaak/Luaが異なるpropertyを書くとone atomic commit。
- 同じpropertyを書くとpolicy未指定時はconflictで0 apply。
- staged derived snapshotはmanifest順とrun IDsをreportへ残す。
- command idempotency key重複をhostが検出する。
- adapter一つがerror/timeoutでも他adapterのplanをapplyするかは明示policyで、silent partial applyしない。
- scene unload/entity generation更新後の全planをstaleとして0 apply。

## packaging

- `IronVaak.Managed`と`IronVaak.Unity`はLuaに依存しない。
- Lua C API adapterはnative Lua library、license、architecture別artifactを自分のoptional packageで持つ。
- managed Lua adapterは対象製品ごとに別packageにし、version/license/IL2CPP support matrixを記録する。
- generic adapter SDKはexchange codecとinterfaceだけを参照し、Unity assemblyを要求しない。
- Unity UPMの`Samples~`にfake adapter合成例を置き、実Luaをinstallしなくてもcontractを試せるようにする。

Lua 5.4自体はMIT licenseだが、採用するbinary配布物とmanaged Lua製品は個別にlicense/noticeを監査する。
[Lua 5.4 README / license](https://www.lua.org/manual/5.4/readme.html)

## 未決事項

1. 複数adapter planのdefault conflict/order policy。
2. runtime error/cancellationが一adapterに出たとき、他adapterの成功planをapplyするかall-or-noneにするか。
3. staged modeの順序をmanifestで固定するか、applicationがrunごとに選べるか。
4. GameCommandのat-most-once/idempotency/outbox retention期間。
5. Lua coroutine/yieldを将来別turn suspensionとしてsupportするか。v0は明示非対応。
6. managed Luaの最初の公式support対象製品。一次資料、license、AOT、thread/cancellationを製品別監査してから選ぶ。
7. untrusted scriptをin-processで扱うか、process/WASM sandbox profileへ送るか。v0 native adapterはsecurity sandboxではない。
8. VaakのVM fuel/suspend-resumeをadapter共通budgetへ接続する方法。意味論判断なしに実装しない。

## 一次資料

- Lua: [Lua 5.4 Reference Manual](https://www.lua.org/manual/5.4/manual.html)
- Lua: [C API stack](https://www.lua.org/manual/5.4/manual.html#4.1)
- Lua: [Registry](https://www.lua.org/manual/5.4/manual.html#4.3)
- Lua: [Error handling in C](https://www.lua.org/manual/5.4/manual.html#4.4)
- Lua: [Handling yields in C](https://www.lua.org/manual/5.4/manual.html#4.5)
- Android: [JNI tips](https://developer.android.com/ndk/guides/jni-tips) — `JNIEnv`はthread-local、local refはnative call中だけという同型のlifetime規律
- Unity: [IL2CPP introduction](https://docs.unity3d.com/ja/current/Manual/il2cpp-introduction.html)
- Unity: [Scripting restrictions](https://docs.unity3d.com/ja/current/Manual/scripting-restrictions.html)
- Unity: [Awaitable main-thread continuation](https://docs.unity3d.com/ja/current/Manual/async-awaitable-continuations.html)
- Microsoft: [Native interop best practices](https://learn.microsoft.com/en-us/dotnet/standard/native-interop/best-practices)
