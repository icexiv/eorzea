# 国服空目录安装与 SDO 组件来源

## 已确认的来源

官方客户端的 `Launcher3Configs/newVersionCenter.ini` 给出独立于游戏 ZiPatch 的启动器更新入口：

- 版本索引：<https://ff.autopatch.sdo.com/v3launcher/build/ver2data/791000814/8821/-1/ver2.dat>
- 全文件清单：<https://ff.autopatch.sdo.com/v3launcher/build/791000814/8821/client-all-files-list/client_all_files_list.dat>
- 启动器版本：<https://ff.autopatch.sdo.com/v3launcher/build/791000814/8821/client-all-files-list/LocalVersion3.xml>

这些入口无需账号即可读取。调查时索引目标版本为 `0.0.0.26`；实现不能固定该版本。

全文件清单第一行是 `baseUrl|productId|version`，后续为 `Windows相对路径|长度|MD5`。清单包含：

- `Launcher3Update\Launcher3Modules\sdologinentry64.dll`
- `Launcher3Update\Launcher3Modules\sdologin\sdologinsdk64.dll`
- `Launcher3Update\sdo\sdologin\sdologinentry64.dll`
- SDO 的其他 DLL、配置及资源。

两处官方 `sdologinentry64.dll` 不是同一个文件，不能仅凭文件名互换。全文件清单的 `baseUrl + 相对路径` 直取 DLL 实测返回 404；该清单不能直接当作已验证的逐文件下载接口。

## 可验证的下载链

`ver2.dat` 包含 `baseUrl` 和 `packages[]`，每项含 `from`、`to`、`fileListUrl`、`md5`。将 `baseUrl` 与 `fileListUrl` 拼接可取得 JSON 文件列表，其 MD5 与索引一致。文件列表包含：

- 包级 `baseUrl`；
- `directFileUrl` 指向 PList；
- `fileList[]` 给出 ZIP 的 `url`、`size`、`md5`。

例如从 `0.0.0.0` 到 `0.0.0.1` 的初始 ZIP 长度为 67,122,048 字节，MD5 为 `81B693A64844B4D0D2C1694C2FA1AF1D`。服务端支持 HTTP Range；读取中央目录确认其中有 370 个成员，包括：

- `Pkg\Launcher3Update\Launcher3Modules\sdologinentry64.dll`（529,728 字节）
- `Pkg\Launcher3Update\Launcher3Modules\sdologin\sdologinsdk64.dll`（2,032,960 字节）
- `Pkg\Launcher3Update\sdo\sdologin\sdologinentry64.dll`（46,080 字节）

两个入口 DLL 已通过 Range 提取、DEFLATE 解压、长度与 ZIP CRC 校验；没有执行。未下载完整 ZIP，因此没有验证整包 MD5。完整包的安装语义、后续差分和官方启动器运行时部署行为仍需验证，不能直接把每个 ZIP 解压到游戏目录就视作安装完成。

## 本机故障边界

`~/Games/ffxiv/Launcher3Modules/sdologinentry64.dll` 与该安装的 `sdo/sdologin/sdologinentry64.sdo.dll` SHA256 一致：`d884bead00ecba3012dc9e0f6faae8692249a439dfaa8fb60db9570dde48986f`，长度 529,792 字节。该文件含 `sdologin\\sdologinsdk64.dll` 和 SDO 更新策略 URL 字符串。这是依赖调查线索，不等同于运行时加载证据。

从空目录使用本项目更新的安装最初仅有 ottercorp shim，没有原版 `.sdo.dll`，实际出现 `Can not load ...sdologinentry64.sdo.dll` 弹窗。补回原版后用户仍报告卡住；继续从旧安装补齐 113 个 SDO 文件（包括 SDK、配置及资源）后仍卡住。因此，缺原版导致的加载弹窗与标题界面的卡住是两个问题，不能以本次卡住推断整套 SDO 组件都是第三方登录所必需的。

卡住现场的 `/proc/<pid>/maps` 未发现 SDO 文件映射，Dalamud boot 日志也未记录其加载；大厅 TCP 连接已建立并持续有少量双向数据。该快照不能证明 DLL 从未加载或从未卸载，但尚无证据表明本次卡住正在等待 SDO SDK。现场仍启用了 Dalamud 与插件，需要一次关闭 Dalamud、启用 DLL 加载日志的对照启动，不能把安装缺口等同于当前卡住根因。

对照启动关闭 Dalamud、设置 `WINEDEBUG=+loaddll` 后仍卡住，日志确认 shim 与原版 `.sdo.dll` 均成功加载。末尾错误是 `wmadmod.dll` 依赖的 `winegstreamer.dll` 缺失；实际 Wine 构建中未找到该组件，构建输入也没有 GStreamer。正常安装与新安装的游戏 EXE 和版本文件 SHA256 相同，但游戏目录下 `My Games/FINAL FANTASY XIV - A Realm Reborn/FFXIV.cfg` 的 `CutsceneMovieOpening` 分别为 `1` 和 `0`。仅将新安装这一项从 `0` 改为 `1` 后，用户确认卡住解决。这支持首次开场动画播放触发媒体解码故障，而非登录网络卡住；该修改是绕过动画，不是修复 Wine 解码能力。

