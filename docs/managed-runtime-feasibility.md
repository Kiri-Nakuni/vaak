# Managed runtime VM の実現性

**IRON VAAK（pure C#/.NET）と JVM 版（名称未決定）の実装前調査**

- 調査日: 2026-08-24
- 基点: `7c5ccd706e4dc466dad45baf275c0b550c8bc777`
- 状態: **非規範的な設計 checkpoint。Vaak の意味論、portable bytecode の opcode、ABI、S-n を決めない。**

## 1. 結論

pure C# の IRON VAAK VM と、pure Java または Kotlin/JVM の VM は、どちらも実現可能である。
Vaak bytecode を読むのは managed runtime 自身が書いた interpreter loop であり、利用者の台本を
CLR IL や JVM class へ実行時生成する必要はない。したがって JIT、`System.Reflection.Emit`、
動的 class 生成、native Rust library、WASM runtime のいずれにも依存しなくてよい。

| | IRON VAAK | JVM 版 |
|---|---|---|
| 通常 desktop/server | .NET library として実行可能 | JAR として実行可能 |
| Unity | pure C# package として Mono / IL2CPP の両方へ置ける | Unity の主経路にはしない |
| iOS | Unity IL2CPP、または .NET 9+ Native AOT で可能 | JVM artifact は不可。Kotlin/Native なら**別 target**として可能 |
| Android | Unity IL2CPP / .NET for Android で可能 | ART 上の Java/Kotlin library が自然 |
| JIT 非依存 | static opcode dispatch だけで可能 | static opcode dispatch だけで可能 |
| native asset | core には不要 | core には不要 |

