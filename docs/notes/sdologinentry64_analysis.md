# SDO 登录入口 DLL 的实际依赖

## 结论与范围

**当前 Injector 启动路径通过 `FfxivArgLauncher.ArgFixer` 替换游戏的 SDO 登录函数，直接从命令行提供 ticket 和账号 ID，不经过 SDO 入口 DLL。** 因此用户当前缺原版仍能正常启动，并不与 shim 的静态控制流矛盾。

**另一条直接运行游戏的路径没有这个修补。** 当 ottercorp shim 被正常加载并执行进程附加入口时，它必须能加载同目录的 `sdologinentry64.sdo.dll`，否则弹窗后退出。这个条件依赖不能推广为所有启动路径都必需原版，也不能据此增加无条件缺原版即禁止启动的检查。

本分析使用 REA 6.8.0 CLI 的 PE 导出、导入、TLS、托管程序集 CIL 检查及 GNU objdump 证据接口，人工阅读汇编，并核对版本固定的上游源码；没有配置反编译引擎，未生成反编译伪代码，也没有执行 DLL、启动游戏或修改安装。下文示意代码是人工归纳，不是原始源码。

分析对象：

- shim：`~/Games/ffxiv/sdo/sdologin/sdologinentry64.dll`，16,896 字节，SHA256 `a7ae15660d00eb0b15e76902736721d2cc7450fed811dfbde879a07336ae65cd`。
- 原版样本：同目录 `sdologinentry64.sdo.dll`，529,792 字节，SHA256 `d884bead00ecba3012dc9e0f6faae8692249a439dfaa8fb60db9570dde48986f`。其 PE 导出目录内部名称是 `updatecore.dll`，包含六个导出。
- `~/LinuxGames/ffxiv/sdo/sdologin/sdologinentry64.dll` 与上述 shim 哈希一致；检查时该目录没有 `.sdo.dll`。用户报告该安装可以正常启动，不能仅凭缺文件判定安装不可用。

其他版本的官方同名 DLL，尤其官方更新包中 46,080 字节与约 529 KiB 的两个文件，不能直接套用该样本结论或互换，见 [SDO 安装来源](sdo-installation.md)。

## 1. 为什么当前 shim 少不了原版

下列地址均为静态首选 VA，image base 为 `0x180000000`，不是实际进程地址。

shim 的 PE 入口 RVA 为 `0x1998`；CRT 包装最终进入 `0x1800011b0` 的 DLL 生命周期处理。`DLL_PROCESS_ATTACH` 分支：

1. 解析命令行，搜索 `DEV.TestSID=`、`XL.SndaId=`（字符串 VA `0x180003408`、`0x180003428`）。
2. `0x18000137b–0x180001395` 根据提取的参数指针是否为空，设置 `0x180005034` 处的回退标志：没有识别到参数则走原版接口。
3. `0x18000139b–0x1800013d4` 取 **shim 自身路径**，拼接 `sdologinentry64.sdo.dll`，调用 `LoadLibraryW`。此处没有检查回退标志，第三方分支也执行。
4. `0x1800013e9–0x18000142a` 检查加载结果；失败则 `MessageBoxW("Can not load %s", …)`，随后 `ExitProcess(0xfffffffe)`，不是可忽略的警告。
5. `0x180001431–0x18000145e` 为原版解析 `SDOLGetModule`、`SDOLInitialize`、`SDOLTerminal`。辅助函数 `0x1800010d0` 对缺失导出同样弹窗并退出。

因此原版不仅要“有一个同名文件”，还必须是能成功加载并提供上述导出的兼容 PE。存在文件、含某个字符串或体积相近都不足以验证。

## 2. 第三方分支究竟调用谁

三个导出都检查同一个回退标志：

- **`SDOLInitialize`，`0x1800015b0`**：标志非零才调用原版函数指针（`0x1800015dd`）。第三方分支在 `0x1800015d8` 跳到返回路径，直接返回 `0`。
- **`SDOLGetModule`，`0x1800014c0`**：原版分支在 `0x18000151a` 尾调用原版；第三方分支从 `0x180001521` 起写输出结构的 `+0`、`+0x18`、`+0x20` 字段，再返回 `0`。它使用 shim 保存的命令行相关指针，不调用原版该接口。字段名与完整 ABI 还需调用方或运行验证，不能仅凭日志标签推定。
- **`SDOLTerminal`，`0x180001570`**：原版分支在 `0x180001594` 尾调用原版；第三方分支在 `0x18000159b` 直接返回 `1`。

控制流可概括为：

```text
加载 shim：
    解析第三方参数
    无条件加载原版；失败则弹窗并 ExitProcess(-2)
    无条件解析原版三个导出；缺失则退出

调用 SDOL*：
    没有识别到第三方参数 → 转发原版
    识别到第三方参数     → shim 自己返回结果
```

