# LVMINIBVS向け候補評価の受領記録

受領日: 2026-08-25

対象: `codex3/iron-vaak-dotnet` (`4f0b5ea`、後続のroadmap-only commit `0f737b9`)

位置づけ: 外部の埋め込み候補評価。IRON VAAKの規範、採用決定、production保証ではない

## 判定

LVMINIBVS側の評価では、IRON VAAKは「安全に呼ぶ入口が無い」段階を越え、adapter候補として
具体的な接続fixtureを作れるところまで来た。一方で、確認対象はLinux x64の縦切りであり、
LVMINIBVSへの採用・接続はまだ行わない。

この記録は評価文から、観測結果、設計判断、採用gateだけを要約したものである。LVMINIBVSや
PraTeXのsource、test、schemaはVaak repositoryへ運んでいない。

## 外部再観測で報告された範囲

評価者はcommit `4f0b5ea`の一時worktreeで`./scripts/check-iron-vaak.sh`を実行し、次を報告した。

- `iron-vaak-ffi` unit test 18件とC header fixture 1件
- `iron-vaak-native` test 2件
- C11 / C++17 header syntax checkと16個のnative export symbol照合
- Linux x64 native library staging
- `.NET Standard 2.1` / `.NET 8` build（warning 0、error 0）とNuGet pack
- managed smoke 11件
- script全体の終了code 0

branch文書にある.NET 8 Native AOTの結果は、この再観測では再実行されていない。Unity IL2CPPの
代替証拠にも数えない。後続の`0f737b9`はIRON JIT VAAKを長期候補へ加えた文書変更だけなので、
上の実行結果や採用判定へ加算しない。

## 適合している境界

- sourceを変更時にprepareし、play中はrunnerを再利用する細い経路
- Unity objectやLua runtimeをVaakへ渡さず、host所有のimmutable Snapshotからowned Patchを返す形
- `SafeHandle`、世代付きnative handle、panic containment、同一runnerへの再入拒否
- LuaとVaakを直接相互callさせず、同じsnapshotから独立planを作ってhostで検証・commitする形
- 候補探索、有限集合、順位付け等をtyped compute kernelへ分離する用途

これはLuaを置き換える一般script runtimeの評価ではない。Luaはevent flowや設定を担当し、Vaakは
短命な計算kernelを担当できる。両者のcall stackを同期的に入れ子にしない。

## 接続前に解く意味差

LVMINIBVSの現行addon pipelineは、前moduleのpayloadを次moduleへ渡す順次filterである。一方、
`ScriptPlanCoordinator`の実装済みpolicyは、同じsnapshotから独立に得たplanを一括mergeする。
adapterがこの違いを隠してはならない。

typed hook codecは少なくとも次を明示する。

- `SequentialFilter`: manifestに固定した順序でshadow payloadを次段へ渡す
- `IndependentPlans`: 同じrevisionのsnapshotからplanを作り、全件を検証して一回だけcommitする

property identity、canonical codec、revision、競合、失敗時のapply範囲をhook schemaへ固定するまで、
既存の`IAddonRuntime`へ直結しない。IRON VAAKのstrict conflict policyを、先にLVMINIBVS addonの
一般則だと宣言しない。

## 採用gate

### 資源停止

- untrusted programを決定的に止めるhard fuel
- program、runner、host value、input/outputを含む完全なmemory accounting
- panic、nested/concurrent entry、handle exhaustion、cancelのbounded fixture

wall-clock timeoutはgameplay分岐の決定的なfuelとして扱わない。上限を対象buildで実証するまでは
first-party trusted experimentに限り、`SandboxedVaak`とは呼ばない。

### wireとtransaction

- aggregate host value、Command batch、capability grant
- bounded Diagnostic / ExecutionReport
- schema、session、run、transaction、base revisionの照合
- stale、capability違反、競合、一runtime失敗時の0 apply
- main thread上のvalidate-on-shadowと一回だけのcommit

最初はscalar-only fixtureでproperty identity、revision、strict conflict、0 applyを固定する。

### Unityと配布

- Unity 6 Editor / Player、Mono / IL2CPP、対象OSの同一出力fixture
- Windows、macOS、Linuxと、採用対象mobileのnative artifact
- `.meta` import設定、domain reload、Scene世代変更、headless build
- artifact hash、署名方針、再現build、pinned release、license / third-party notice
- fuzz、sanitizer、長時間soakとmanaged/native memoryの非増加確認

### Lua製品

製品非依存の`ILuaPlanExecutor`だけをLua製品対応とは数えない。PUC-Lua、MoonSharp、xLua等の
候補は、license、AOT/stripping、allocator、instruction上限を同じfixtureで比較してから選ぶ。
Lua artifactが無いbuildでもIRON VAAK coreが成立しなければならない。

## 先行pilot

production接続より先に実験assemblyで、canonical round-trip、malformed/oversized拒否、ID/revision
不一致、stale/capability違反、strict conflict、runtime error時の0 apply、再入拒否、Scene世代変更、
runtime不在時のfail-closedを固定する。

外部向けの最初の計算fixture候補は、有限解釈maskから次の操作を順位付けするhintである。
Vaakは各操作後の最悪残存数を最小にし、同点ならstable IDで決める。C# oracle、fake Lua、Vaakを
全有限入力で照合し、frontendは候補表示だけを行う。stage進行、save、world状態は変更しない。

このfixtureは採用前に「採用済みsample」として公開しない。公開する場合はsource、oracle、全入力test、
artifact hash、fallback、license、再現手順を一緒に固定する。

## roadmapへの反映

短期の優先順は次とする。

1. typed hook codecと`SequentialFilter` / `IndependentPlans`の明示
2. scalar-only LVMINIBVS pilotとC# / fake Lua / Vaakの同一fixture
3. hard fuelと完全なmemory accountingの公開契約
4. aggregate / Command / capability / Diagnostic wire
5. Unity Editor / Player、Mono / IL2CPP、対象OS matrix
6. artifact、license、fuzz、soakを含むrelease gate

長期候補のIRON JIT VAAKはこのgateを迂回しない。VM fallbackと同じSnapshot/Patch契約、fuel、
memory budget、artifact identity、差分fixtureを満たした後にだけ別途評価する。