.NET Native AOT は runtime code generation と動的 assembly loadingを持たず、AOT 互換性解析を
提供する。[Microsoft: Native AOT](https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/)
Unity IL2CPP も C# をIL、C++、platform native binaryの順に変換するAOT backendであり、iOSのように
JITを使えないplatformを主要用途に含む。[Unity: IL2CPP](https://docs.unity3d.com/jp/current/Manual/il2cpp-introduction.html)

AndroidではJava classがD8によってDEXへ変換され、ARTがinterpreter、JIT、AOTを管理する。
Vaak VM自身はARTの方針に依存せず、通常のJava codeとして動かせる。
[Android platform architecture](https://developer.android.com/guide/platform)

ただし「同じ意味を別言語で再実装できる」ことと「自然に同じ意味になる」ことは別である。
Rust、C#、Javaの整数除算、overflow、float、container order、例外はそのまま使わず、既存の
Vaak規則を共通fixtureで固定する必要がある。

## 2. この調査が守る既存契約

この文書は次の既存判断をmanaged runtimeへ写す。新しい判断ではない。

- **S-5**: 木を辿るRust実装が参照実装である。VMが食い違えばVM側を直す。
- **C-90**: 記憶の意味上の寿命は領域ごとのarenaであり、個別destructorを走らせない。
- **C-95**: hostは値の実体を公開せず、実行前のsnapshotと実行後のwritebackを仲介する。
- **C-31**: 最上位外界面の解釈はhostの仕事である。
- **C-2 / S-22**: 直接HostBinding車線では、runtime errorより前のwritebackを捨てない。
- **S-11**: hostの呼べる名前は型付きで事前登録する。第一級callbackやclosureにはしない。
- **C-33 / C-48**: 値は深く複製され自己完結し、aliasは値の中へ入らない。

`docs/experiments/embedding.md`にある
`snapshot -> compute -> command buffer -> validate -> commit`は、C-2を変更するtransactionではない。
hostへまだ作用を渡していないbatchをhostが検証してから適用する、別の車線である。

## 3. 全体像

```text
Vaak source
    |
    | 当面のcanonical compiler
    v
Rust lexer / parser / check / type-check / bytecode compiler
    |
    | versioned, language-neutral portable bytecode
    +------------------------+------------------------+
    |                        |                        |
    v                        v                        v
Rust VM                 IRON VAAK VM             JVM VM
    |                        |                        |
    +------------------------+------------------------+
                             |
                    differential result corpus

host snapshot + capabilities
             |
             v
      prepared program + per-run Runner
             |
             v
 outcome / patch / command batch + provenance
             |
             v
       host validates and commits
```

WASMは追加の配布・sandbox車線にはできるが、managed applicationの主経路にしない。
pure managed VMなら、Unity IL2CPP/iOSでWASM runtimeを埋める必要も、managed/native境界を
命令ごとに越える必要もない。

将来managed compilerを作っても、まず同じportable bytecodeを出すfront endとする。
managed compilerが独自VM内部表現へ直結すると、compilerとVMの食い違いを切り分けられない。

## 4. IRON VAAK: pure C# VM

### 4.1 AOTで守る実装規律

IRON VAAK coreは最初から次を禁止する。

- `System.Reflection.Emit`、expression treeのruntime compile
- `Assembly.Load*`とplugin classの動的発見
- opcode名からreflectionでhandlerを探すこと
- reflection任せのserializer/deserializer
- runtimeにしか分からないclosed generic typeの生成
- native P/Invoke、Rust `cdylib`、platform別native asset

Native AOTではdynamic loadingとruntime code generationが使えず、trimmingも必須になる。
[Native AOT limitations](https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/#limitations-of-native-aot-deployment)
`<IsAotCompatible>true</IsAotCompatible>`はtrim、single-file、AOT analyzerを有効にする。
[AOT-compatible libraries](https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/#aot-compatibility-analyzers)

bytecode decoderはbounds check付きの`byte[]` reader、VMは`switch (opcode)`、host capabilityは
compile時に確定した整数indexで扱える。portable bytecodeのendianはruntime native endianへ
委ねず明示する。.NETには特定endianを読む`BinaryPrimitives`があるが、Unity互換baselineでは
同じ処理を小さな`byte[]` helperに閉じてもよい。
[BinaryPrimitives](https://learn.microsoft.com/en-us/dotnet/api/system.buffers.binary.binaryprimitives)

### 4.2 Unity IL2CPP

Unity 6の既定cross-platform API profileは.NET Standard 2.1であり、managed pluginとして
.NET Standard assemblyを読める。[Unity .NET profile support](https://docs.unity3d.com/ja/current/Manual/dotnet-profile-support.html)
IL2CPP buildはmanaged assemblyをstripしてからC++へ変換し、platform compilerでnative binaryを作る。
[Unity IL2CPP build stages](https://docs.unity3d.com/jp/current/Manual/il2cpp-introduction.html#how-il2cpp-works)

reflectionをcoreから除けば、UnityLinker用`link.xml`をcoreの正常動作に必須とせずに済む。
UnityLinkerはreflection経由の参照を常には検出できないため、reflection adapterを将来足す場合だけ
静的登録または明示preservationをadapter側に置く。
[Unity managed code stripping](https://docs.unity3d.com/es/current/Manual/ManagedCodeStripping.html)

IL2CPP gateはEditor上のMono実行だけでは足りない。最低限次をCIまたは手動release gateにする。

1. Windows/macOSのIL2CPP player
2. Android arm64 APK/AAB
3. iOS arm64 player
4. managed stripping `Medium`と`High`
5. development buildとrelease buildで同じconformance digest

### 4.3 .NET iOS / static AOT

.NET 9以降のNative AOTはiOS-like platformを対象にでき、OS非依存libraryも`ios-arm64`等へ
publishできる。[Native AOT for iOS-like platforms](https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/ios-like-platforms/)
.NET MAUIのiOS releaseはAOTを前提とし、Native AOTはruntime code generationなしの単一native binaryを
作る。[.NET MAUI runtimes and compilation](https://learn.microsoft.com/en-us/dotnet/maui/deployment/runtimes-compilation)

AppleのApp Review Guideline 2.5.2は、機能を変えるcodeのdownload/install/executeを制限する。
Vaak portable bytecodeをdataとしてinterpreterが読む設計でも、配布するapplicationの用途と審査規則は
host製品側で確認しなければならない。[Apple App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/)
この文書はApp Store受理を保証しない。

### 4.4 値表現とGC

Rustの`Value` enumのmemory layoutをmanaged ABIに写してはいけない。C#での候補は次である。

```text
ValueSlot
  tag          : small enum
  primitiveBits: 64-bit payload
  heapHandle   : arena index, or none
```

初期実装は`readonly struct ValueSlot`の配列でよい。ただし「16 byteである」ことをwire contractにせず、
実測値として監視する。reference型を`object`欄へ直接置く形は簡単だが、boxingとGC root走査を増やすため、
hot stack/cellでは整数handleを優先する。C#のvalue typeはheap allocationを減らせる一方、大きいstructの
copy自体が費用になる。[Microsoft: avoid allocations and copies](https://learn.microsoft.com/en-us/dotnet/csharp/advanced-topics/performance/)
[Boxing and unboxing](https://learn.microsoft.com/en-us/dotnet/csharp/programming-guide/types/boxing-and-unboxing)

arenaはGCを無効にする仕組みではない。意味上の寿命を明示する仕組みである。

- `PreparedProgram`: immutableなopcode、constant、type、source-map配列
- `Runner`: operand stack、frame、cell、stageの再利用buffer
- `RegionArena`: region markとheap object handle
- regionを抜けたらlive countを戻し、捨てる欄のreferenceをclearする
- 大きい配列はpoolを使いうるが、poolは最適化であり意味論にしない
- finalizer、`IDisposable`、GC timingをVaakの作用にしない

.NET GCはmanaged heap上の到達可能性を追跡するため、arenaをresetしても古いreferenceを保持すれば
対象は回収されない。[.NET GC fundamentals](https://learn.microsoft.com/en-us/dotnet/standard/garbage-collection/fundamentals)

`str`はC# `string`ではなく、既存仕様どおり`u8 array`のwrapperとしてbyte列を保つ。
Map/Hashも`Dictionary`の既定iteration orderへ意味を委ねず、Vaakが定めた順序を明示的な配列とindexで持つ。

### 4.5 Threading

公開契約は次の形が安全である。

- `PreparedProgram`と検証済みschemaはimmutableで、複数threadから共有可能
- `Runner`は一度に一threadだけが所有する
- 同じ`Runner`への同時実行・再入は状態flagで早期拒否する
- 並列実行にはRunnerをrunごと、またはworkerごとに一つ用意する
- host adapterはUI/main-thread制約をcoreへ持ち込まず、自身でdispatchする
- cancellationは命令境界のbudget検査候補とし、async callbackでarenaを横断させない

lockをRunner内部へ散らすより、thread confinementをAPIにする。managed threadingでも共有状態は正しい
synchronizationなしには安全にならない。[.NET managed threading practices](https://learn.microsoft.com/en-us/dotnet/standard/threading/managed-threading-best-practices)

### 4.6 Package構成案

```text
managed/dotnet/
  IronVaak.Core/            bytecode decoder、validator、VM、値
  IronVaak.Host/            snapshot / command / capability adapter
  IronVaak.TestKit/         共通fixture readerとdigest
  IronVaak.Unity/           Unity固有の薄いadapter。coreへUnityEngine依存を入れない
  com.vaak.iron/            UPM package view
```

NuGetは`netstandard2.0`と`net8.0`以降をmulti-targetする案が有力である。`netstandard2.0`は広いconsumerへ、
`net8.0`はAOT analyzerを有効にするためのtargetである。MicrosoftもAOT libraryには`net8.0`以降を含む
multi-targetを案内している。[Native AOT target guidance](https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/#target-framework-requirements)
最終baselineを`netstandard2.0`と`2.1`のどちらにするかは未決事項とする。

Unity Package Manager版は`package.json`、`Runtime/*.asmdef`、`Tests`、`Documentation~`を持つsource packageとし、
coreの同じC# sourceをbuildする。これはUnityの推奨package layoutに沿う。
[Unity package layout](https://docs.unity3d.com/6000.0/Documentation/Manual/cus-layout.html)

pure C# coreにはRID別native assetを入れない。将来Rust VMを.NETから呼ぶnative packageを作る場合も、
それはIRON VAAKとは別artifact・別capability・別test laneとする。

## 5. JVM版: pure Java / Kotlin VM

### 5.1 Core言語の候補

最小依存とJava consumer互換を優先するなら、VM coreをpure Java、Kotlin APIを薄い別artifactにする案が
最も保守的である。Kotlin/JVM coreも実現可能だが、Kotlin value classはgeneric、interface、nullable位置で
boxingされ、Java公開APIでは追加wrapperが要る。
[Kotlin inline value classes](https://kotlinlang.org/docs/inline-classes.html)

したがって最初のhot pathは、Javaのprimitive配列と明示indexで書く。

```text
byte[]  tags
long[]  primitiveBits
int[]   heapHandles
int     stackTop
```

AoSの小さなclassを命令ごとに生成するより、primitive配列ならGC allocationを避けやすい。
これはJVM object layoutや将来のvalue class機能へ依存しない。

Java baselineはJava 8 bytecodeがAndroid互換上は有力である。Android Gradle PluginはJava 8+機能を
D8/R8でdesugarできる。[Android Java 8+ support](https://developer.android.com/studio/write/java8-support)
ただしdesktopで利用するJDK baselineを8、11、17のどこに置くかは、release時点の利用者要件と
Android minSdkを確認して決める。

### 5.2 Android

ARTはDEXを実行し、AOTとJIT、GCをplatform側で管理する。
[Android Runtime](https://developer.android.com/guide/platform#art)
Vaak VMはdynamic class loading、JNI、runtime bytecode compilerを使わないため、通常のlibrary codeとして
D8/R8へ渡せる。最初のAndroid artifactはresourceを持たないpure JARでよく、Android固有APIを足した場合だけ
AAR adapterを分ける。

hot pathが固まった後は、library用Baseline ProfileでVM loopを初回からAOT対象へ寄せられる。
これは意味論ではなくplatform optimizationである。
[Android Baseline Profiles](https://developer.android.com/topic/performance/baselineprofiles/overview)

### 5.3 Desktop JVMとGraalVM Native Image

通常のHotSpot上ではJITの有無をVaakが制御する必要はない。Vaak自身はinterpreterであり、正しさを
JIT warm-upへ依存させない。

追加gateとしてGraalVM Native Imageを通す価値がある。Native Imageはclosed-world static analysisを行い、
reflection、dynamic proxy、resource、JNI等にはreachability metadataを要求する。
[GraalVM Native Image](https://www.graalvm.org/latest/reference-manual/native-image/)
[Reachability metadata](https://www.graalvm.org/latest/reference-manual/native-image/metadata/)
coreをreflection-freeかつresource-freeにすれば、metadataの面積を小さくできる。

GraalVMはJVM版の必須runtimeにしない。通常JAR、Android ART、Graal nativeの三者で同じfixtureを通す
追加のAOT健全性gateとして扱う。

### 5.4 iOSはJVM targetではない

iOSに通常のJVM artifactを置く案は採らない。Kotlin Multiplatformは、JVMが望ましくない／不可能なiOSで
Kotlin/Nativeを使いnative binaryを作る。[Kotlin Multiplatform overview](https://kotlinlang.org/docs/multiplatform/kmp-overview.html)
Kotlin/Nativeはstatic library、framework、XCFrameworkを生成できる。
[Kotlin native binaries](https://kotlinlang.org/docs/multiplatform/multiplatform-build-native-binaries.html)

将来一つのKotlin `commonMain`からJVMとKotlin/Native VMを作る余地はある。ただし次が変わる。

- JVM標準libraryを使えず、common APIだけへ制約される
- Kotlin/Native自身のGC・allocator・Swift/Objective-C interopが入る
- Java consumer向けpure Java coreとはsource構成が別になる
- iOS conformance gateが増える

これは「JVM版をiOSへ持ち込む」ことではなく、第三のruntime targetである。今回のscopeでは決めない。

### 5.5 値・算術・GC

- `u1/u8/u16/u32`はJavaのsigned primitiveへbit patternとして保持し、比較・除算はunsigned helperを使う
- `i32/i64`のoverflow、shift、Euclidean divisionは共通helperへ集約する
- `f32/f64`は各演算後のfinite検査、`-0.0`正規化、map key変換を共通fixtureで固定する
- `str`はUTF-16 `String`ではなく`byte[]`
- map/hashの観測順はJDK collection実装へ委ねない
- `Throwable`をparadoxに自動変換せず、VM内部bug、resource exhaustion、Vaak runtime resultを分ける

JVMのprimitive型とobject/arrayは仕様上区別されるが、objectのphysical layoutはVaak ABIではない。
[JVMS 25, §2](https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-2.html)
GCの時期も意味論にせず、C-90のregion markとarena handleをVM自身で管理する。

### 5.6 Threading

.NET版と同じく、immutable `PreparedProgram`は共有可能、mutable `Runner`はthread-confinedとする。
Javaでは正しく同期されない共有read/writeが驚く結果を持ちうるため、Runnerの欄を公開して共有させない。
[JLS 25, Chapter 17](https://docs.oracle.com/javase/specs/jls/se25/html/jls-17.html)

同じRunnerへの再入はmonitorで待たせず、明示errorにする。待たせると同じthreadからのhost callbackで
deadlockし、問題を隠す。別Runnerによる並列実行だけを許す。

### 5.7 Maven / Gradle構成案

```text
managed/jvm/
  vaak-bytecode/       decoder、validator、format model
  vaak-vm/             pure Java VM
  vaak-host/           snapshot / command / capability adapter
  vaak-kotlin/         optional Kotlin facade
  vaak-testkit/        共通fixture、digest、CLI
  vaak-android/        Android固有機能が必要になった時だけ。coreは依存しない
```

Gradle `java-library` pluginは公開APIの依存と内部実装依存を分け、Maven Publish pluginからPOMを生成できる。
[Gradle Java Library](https://docs.gradle.org/current/userguide/java_library_plugin.html)
[Gradle Maven Publish](https://docs.gradle.org/current/userguide/publishing_maven.html)

Maven CentralにはJAR、POM、source、Javadoc、署名等を公開する。
[Apache Maven Central publishing guide](https://maven.apache.org/repository/guide-central-repository-upload)
pure VMはJNI `.so`を同梱しない。Android専用variantやresourceが無ければAARにする理由もない。

## 6. Portable bytecodeへ要求すること

portable bytecode v0の具体的opcode、番号、section layoutは別作業であり、この文書では決めない。
managed VMから見て必要な不変条件だけを挙げる。

### 6.1 Rustの内部表現をwireにしない

次を直接serializeしてはならない。

- Rust `enum` discriminant、`usize`、native endian
- `Program2`の`HashMap` iteration order
- `Value`のmemory bytes、pointer、`Box`、`Rc`
- `serde`/`bincode`の既定layout
- source treeやhost trait object

wireは固定幅または明示varint、明示endian、固定されたsection IDとlengthを持ち、unknown required sectionを
拒否する。全offset・length・countは加算overflowを検査してからsliceへ変える。

### 6.2 Decodeとexecuteを分ける

`PreparedProgram.Load(bytes)`は実行前に少なくとも次を全件検証する。

- magic、format major/minor、総長
- sectionの重複・重なり・範囲
- opcodeとoperand幅
- constant/type/function/source-map index
- jump targetが命令境界であること
- stack/frame/stageの静的上限または検証可能なmetadata
- declared host layoutとcapability
- collection・string・debug tableのresource limit

一命令走らせてから「後半が壊れていた」と判明させない。decode errorはVaak runtime paradoxではなく、
artifact validation failureである。

### 6.3 Capability

artifactが持つのは要求であって権限ではない。

```text
required feature/capability  -> hostが全てgrantしなければload失敗
optional feature/capability  -> grantされた集合だけprogramへ固定
host layout                  -> name、kind、type、schema、順序をprepare時に照合
run                           -> prepare後にlayoutを差し替えない
```

host function indexはhot loopへ入る前に決める。名前lookup、signature check、capability negotiationを
命令ごとに繰り返さない。dialect固有型、例えばSTEELの`f80`はcapabilityが無いmanaged VMで明示拒否する。

### 6.4 Source provenance

少なくともsource file ID、byte span、function/chunk、bytecode PCを対応付けられるdebug/source-map sectionが要る。
managed VMのexception stackだけでは、元Vaak sourceのparadox発生点を再現できない。

source-mapをstripしたrelease artifactでもPCとartifact identityは保持し、hostが別置きmapを照合できるようにする。
artifact hash、compiler identity、format versionは互いに別欄にする。compiler versionが同じ意味versionを
保証するとは限らない。

## 7. Rust compilerと将来managed compiler

### 7.1 第一段

Rust front endだけがsourceをportable bytecodeへcompileする。

```text
source -> Rust check/type-check -> portable bytecode
                                -> Rust VM
                                -> IRON VAAK
                                -> JVM VM
```

この段階なら、managed VMの失敗はdecoderかexecutionのどちらかへ絞れる。

### 7.2 第二段

C#またはJava/Kotlin front endを足す場合も、次の順で資格を与える。

1. token/span fixture
2. AST normalization fixture
3. static error categoryとsource span fixture
4. typed program fixture
5. portable bytecode normalized digest
6. 全runtimeでのexecution differential

byte列完全一致を要求するにはconstant pool、function order、map order、debug sectionのcanonicalizationが必要である。
そこをportable bytecode v0が決めない間は、sectionをdecodeした正規化IRの一致を先に使う。

managed compilerが通ったからRust参照実装を退役させる、という判断はこのsliceに含めない。

## 8. 四実装の差分試験基盤

### 8.1 Test bundle

repositoryに実装言語中立なbundleを置く。

```text
conformance/
  manifest.json
  source/*.vaak
  bytecode/*.vbc
  expected/*.json
  malformed/*.vbc
  host/*.json
```

JSONは人がreviewできるtest oracleに限定し、runtime production ABIとは分ける。RFC 8259で正確に相互運用できる
整数は概ね53bitまでなので、`i64`、float bit pattern、byte列はdecimal JSON numberへせず、型付きstringまたは
hexで持つ。[RFC 8259, §6](https://www.rfc-editor.org/rfc/rfc8259#section-6)

例:

```json
{
  "outcome": "value",
  "type": "i64",
  "bits_be": "ffffffffffffffff",
  "source": { "file": 0, "start": 12, "end": 17 }
}
```

### 8.2 比較する層

| 層 | 入力 | 比較 |
|---|---|---|
| decoder | valid/malformed `.vbc` | accept/reject、error offset、category |
| instruction | 手製の最小bytecode | stack、cell、stage、PC |
| language | 同一sourceからRust compilerが出したbytecode | typed outcome、発生span |
| reference | sourceをRust tree interpreterで実行 | Rust VM/.NET/JVMとの差 |
| host | snapshot、host layout、capability | reads/writes、command batch、error provenance |
| platform | 同一bundle | CoreCLR、Native AOT、Unity Mono/IL2CPP、HotSpot、ART、Graal native |

### 8.3 必須fixture族

- 全整数幅のoverflow、shift count、signed/unsigned compare
- 負数を含むEuclidean divisionと0除算
- `f32/f64`のfinite境界、`-0.0`、map/hash key order
- `str`の任意byte、NUL、非UTF-8
- deep copy、array/map/hash/struct、insert orderとremove後の穴
- 何も無い、paradox、escapeを混同しないこと
- 裸block、loop、frame、`continue`遅延、`outward`
- HostBindingの未使用・定数添字・動く添字、partial read/write
- runtime error時の`after` state（S-22）
- host functionの型、返値schema、contract error
- stale handle、epoch/generation、範囲外patch
- decode limits、巨大count、offset overflow、unknown required feature

runtimeが返す日本語message全文を最初からpublic ABIにしない。安定category、phase、source span、PC、
provenanceを比較し、messageは人向けsnapshotとして別に扱う。category集合自体は意味論所有者の判断待ちである。

### 8.4 Fuzzing

Rust側generatorからvalid/invalid bytecode corpusを作り、全decoderへ同じfileを渡す。random sourceを各managed
front endへ直接渡すのは、compilerとVMの二変数が同時に動くので第二段に回す。

crash、hang、OOMだけでなく、reject位置、resource limit超過、部分的host作用の有無も比較する。

## 9. Snapshot / Patch / Command schema

これはVaakの値やbytecode formatとは別のhost protocolである。次の三車線を混同しない。

| 車線 | 入力 | 作用 | failure時 |
|---|---|---|---|
| C-95 direct binding | host値のsnapshot | run後に変更分を書き戻す | S-22どおりerror前の変更も回収 |
| command batch | immutable snapshot | hostが全件検証後commit | commit前ならhost作用なし |
| leaf HostFn | 型付き引数 | 呼んだ場所で同期作用 | 既に起きた作用は戻さない |

### 9.1 非規範的なenvelope案

具体的なfield numberやbinary encodingは未決である。必要な情報は次である。

```text
Invocation
  schemaVersion
  invocationId
  baseRevision
  programIdentity
  requiredCapabilities[]
  grantedCapabilities[]
  hostLayoutIdentity
  limits
  snapshotBindings[]

CommandBatch
  schemaVersion
  invocationId
  baseRevision
  commands[]
  observations[]

Command / Patch
  operationKind
  targetKind
  targetHandle { arenaEpoch, slot, generation }
  expectedRevision or precondition
  typedPayload
  provenance { engine, adapter, sourceSpan, bytecodePc }

Failure
  phase
  category
  engine
  adapter
  sourceSpan?
  bytecodePc?
  commandIndex?
  hostCode?
  humanMessage
```

`baseRevision`とhandle generationにより、snapshot後にhost stateが変わったbatchをcommit前に拒否できる。
これはVaakのrollbackではなくhostのoptimistic validationである。

### 9.2 Capabilityの不変条件

- scriptやadapterは自分にcapabilityをgrantできない
- unknown required capabilityはprepare失敗
- unknown optional capabilityは無視できるが、使用命令へ到達させない
- capabilityは名前だけでなくversionとoperation setを持つ
- filesystem/network/clock/random/UI thread等はcoreから暗黙利用しない
- output byte数、command数、arena、stack、instruction budgetをhostが上限化する
- errorにも「どのcapability providerが答えたか」を残す

### 9.3 Encoding候補

| 候補 | 利点 | 注意 |
|---|---|---|
| custom fixed binary | decoder小、exact numeric、portable bytecodeと整合 | schema進化とtoolingを自分で持つ |
| Protocol Buffers | C# / Java / Kotlin生成、unknown field、広いtooling | runtime/codegen依存、serializationはcanonicalではない |
| deterministic CBOR profile | exact integer/byte string、小さいdecoderも可能 | application profileで型・map order・float表現を追加規定する必要 |
| JSON | 人が読みやすくfixture向き | 53bit整数、float bit、byte列、sizeでproduction hot pathに不向き |

Protocol BuffersはC#、Java、Kotlinを公式生成対象に持ち、schema evolutionを想定するが、wire serialization自体は
canonicalではない。[Protocol Buffers guides](https://protobuf.dev/programming-guides/)
CBORはbyte stringと64bit整数を持ち、core deterministic encoding要件を定義する。
[RFC 8949](https://www.rfc-editor.org/rfc/rfc8949.html#name-deterministically-encoded-c)

初期conformance fixtureは型付きJSONでよい。production envelopeをcustom binary、Protobuf、CBORのどれにするかは、
portable bytecode v0とUnity/Android dependency予算を見て別途決める。

## 10. Error provenance

managed exceptionを全て同じVaak runtime errorへ潰さない。

| phase/provenance | 例 | 扱い |
|---|---|---|
| artifact/decode | section範囲外、unknown opcode | programを走らせない |
| static | source/typed program不正 | Rust compiler由来ならcompiler診断を保持 |
| Vaak runtime | paradox、control error | 既存の発生spanとPCを返す |
| host contract | 宣言型と返値が不一致 | 次命令前に停止、providerを記録 |
| resource | instruction/arena/output上限 | host policy failureとして区別 |
| adapter | Lua/.NET/Java側のfailure | engine・adapter・原errorを保持 |
| commit | stale revision、不正command | batchを適用せずcommand indexを返す |
| implementation bug | impossible opcode/state、managed exception | paradoxに偽装せずbugとしてfail closed |

どのcategoryをpublic stable codeにするか、resource exhaustionを既存三種の実行時errorへどう写すかは未決である。
ここでS-nを追加しない。

## 11. Reentrancy禁止

既存`docs/experiments/embedding.md`のLeaf NodeOps契約を全managed VMで維持する。

```text
Runner.run()
  -> leaf HostFn
       -> 同じRunner.run()     禁止
       -> 別Runner.run()       hostが明示的に管理するなら可
       -> commandを返す        推奨
```

実装は`Idle -> Running -> Idle`のstate transitionを持ち、`Running`中の同じinstanceへのentryを
同期lockで待たせず拒否する。host callの途中で待つと自己deadlockになるためである。

本当に再入・中断が必要なoperationはLeaf HostFnへ偽装せず、将来のsuspend/resume契約へ送る。
その契約はS-11の未完成部分であり、この文書ではstate machine、borrow lifetime、writeback時点を決めない。

## 12. Lua等との併用

LuaをIRON VAAK/JVM版の依存にも、Vaak bytecodeの実行基盤にも置かない。hostが必要なら**並列するscript adapter**として
追加する。

```text
                 immutable host snapshot
                    /             \
                   v               v
              Vaak Runner      Lua engine
                   |               |
                   v               v
              command batch    command batch
                    \             /
                     host validates,
                     orders and commits
```

Lua 5.4のC APIはglobal変数を持たず`lua_State`へ状態を集約するが、native library、C stack、yield/continuation、
platform別linkingを伴う。[Lua 5.4 manual, C API](https://www.lua.org/manual/5.4/manual.html#4)
そのためpure managed coreの必須依存には向かない。

任意adapter候補は次である。

| runtime | 候補 | 注意 |
|---|---|---|
| .NET / Unity | [MoonSharp](https://www.moonsharp.org/) | pure C#、Unity/iOS AOT対応を掲げるがLua 5.2系。version・license・IL2CPP現行性を採用前に監査 |
| JVM / Android | [LuaJ](https://github.com/luaj/luaj) | pure Java。interpreter車線だけを使い、Lua→Java bytecode compilerはAOT前提から外す |
| native host | Lua 5.4 C API | managed coreとは別native adapter。static/dynamic linkingとlicense inventoryが必要 |

併用時の不変条件:

- Lua table/userdataをVaak `Value`として直結せず、schema付きsnapshotへ変換する
- VaakとLuaを互いの同期callbackから呼ばない
- 各commandへ`engine=vaak|lua`、adapter version、source provenanceを付ける
- batch間のordering、conflict、last-writer-winsを各言語に決めさせずhost policyにする
- Lua errorを自動的にVaak paradoxと呼ばない
- 同じhost handleを複数engineへ渡す場合もepoch/generationを検査する
- Luaが無くても全Vaak APIとconformance testが成立する

## 13. Securityとresource bounds

portable bytecodeとsnapshotはtrusted compilerから来ても、managed VMではuntrusted bytesとしてdecodeする。

- bytecode総長、section数、constant数、function数
- string/array/map/hash要素数
- operand stack、frame、stage、call depth
- region arena bytesまたはnode数
- instruction budget
- HostFn回数、command数、output bytes
- source-map/debug table

をrun-local limitで上限化する。limit超過時に巨大bufferを先に確保しない。

clock、random、filesystem、network、thread生成、process起動はcore VMの標準命令にせず、明示capabilityを持つhost
operationだけから到達させる。managed runtimeのreflectionやclass loaderをcapability mechanismに流用しない。

## 14. 実装roadmap

### Checkpoint A: format消費者としてのtest kit

- portable bytecode v0文書を読むdecoder contract
- valid/malformed fixture manifest
- typed JSON outcome
- Rust interpreter/Rust VMのoracle exporter

### Checkpoint B: IRON VAAK core

- decoder/validator
- scalar、stack、frame、control、region arena
- collectionとdeep copy
- differential runner
- CoreCLR / Native AOT gate

### Checkpoint C: Unity

- UPM layoutとasmdef
- Mono/IL2CPP、Android/iOS player
- stripping gate
- allocation/GC benchmark。ただし時間を意味試験の合否にしない

### Checkpoint D: JVM core

- pure Java decoder/validator/VM
- HotSpot、ART、Graal Native Image gate
- Maven publicationとoptional Kotlin facade

### Checkpoint E: host protocol

- C-95 snapshot/writeback
- command batch validator
- capability negotiation
- provenance、resource limit、reentrancy guard

### Checkpoint F: managed compiler

- parser/check/type-checkを差分fixtureで一段ずつ資格化
- normalized bytecode一致
- Rust compilerを外すかは別判断

WASM、Lua adapter、Kotlin/Nativeはこの主列へ割り込ませず、core conformance成立後のoptional laneとする。

## 15. 意味論所有者へ返す未決事項

以下はこのcheckpointで決めていない。

1. JVM版の製品名。
2. managed実装を同じrepositoryへ置くか、別repository/packageで管理するか。
3. IRON VAAKの最低TFMを`netstandard2.0`、`netstandard2.1`のどちらにするか。
4. JVMの最低class-file/JDKを8、11、17のどこにするか。
5. JVM版coreをpure Javaにするか、Kotlin common coreにしてKotlin/Nativeも同一sourceで狙うか。
6. portable bytecode v0のopcode、section、versioning、canonicalization。
7. snapshot/command production encodingをcustom binary、Protobuf、CBORのどれにするか。
8. public error category/codeとsource provenanceの安定範囲。
9. resource exhaustionとcancellationを既存runtime errorへどう写すか。
10. S-11第二段のsuspend/resume、再開時writeback、別Runner再入の公開契約。
11. 複数engineのcommand conflict/orderをどのhost層で定義するか。
12. managed VMが対応しない方言型・将来機能をcapabilityでどう表すか。
13. managed compilerにcanonical statusを与える合格条件。
14. package coordinate、署名、release cadence、support matrix。

これらへ答えるまで、実装側が便宜的な挙動を言語仕様として固定してはならない。

## 16. 一次資料

### .NET / Unity / Apple

- [.NET Native AOT deployment](https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/)
- [.NET libraries and trimming](https://learn.microsoft.com/en-us/dotnet/core/deploying/trimming/prepare-libraries-for-trimming)
- [.NET Native AOT for iOS-like platforms](https://learn.microsoft.com/en-us/dotnet/core/deploying/native-aot/ios-like-platforms/)
- [.NET MAUI runtimes and compilation](https://learn.microsoft.com/en-us/dotnet/maui/deployment/runtimes-compilation)
- [.NET Standard](https://learn.microsoft.com/en-us/dotnet/standard/net-standard)
- [NuGet multi-target packages](https://learn.microsoft.com/en-us/nuget/create-packages/supporting-multiple-target-frameworks)
- [Unity IL2CPP](https://docs.unity3d.com/jp/current/Manual/il2cpp-introduction.html)
- [Unity .NET profiles](https://docs.unity3d.com/ja/current/Manual/dotnet-profile-support.html)
- [Unity managed code stripping](https://docs.unity3d.com/es/current/Manual/ManagedCodeStripping.html)
- [Unity package layout](https://docs.unity3d.com/6000.0/Documentation/Manual/cus-layout.html)
- [Apple App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/)

### JVM / Android / Kotlin

- [Android platform architecture / ART](https://developer.android.com/guide/platform)
- [Android Java 8+ desugaring](https://developer.android.com/studio/write/java8-support)
- [Android Baseline Profiles](https://developer.android.com/topic/performance/baselineprofiles/overview)
- [Android library publishing](https://developer.android.com/build/publish-library)
- [Java Language Specification 25, Threads and Locks](https://docs.oracle.com/javase/specs/jls/se25/html/jls-17.html)
- [Java Virtual Machine Specification 25, Structure](https://docs.oracle.com/javase/specs/jvms/se25/html/jvms-2.html)
- [GraalVM Native Image](https://www.graalvm.org/latest/reference-manual/native-image/)
- [GraalVM reachability metadata](https://www.graalvm.org/latest/reference-manual/native-image/metadata/)
- [Kotlin value classes](https://kotlinlang.org/docs/inline-classes.html)
- [Kotlin Multiplatform overview](https://kotlinlang.org/docs/multiplatform/kmp-overview.html)
- [Kotlin/Native binaries](https://kotlinlang.org/docs/multiplatform/multiplatform-build-native-binaries.html)
- [Kotlin/Native memory management](https://kotlinlang.org/docs/native-memory-manager.html)
- [Gradle Java Library plugin](https://docs.gradle.org/current/userguide/java_library_plugin.html)
- [Gradle Maven Publish plugin](https://docs.gradle.org/current/userguide/publishing_maven.html)
- [Apache Maven Central publication](https://maven.apache.org/repository/guide-central-repository-upload)

### Protocol / optional script adapter

- [RFC 8259 JSON](https://www.rfc-editor.org/rfc/rfc8259)
- [RFC 8949 CBOR](https://www.rfc-editor.org/rfc/rfc8949.html)
- [Protocol Buffers programming guides](https://protobuf.dev/programming-guides/)
- [Lua 5.4 Reference Manual](https://www.lua.org/manual/5.4/)
- [MoonSharp](https://www.moonsharp.org/)
- [LuaJ](https://github.com/luaj/luaj)