Eorzea 的 `src/game.rs::build_sdo_launch_args()` 会构造 `DEV.TestSID=…` 和 `XL.SndaId=…`。上述分支只说明本地接口如何提供数据，**不表示服务端不验证票据**，也不证明凭任意字符串可以登录。

## 3. 原版加载是否意味着 SDK 也必须存在

原版样本的静态导入表有九个 Windows 系统 DLL，没有 `sdologinsdk64.dll`，也没有延迟导入表。SDK 采用动态加载，不能仅凭导入表判断不需要。

关键路径：

- 原版 PE 入口 `0x180033610` 经 CRT 包装，在 `0x18003356f` 调用用户 DllMain `0x1800025d0`。该函数在进程附加时只保存模块句柄，随后返回 `1`，没有加载 SDK。
- 原版 `SDOLInitialize` 在 `0x180002b30`，其 `0x180002e46–0x180002e82` 路径构造 `sdologin\\sdologinsdk64.dll`，通过 `LoadLibraryExA` 加载，再查找并调用 SDK 的 `SDOLInitialize`。
- `igwInitialize` 中另有 SDK 加载路径（字符串引用 `0x1800029bc`）。shim 没有导出或解析 `igwInitialize`。
- REA 检查未发现原版样本的 PE TLS 表。CRT 仍有全局初始化过程；未对全部初始化及间接调用做完备可达性证明，不能把“用户 DllMain 很短”扩大为“加载原版绝无副作用”。

**推断**：Eorzea 正常识别第三方参数时不进入上述官方接口，SDK 不是已经证实的必要安装依赖。完整 SDK 需求仍须最小安装对照测试；官方回退分支不在这个推断范围内。

## 4. 已有运行日志能佐证什么

### 当前运行反例

2026-11-17 只读检查 `/proc/3450352`：当前 `ffxiv_dx11.exe` 的工作目录为 `~/LinuxGames/ffxiv/game`，映射的游戏 EXE 也位于此安装，排除了仅凭另一个旧安装成功启动作判断的情况。该安装的 `sdo/sdologin/` 只有上述哈希的 shim，没有 `.sdo.dll`。用户报告可以正确启动。

该进程的 maps 快照没有 `sdologinentry64.dll`、`.sdo.dll` 或 SDK 路径，但有 `Dalamud.Boot.dll`。仅这个快照不证明从未加载、没有卸载，也不证明是 Dalamud 绕过了它们。没有附加调试器、读取游戏业务内存或操作窗口。

实际原因由分发程序集、对应源码及 Injector 日志共同解释，见第 5 节。以下旧日志只能证明相应那次启动加载过两个入口。

### 历史加载记录

只读取既有日志，不执行新启动：

- `~/.eorzea/logs/game-1794570263.log:250,253`：分别记录 shim、原版成功加载。
- `~/.eorzea/logs/game-1794570441.log:250,253`：同样记录两者成功加载。
- 两份日志全文搜索 `sdologinsdk` 均为零匹配；不能据此证明所有进程、所有时段都未使用 SDK。
- 日志中的 `SDOLInitialize@地址` 等输出来自 shim 的 **GetProcAddress 打印**，不能当作原版接口被调用的记录。

这与静态控制流一致，但没有原版接口调用跟踪，也没有移除原版后的成功进入游戏对照。开场卡住的媒体问题独立处理，见 [Wine 媒体故障](wine-media.md)。

## 5. Injector 如何绕过 DLL

### 分发文件与调用顺序

本机 `~/.eorzea/dalamud/Hooks/15.0.4.0/version.json` 声明 Dalamud GitSha `3dcf078426f9d74e8036d163715949c8f3446556`；该提交锁定的 `lib/FfxivArgLauncher` 子模块为 `166d51223ef3270c46a5a2eeb1500fb53aa9ccad`。