实际媒体修复已由用户确认：启用 Wine 构建的 GStreamer 支持，并用该新 Wine 执行 `wineboot --update` 同步已有 prefix。仅切换构建不足以修复旧 prefix 中缺失的组件；无需通过复制整套 SDO 文件或永久跳过动画解决。完整操作与验证边界见 [`wine-media.md`](wine-media.md)。

原版 DLL 是 **shim 路径的条件依赖**：shim 的进程附加入口执行后，会强制加载 `.sdo.dll` 并解析三个导出，失败则退出。Injector 路径通过随包的 `FfxivArgLauncher.ArgFixer` 替换游戏 SDO 登录函数，直接填写命令行中的 ticket 和账号 ID，不经过入口 DLL。分发程序集 CIL、版本固定的源码及同次 Injector 日志相互印证，不仅是 maps 无匹配的推测。Eorzea 的 Injector 途径在关闭 Dalamud 后仍保留该修补，不要求原版 DLL。详见 [入口 DLL 分析](sdologinentry64_analysis.md)。

## 与 XIVLauncher.Core 的对比

- `lib/FFXIVQuickLauncher/src/XIVLauncher.Common/Game/Launcher.cs` 的 `CheckBootVersion()` 在 CN 分支直接返回空列表。
- 同目录 `SdoLauncher.cs` 的 `EnsureLoginEntry()` 在原 DLL 不存在时仅复制 shim；原 DLL 存在且不是 ottercorp 版本时才生成 `.sdo.dll`。
- 上游资源 shim 与本机 shim SHA256 相同：`a7ae15660d00eb0b15e76902736721d2cc7450fed811dfbde879a07336ae65cd`。

因此此缺口不是少执行一次国际服 boot 更新；现有上游逻辑也不能证明国服空目录安装完整。

## 启动途径准备与验证边界

`[launch].method` 选择 `injector`（默认）或 `sdo_dll`，与 Dalamud 开关独立。两者均在重新认证前完成准备；不自动回退。DLL 途径不加载 Dalamud，但不会改动 Dalamud 开关。

`src/sdo_entry.rs` 动态读取官方清单和索引，按版本从新到旧寻找与当前清单一致的完整 DLL，不尝试未知差分重建。仅接受官方 HTTPS 更新路径，校验文件列表和完整 ZIP 的 MD5、ZIP 长度、精确成员的长度/MD5，以及 AMD64 PE DLL 的三个非转发可执行导出。元数据上限 2 MiB、DLL 4 MiB、单包 256 MiB、总归档下载 512 MiB；失败重试重新下载，不提供断点续传。

已完整下载并校验的样本：

- 目标清单版本 `0.0.0.26`；原版成员来自到 `0.0.0.25` 的 ZIP，路径 `Pkg\Launcher3Update\Launcher3Modules\sdologinentry64.dll`。
- ZIP 长度 `143346229`，MD5 `5D9DF9CAAFDCD7B5B99584DCFCB2B8DB`。
- 原版长度 `511352`，MD5 `ACCDEDB3A2399333A21F82F816CD739D`，SHA256 `afe7cac474ba49a93ee96aecc1b8b8fef12d36fee199e6c064fdd77941f52be1`。
- 导出 `SDOLGetModule`、`SDOLInitialize`、`SDOLTerminal` 和 `igw*`；静态导入均为 Windows 系统 DLL。用户 DllMain `0x1800025a0` 仅在进程附加时保存模块句柄；这些静态事实不代表完整运行路径均无其他依赖。

准备过程只部署 `sdo/sdologin/sdologinentry64.sdo.dll`（原版）与 `sdologinentry64.dll`（shim），不解压其他 ZIP 成员、不运行官方启动器。已有原版经结构/导出及非 shim 检查后保留，不强制与最新清单相同；无效备份报错而不覆盖。新下载的原版缓存于 `~/.eorzea/tools/sdo/<md5>.dll`，旁边 JSON 记录来源、版本和摘要；shim 使用固定 MD5 并缓存于 tools 根目录。安装锁与同目录原子写保护并发和写入失败，原版先于 shim 提交。

单元测试覆盖 PE/ZIP/URL/清单校验、无效备份保护和条件依赖；真实官方样本的隔离幂等部署及空目录官方网络下载准备测试均通过，未执行任何 DLL 或游戏。`game verify` 仅对 DLL 途径检查两个入口；这是静态预检，不是 Wine 加载验证。还需在用户许可下实际验证两条途径进入角色列表/游戏，以及中断恢复、升级兼容性；不能宣称完整运行时验证已完成。

调查下载材料保存在 `/tmp/opencode/ffxiv-install-investigation/`；该临时目录不是持久缓存或可发布依赖。