对应 [Injector Program.cs:938–958](https://github.com/ottercorp/Dalamud/blob/3dcf078426f9d74e8036d163715949c8f3446556/Dalamud.Injector/Program.cs#L938-L958) 的启动回调先调用 `new ArgFixer(p).Fix()`，随后才检查 `!withoutDalamud` 和 entrypoint 模式。本机实际 `Dalamud.Injector.dll` 的 CIL 也确认该顺序：回调 token `0x0600016d` 在 IL offset `2` 构造 ArgFixer、`9` 调用 Fix，`20` 才出现第一个条件跳转。这不是只看当前源码分支的推测。

`GameStart.LaunchGame()` 使用 `CREATE_SUSPENDED` 创建游戏，在 `beforeResume` 回调完成后才 `ResumeThread`。修补无需依赖游戏内的 Dalamud 托管插件，也不等同于仅改写 CLR 入口。

### 修补内容

[ArgFixer.cs](https://github.com/ottercorp/FfxivArgLauncher/blob/166d51223ef3270c46a5a2eeb1500fb53aa9ccad/ArgFixer.cs) 中：

1. `Fix()`（46–60 行）用 `E8 ?? ?? ?? ?? 44 38 64 24` 定位调用点，读取相对地址得到游戏 SDO 登录函数。
2. `SetupArgFixFunction()`（81 行起）生成机器码，扫描游戏参数；把 `DEV.TestSID=` 后的指针写到游戏对象 `+0xA0`，把 `XL.SndaId=` 后的指针写到 `+0xA8`，并设置相应状态。
3. `SetupHook()`（255–291 行）在原函数入口写入 `mov rax, replacement; jmp rax`，直接跳到替代实现。替代代码不调用 shim 或原版 DLL。

本机实际 `FfxivArgLauncher.dll` 的 CIL 确认 `Fix → SetupArgFixFunction/SetupHook`，以及 `SetupHook → ExternalMemory.WriteRaw`。源码说明具体生成的机器码语义；没有进行源码可复现构建或对正在运行的进程读取补丁字节，不能把这些静态证据写成完整动态调用跟踪。

普通 objdump 检查本机游戏 EXE（51,961,600 字节，SHA256 `52042b60631cf2c48851b3aaee36e9ec4fedd6d7f2c68f0e9a8c3befb0af9f98`）发现 `.text` 中签名的首个候选为 `0x14005f6d1`，其 `call` 目标为 `0x140095280`。该函数原本在 `0x14009533f` 调用 SDO 包装层 `0x14005d000`。这些地址是静态分析结果，不是本次读取到的运行时补丁地址；签名另有三个较后的候选，不能声称签名全局唯一。

### 与当前运行相互印证

当前 PID 3450352 的进程起始时间约为 `2026-11-17 18:40:57.860 +08:00`。同一时刻的 `~/.eorzea/dalamud/logs/dalamud.log/dalamud.injector.log`：

```text
564: 2026-11-17 18:40:57.951 +08:00 [INF] ArgFixFunctionAddress: 0x7FFD0034
565: 2026-11-17 18:40:57.967 +08:00 [INF] Nops Num:0
```

这些日志证明该次启动执行了 ArgFixer 的生成/安装流程相关代码；结合当前进程存活、安装没有原版和未见 SDO 映射，与入口被替换的机制一致。服务器票据验证仍然存在，这个补丁只改变客户端提供票据的方式。

## 6. Eorzea 两条启动路径的差异

- **`launch.method = "injector"`**（默认）：`prepare_launch()` 准备版本匹配的国服 Injector / ArgFixer 和所需 .NET。`GameLaunchConfig.dalamud = Some(...)` 表示使用 Injector，不表示一定加载插件；关闭 Dalamud 或使用 `AclOnly` 时传入 `--without-dalamud`，仍执行修补，不准备 assets。不要求或部署 SDO 入口 DLL。
- **`launch.method = "sdo_dll"`**：独立准备官方原版 + 固定摘要 shim，保留已有有效备份；缺失时从官方启动器 ZIP 获取原版。`GameLaunchConfig.dalamud = None` 表示直接运行游戏，此时创建进程前静态预检两个入口；该途径不加载 Dalamud，但保留 `[dalamud].enabled` 供 Injector 途径使用。
- `ensure_login_entry()` 是 DLL 途径准备的兼容入口；`game verify` 仅在 DLL 途径检查这两个文件。两条途径的准备均先于账号重新认证，失败不自动回退。

因此，当前能无原版启动的原因是 **通过 Injector 使用了另一种国服启动实现**，不是已经证明游戏本身删除了官方 DLL 依赖。是否在某个日期切换，不能仅凭本次调查断言。XIVLauncher.Core 仍分发 shim、其锁定源码仍含替换逻辑，也不能反推出所有实际启动都使用 shim。

配置、CLI、GUI、准备与条件预检已实现；官方完整下载和隔离目录幂等准备测试通过，没有进行两条途径的新游戏启动对照。官方来源、摘要和运行时验证边界见 [SDO 组件来源](sdo-installation.md)。

不能从本次证据断言官方入口检查父进程名称、签名、虚拟机或调试器；也不能给出不会封号、服务端无法检测等保证。

## 7. 提交记录（不等同于发版时间）

以下时间按 UTC+08:00 记录，来自提交元数据及实际 diff，不是首个公开发行包的时间：

- **2025-02-05 19:49:57**：[FfxivArgLauncher `3df7a10`](https://github.com/ottercorp/FfxivArgLauncher/commit/3df7a1054124403aef61fa464be5c1f5d9f8fec6) 的 `init` 已加入 ArgFixer、读取 `DEV.TestSID`/`XL.SndaId` 并替换 SDO 登录函数的代码；初始定位使用固定偏移。
- **2025-02-06 12:25:56**：[Dalamud `6d6c5d0`](https://github.com/ottercorp/Dalamud/commit/6d6c5d001b331494354e986617529ab984adb056) 的 `Add: ArgLauncher` 将该组件接入 Injector。diff 在当时的 `Dalamud.Injector/EntryPoint.cs` 中直接增加 `new ArgFixer(p).Fix()`，位置已在 `!withoutDalamud` 判断之前。
- **2025-06-06 09:11:52**：[Dalamud `ff0392e`](https://github.com/ottercorp/Dalamud/commit/ff0392e4e840098190c0175ae3ede68974d3f530) 将子模块更新为本机分发版本声明所对应的 `166d512`；该子模块提交为 `net9`，不是首次引入 SDO 修补。
- **2026-08-07 21:20:24**：Eorzea 本地提交 `75108764549158a4e818d472d1f1335ebdc03139` 接入启用 Dalamud 时通过 Injector 启动的路径；未启用时保留直接 Wine 启动。因此这种路径差异不是本次调查期间才发生的更新。

上述 Eorzea 提交的关闭 Dalamud 分支会移除 ArgFixer；仅有 shim、没有原版时按代码预计加载失败/退出，未据此重启游戏实测。当前实现由独立的 `launch.method` 选择途径；Injector 的 `--without-dalamud` 仍执行 ArgFixer，“不加载游戏内 Dalamud”并不意味着必须使用原版 DLL。

## 8. REA 证据与复查

完整本机输出保存在 `/tmp/opencode/sdo-dll-analysis/`，没有复制 DLL 或含票据的完整游戏日志。这是临时调查材料，不是持久发布资产。

- shim 汇编：`ev_aa5ab4d35b16e3cfa6367ff54d2cc7b725c3db4e8f0baaaf9f1c4c8d7d6b2df6`，`shim-disassembly.evidence.json`。
- shim 导出：`ev_eb1e2603ede752da3d267a2fb33879348b16176d40bcc488e631a8f315593a7e`，`shim-exports.json`。
- 原版汇编：`ev_3ef9bda299312568e57564155eb9bfc6c1ae7467271c790db29113aa1bd6f2f9`，`original-disassembly.evidence.json`。
- 原版导出：`ev_1b3a888c12fe3b46673c72fae4b94863b29ffb9119fb3d49c56989fca0c42b2a`，`original-exports.json`。
- 原版导入：`ev_ee2926b3baba440990a339bc2a4beafe46ee4e388d5686af3f537fb2e2590b54`，`original-imports.json`。
- 原版 TLS/load config：`ev_479dca7b770b0d25d399cbc6dcdeb5312948eb1342015d42220360f12dbb9902`，`original-load-config.json`。
- 实际 Injector CIL：`ev_c662b6a02f8cc4d2dfc63b52327d6870dca45790f5a5840783db0c524552d5ea`，`injector-members.evidence.json`；159,744 字节，SHA256 `3bde74f86fa050530dbe2295ed53eaf86c0a199f70751e63032fdabfaf4159f1`。
- 实际 FfxivArgLauncher CIL：`ev_195c0bde628d1a7fd45c8b34359cca6486d0fd7e464f97d375a07da7a2e0d7ad`，`arglauncher-members.evidence.json`；28,160 字节，SHA256 `ec881f31fae3153eb2ba27639e6c30018bb816879c83f813971ad27c53a88438`。

复查命令（只做静态读取；先确认文件 SHA256 与上述样本一致）：

```bash
rea inspect-native-exports "$HOME/Games/ffxiv/sdo/sdologin/sdologinentry64.dll" --json
rea inspect-pe-imports "$HOME/Games/ffxiv/sdo/sdologin/sdologinentry64.sdo.dll" --json
rea inspect-pe-load-config "$HOME/Games/ffxiv/sdo/sdologin/sdologinentry64.sdo.dll" --json
objdump -d -M intel --start-address=0x1800011b0 --stop-address=0x180001632 "$HOME/Games/ffxiv/sdo/sdologin/sdologinentry64.dll"
objdump -d -M intel --start-address=0x1800025d0 --stop-address=0x1800025e2 "$HOME/Games/ffxiv/sdo/sdologin/sdologinentry64.sdo.dll"
objdump -d -M intel --start-address=0x180002e0f --stop-address=0x180002e8b "$HOME/Games/ffxiv/sdo/sdologin/sdologinentry64.sdo.dll"
```
