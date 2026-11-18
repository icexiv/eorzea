# eorzea-auth 与 XIVLauncher.Core 差异追踪

> 本文件记录 Rust `auth` 子 crate 与上游 C# (`XIVLauncher.Core`) 实现之间的所有已知差异。
> 当前聚焦 **SDO（中国服/盛趣）** 登录功能。`se`（国际服）为低优先级。

---

## P0 — SDO 核心登录链路（必须先通）

### P0-1 设备指纹生成 (`SdoUtils.cs`)

- [x] **实现 `generate_device_id()`**：格式为 `{MD5_UPPER(MAC)}:{MD5_UPPER(CPU_ID)}:{MD5_UPPER(磁盘序列号)}` — `sdo_device::get_device_id()`
- [x] **实现 `generate_mac_id()`**：格式为 `MD5_UPPER(MAC地址)` — `sdo_device::get_mac_address_hash()`
- [x] **跨平台硬件信息采集**：macOS 用 `ioreg` + `diskutil`，Linux 用 `/etc/machine-id` + `/proc/cpuinfo` + `lsblk`
- [ ] **Windows 设备指纹采集**：C# 用 P/Invoke `iphlpapi!GetAdaptersInfo`（MAC）、CPUID 指令（CPU）、WMI `Win32_DiskDrive.SerialNumber`（磁盘）；Rust 三者目前全部走 `HOSTNAME-USER` fallback。Windows 上 `HOSTNAME`/`USER` 通常不存在，实际得到 `unknown-<USERNAME>`（用户名变更/换用户即漂移）。详见 [`docs/notes/auth.md`](./docs/notes/auth.md)
- [x] **集成到 `SdoAuth`**：`SdoAuth::new()` 自动采集，`common_query()` 使用自身存储的值，`SdoContext` 不再需要调用方传入

> C# 参考：`SdoUtils.cs` — `GetMacAddress()`, `GetCPUId()`, `GetDiskSerialNumber()`, `GetDeviceId()`, `GetMD5()`
>
> **⚠️ 存疑**：Rust 与 C# 的 MD5 输入字符串格式存在系统性差异（C# `DeviceId` 库默认加 `Key=` 前缀，Rust 没有），导致同一台机器指纹不同。详见 [`docs/notes/auth.md`](./docs/notes/auth.md)。
> 待启动功能实现后通过实际登录测试验证：若 SDO 服务端仅要求"稳定唯一"则无需修复；若触发风控（频繁要求验证码/拒绝密码登录），则需复刻 C# 的 `Key=Value` 格式。

### P0-2 SDO HTTP 请求细节 (`SdoLauncher.cs` → `GetSdoHttpRequestMessage`)

- [x] **自动注入 Cookie**：每个请求注入 `CASCID=CID{MD5(MAC)}` 和 `SECURE_CASCID=CID{MD5(MAC)}` 及 `_rsid=""`（通过 `build_cookie_header()` 方法）
- [x] **`getPromotionInfo` 补全 `serviceUrl` 参数**：已添加 `serviceUrl=http%3A%2F%2Fwww.sdo.com`
- [x] **`getPromotionInfo` 请求/响应对齐**：请求已补全 `common_query()` 和完整 CAS Cookie，并解析 JSON、检查 `return_code`/`error_type`；激活失败会中止登录
- [x] **`macId` 参数语义对齐**：`common_query()` 发送 `SdoUtils.GetMac()` 对应的原始设备标识，密码登录 `mac` 和 `CASCID` 继续使用其 MD5
- [x] **请求头 `Host`**：所有 SDO 请求添加 `Host: cas.sdo.com`
- [x] **SDO HTTP/2 兼容性（2026-10-31 实测）**：本机 `reqwest` 自动协商 HTTP/2 时，主/备用域名的 `getGuid.json` 均返回 HTTP 567 + HTML；同一请求强制 HTTP/1.1 后均返回 HTTP 200、`return_code=0`。移除环境代理后仍可复现 HTTP/2 失败。原生 `SdoAuth` 客户端已限制 HTTP/1.1，三个 JSON 请求辅助方法均在解析前检查 HTTP 状态（错误不附带含凭据的 URL），并添加 HTTP 567 HTML 响应回归测试；CLI 不再将上下文/网络失败统一提示为 session key 过期。

### P0-3 SDO 错误处理完善

- [x] **错误码保留**：`AuthError::SdoError { code, message, remove_auto_login }` 现在保留原始 `return_code`
- [x] **自动登录过期标记**：`SdoError.remove_auto_login` 字段标记是否需删除本地 session key
- [x] **`SdoLoginData.ErrorType` 字段**：已添加 `error_type: Option<i32>`

### P0-4 域名故障转移

- [x] **`with_fallback_url()` 方法**：可在创建后切换到备用域名 `n1.cas.sdo.com`

---

## P1 — SDO 扫码/推送/自动登录完善

### P1-1 扫码登录后处理 (`SdoLauncher.cs` → `QrCodeLogin`)

- [x] **`getAccountGroup`/`accountGroupLogin`**：扫码/推送确认后调用 `account_group_login` 刷新 tgt 并获取 `auto_login_session_key`（对应 C# `AccountGroupLogin`），`launch` 自动登录链路打通
- [x] **自动登录续期**：`autoLogin.json` 每次返回新 key + `autoLoginMaxAge`（剩余期限）；`fast_login`（`fastLogin.json`）再刷新 tgt/snda_id 对齐 C# `LoginBySessionKey`；`launch` 自动登录后保存新 key 到配置（旧 key 立即作废），并在输出显示剩余有效期
- [x] **密码登录保存可续期凭据**：`login_password` 与扫码/推送共用 `finalize_login`，通过 `accountGroupLogin` 获取 session key；添加账号后启动统一重新认证，不依赖交互登录时的短期 ticket。回归测试覆盖静态登录不带 key 的响应。
- [x] **选择 `sndaId`**：`get_account_group` 返回 `SdoAccount { snda_id, account_name }` 配对列表（`sndaIdArray`×`accountArray`，对应 C# 取显示名逻辑）；`QrCodeSession`/`PushLoginSession` 暴露 `poll()` + `finalize(data, snda_id)`，多账号场景前端可自行轮询、展示账号组后传入用户选择的 `snda_id` 完成登录
- [x] **QR 扫码完整链路**：`qr_code_request` → 轮询 → `sso_login(tgt)` → ticket 获取已验证通过

### P1-2 推送/滑动登录超时与取消

- [x] **超时机制**：`QrCodeSession::wait_for_scan(None)` 默认 300s（`QR_CODE_TIMEOUT`）、`PushLoginSession::wait_for_confirm(None)` 默认 30s（`PUSH_LOGIN_TIMEOUT`），均对齐 C# 过期时间；`Some(d)` 可覆盖
- [x] **Launcher 层推送登录封装**：`Launcher::request_push_login(account)` → `PushLoginSession`（`serial_num()` 展示验证序号、`poll()`/`wait_for_confirm()` 轮询、`finalize()` 收尾），内部自动先取消上一轮推送（`cancelPushMessageLogin`）
- [ ] **取消推送**：`slide_login_request` 之前应该调用 `cancelPushMessageLogin`，当前已实现但逻辑应更健壮

### P1-3 WeGame 登录

- [ ] **`thirdPartyLogin` 实现**：`companyid=310`, 传入 WeGame `userId` 和 `token`
- [ ] **WeGame SID 直接构造**：跳过 SDO 认证，直接构造 `OauthLoginResult`

---

## P2 — 游戏启动参数构造

### P2-1 国服启动参数 (`SdoLauncher.cs` → `LaunchGameSdo`)

- [x] **启动参数构造**：`game.rs` 中 `build_sdo_launch_args()` 实现完整参数拼接
- [x] **参数分隔符修正**：C# `ArgumentBuilder.Build()` 使用 `key=value` 格式（不是空格分隔）
- [x] **`areasInfo` 计算**：`build_lobby_hosts()` 将所有大区 `lobby:54994` 用 `|` 分隔
- [x] **`XL.DcTraveler` 参数**：当 `dc_travel_port > 0` 时添加
- [x] **端到端验证**：`sdo_login` 示例完成 QR 扫码 → `sso_login` → ticket 获取 → 启动参数构造
- [x] **`sdologinentry64.dll` 替换**：`EnsureLoginEntry()` — 自动从 ottercorp GitHub 下载修改版 DLL，缓存到 `~/.eorzea/tools/`，复制到 `{gamePath}/sdo/sdologinentry64.dll`
  - [x] **确认无原版仍可启动的原因**：实际 Injector 在恢复游戏前调用随包 `FfxivArgLauncher.ArgFixer.Fix()`，替换 SDO 登录函数并从启动参数填写 ticket/账号 ID，不经过 shim。REA CIL、固定提交源码及同次 Injector 日志相互印证。`--without-dalamud` 判断在修补之后，修补不依赖游戏内 Dalamud。详见 [DLL 与启动路径证据](docs/notes/sdologinentry64_analysis.md)。
  - [x] **解耦国服启动修补与 Dalamud 开关**：`launch.method = injector|sdo_dll`（默认 Injector），CLI/GUI 独立选择。Injector 关闭 Dalamud 仍通过 `--without-dalamud` 保留 ArgFixer、版本门控和 .NET 准备，跳过 assets；DLL 途径独立准备官方原版 + 固定摘要 shim，保护已有备份，仅条件预检。准备/冲突错误均先于账号认证，不自动回退。官方 ZIP 下载校验与隔离目录幂等部署测试通过，见 [`docs/notes/sdo-installation.md`](docs/notes/sdo-installation.md)。
  - [ ] **双途径实际游戏验证**：在用户许可下，对最小安装分别验证 Injector 不加载 Dalamud、Injector 加载 Dalamud、原版 + shim 从标题到角色列表/进入游戏；当前验证覆盖静态结构、官方完整下载和隔离部署，不代表 Wine 运行时兼容性已验证。
  - **已修复（2026-08-06）**：`is_ottercorp_dll()` 原先按 UTF-8 搜索 `"ottercorp"`，但 PE version info 中该字符串是 UTF-16LE 存储，检测恒为 false → 每次启动都把当前 DLL 备份覆盖 `.sdo.dll`，第二次启动即永久覆盖原版备份，修改版 shim 转发到自身导致游戏内 5003「帐号认证发生了错误」。现已同时匹配 ASCII/UTF-16LE 两种编码，且已有 `.sdo.dll` 备份时不再覆盖

### P2-2 参数加密 (`ArgumentBuilder.cs` → `BuildEncrypted`)

- [ ] **Blowfish ECB 实现**：需实现 SE 的变体 Blowfish（带 signed byte bug）
- [ ] **启动参数加密流程**：
  1. 取 `Environment.TickCount` 作 key
  2. 构建 ` /key =value` 格式参数串
  3. LegacyBlowfish ECB 加密
  4. SE Base64 变形编码 (`+→-`, `/→_`, `=*`)
  5. 计算校验字符
  6. 结果格式：`//**sqex{version:D04}{base64}{checksum}**//`

### P2-3 版本检查与补丁 (`Launcher.cs` → `CheckGameUpdate`)

- [x] **版本报告生成**：`game_files/version.rs` 的 `build_version_report()` 按 `max_expansion` 动态生成 ex1-ex5（不再硬编码），对齐 C# `GetVersionReport()`（首行为硬编码 boot hash，国服同 C# FIXME 值）
- [x] **SDO 版本检查协议**：`game_files` 的 `check_update()` 实现 `CheckGameUpdate`（POST `{area_patch}/http/win32/shanda_release_chs_game/{ver}`，`X-Hash-Check` 头，解析 `X-Patch-Unique-Id` + TSV），免登录，已通过真实 API 验证（返回 185 补丁/126 GiB）
- [x] **补丁列表解析**：`game_files/patch_list.rs` 实现 `PatchListParser`（跳过前 5 行、9 字段带 hash / 6 字段 boot），`PatchListEntry` 扩展为完整字段（`hash_type`/`hash_block_size`/`hashes[]`）
- [x] **补丁下载管理**：`game_files/patch_manager.rs` 实现下载管线（并发 4 槽同 C# `MAX_DOWNLOADS_AT_ONCE`、SHA1 块校验同 `CheckPatchValidity`、已校验文件跳过、进度回调）；SHA1 块算法已用真实补丁验证通过
- [x] **修复跨仓库补丁缓存名碰撞（2026-08-06）**：缓存名加入完整 URL 的 SHA1 身份，`ffxiv`、`ex1`-`ex5` 的同名补丁不再互相覆盖；安装前再次校验长度/逐块 SHA1，不匹配时禁止应用和写 `.ver`。旧的 basename + version 歧义缓存不会再被复用
- [x] **eoz 命令行**：`packages/eorzea-cli`（bin `eoz`，clap）— `areas` / `game status` / `game check` / `game update`（下载 + 应用）
- [x] **eoz auth**：多账号管理 — `auth login qr|password|auto` / `auth status` / `auth default <账号>` / `auth logout`；配置持久化到 `~/.eorzea/auth.toml`（`--config` 或 `XIV_LAUNCHER_RS_CONFIG` 覆盖）；`launch` 未指定账号时用默认账号自动登录
- [x] **eoz login/launch**：登录 + 启动游戏；扫码二维码通过终端图片协议直接显示（kitty graphics protocol / iTerm2 OSC 1337，`src/term_img.rs`），无协议时 fallback 保存 PNG
- [x] **补丁应用**（ZiPatch）：`src/game_files/zpatch/` 完整移植 C# `ZiPatch` 解析与应用（FHDR/APLY/SQPK:T/F/A/D/E/H/I/X/ADIR/DELD/EOF），`RemotePatchInstaller` 流程（应用 → `SetVer` → `VerToBck`）；已用修复后的唯一缓存键重放 11 个补丁（877.50 MiB），版本检查通过且实际启动进入游戏
- [x] **安装进度与流式应用（2026-10-30）**：`InstallProgress` / `InstallPhase` 上报总进度和当前补丁的「校验中/应用中」；ZiPatch 改为 `read_chunk` 流式解析（一次只保留一个 chunk），不再把 1.4 GiB 补丁整文件读入内存；CLI 用 `InstallView` 显示总进度 + 当前补丁子进度
- [x] **跳过安装前重复 SHA1**：新增 `install_verified()`；`download` 成功后的 CLI/GUI 安装跳过第二次整文件 SHA1，仅做长度检查，避免 84 GiB 缓存被重复读一遍；独立调用 `install()` 仍保留完整校验
- [ ] **Boot 版本检查**：国服无需实现（C# `CheckBootVersion` 对 CN 直接 `return Array.Empty`）
- [x] **完整性校验**（`game verify`）：`game_files/verify.rs` 文件级校验（.ver 一致性、关键文件存在性、sqpack 魔数/header/数据块结构、movie）；注：C# `PatchVerifier` 依赖 ottercorp S3 的 `.patch.index`（国际服专用，国服无此服务），故不做逐文件 hash 校验
- [x] **断点续传**（Range）：不完整缓存直接作为 `.part` 使用；下次启动先按块 SHA1 校验已有完整前缀（损坏/超长则截断），再发 `Range: bytes=<offset>-` 续传。服务端忽略 Range 返回 200 时自动清空重下；每个文件最多 5 次尝试，失败保留已下载部分（2026-10-30）
- [ ] **单文件多连接下载**：当前每个补丁 1 条连接、全局最多 4 条；C# 默认 aria2 每文件 `max-connection-per-server=8`（4 文件最多 32 条），NetDownloader 也按 `ChunkCount=8` 分块并行。需按 Range 分块 + 随机写盘，否则大补丁只能吃单连接带宽
- [x] **默认暂存目录改到游戏目录**：`default_patch_dir()` 返回 `<游戏目录>/.eorzea/patches`，避免把约 100 GiB 补丁缓存写进系统盘；CLI `--patch-dir` 仍可覆盖（2026-10-30）
- [ ] **下载前磁盘空间检查**：C# `PatchManager.PatchAsync` 会检查补丁目录/游戏目录剩余空间并抛 `NotEnoughSpaceException`；当前没有检查，补丁目录写满后会在写入/重试阶段表现为卡住或失败
- [x] **实时下载进度**：`download_patches` 用 `JoinSet` 乱序收任务，主任务每 120ms 上报一次；`DownloadProgress` 同时给出总字节和每个在途补丁的子进度。CLI 用 `DownloadView` 绘制一行总进度 + 每个补丁一行子进度（`--no-progress` / 非 TTY 自动退化为总量）。已存在/跳过文件也计入总进度；子进度条先挂到 `MultiProgress` 再设置样式/消息，避免 `set_message` 的即时绘制留下重复行（2026-10-30）
- [x] **校验不占下载槽位**：网络下载完成后先释放信号量，再执行 SHA1 校验；否则 4 个大补丁同时校验时所有槽位被占住，网络空转，表现为“每批下完停一段时间 / 越下越慢”（2026-10-30）
- [x] **校验阶段进度显示**：`PatchProgress.phase` 区分 `Downloading` / `Verifying`；子进度在后者把速度列换成「校验中」，总进度在没有下载任务时也显示「校验中」，不再用 0 B/s 和超大 ETA 误导（2026-10-30）
- [x] **dev profile SHA1 优化**：`[profile.dev.package.sha1] opt-level = 3`；否则 debug 版 SHA1 只有约十几 MiB/s，1.4 GiB 补丁的校验会占用几分钟 CPU。实测优化后约 269 MiB/s（2026-10-30）
- [x] **下载超时与重试**：`reqwest::Client` 配 `connect_timeout=15s` / `read_timeout=30s`；单文件最多 5 次尝试，失败后退避并从当前文件长度续传。仍保留“任一文件最终失败则中止整批”的行为（2026-10-30）
- [x] **边下边装**：共享 `GameFileManager::update` 按序领取下载任务、默认 4 路并发，预取窗口为当前补丁 + 并发数；独立后台 routine 仅消费已下载并校验通过的队头，提交且删除成功才推进。CLI/GUI 同时展示应用总进度/当前补丁和下载总进度/各并行任务，GUI 更新使用根状态与 `spawn_forever`，切换页面不中断。测试覆盖下载与应用重叠、乱序下载不乱序安装、有限预取。
- [x] **CLI 应用进度防闪动**：每轮刷新只设置最终应用/等待文案，安装总进度样式仅初始化一次，避免等待队头下载时文案反复切换；测试覆盖连续等待及等待/应用状态切换。
- [x] **CLI 预取队列可见性**：已下载并校验通过的补丁保留为「✓ 等待应用」，开始应用时才从下载列表移除；更新快照单独提供待应用列表，不计入活动下载或重复累计字节。
- [x] **可选最少下载并发**：CLI `--min-concurrency` 默认 0，保留有界预取；非零时允许突破预取窗口以补足下载任务，但不超过 `--concurrency` 上限，可能积压更多缓存。测试覆盖慢队头不阻止后续下载及上下限校验。
- [x] **共享下载并发配置**：`config.toml` 的 `[download]` 保存最大/最少并发，CLI 显式参数单次覆盖且支持 `eoz config` 读写；GUI 设置页提供草稿编辑、校验、恢复与保存，主页更新使用已保存配置。默认最大 4、最少 0。
- [x] **单补丁中断恢复与提交**：`<游戏目录>/.eorzea/update.json` 保存完整有序清单、缓存目录和消费游标，不记录 chunk 断点；恢复优先于服务端版本检查，当前补丁从头重放。按游戏数据同步 → 原子提交 `.ver` → 删除补丁 → 持久化队头顺序推进，删除失败不安装后继；队列完成后更新 `.bck` 并清理清单。更新目录使用文件锁，未完成时拒绝启动游戏。测试覆盖缺失 `.ver`、部分游戏文件、删除后游标未推进、清理失败重试及互斥。详见 [`docs/notes/patch-interruption-recovery.md`](docs/notes/patch-interruption-recovery.md)。
- [x] **ZiPatch 删除与句柄失效**：删除文件/目录树前清理相关缓存句柄，对齐 C# `CloseStream`；删除及 RemoveAll 枚举/删除失败均返回错误，NotFound 可重试。回归测试覆盖删除后重建和 RemoveAll 后重放。
- [x] **ZiPatch 完成与落盘校验**：显式要求读到 `EOF_`，文件使用 `sync_all()`，Unix 上同步变更目录；失败禁止推进 `.ver`。缺结束标记的补丁即使 SHA1 匹配也不能完成安装。
- [ ] **无 SHA1 补丁 CRC 校验**：当前下载对无 hash 的 boot 补丁只检查长度；安装检查结构与 EOF，但未对齐 C# `CheckPatchValidity` 的逐 chunk CRC 校验。
- [x] **缺失前置补丁不得跳过安装**：独立 `install` 返回缺失错误；流水线按原清单重新下载，禁止跳过后推进版本。
- [ ] **流水线实机恢复验证**：在 Windows/macOS 及真实发行补丁上验证更新、强杀重启和断电恢复，重点检查原子版本替换、目录元数据持久化及文件占用导致的清理失败；当前回归使用 Linux 本地 HTTP 和合成补丁，GUI 进度组件已有虚拟 DOM 渲染测试。
- [x] **SHA1 流式校验**：`verify_patch_sha1` 改为 `BufReader` 分块读取，不再把 1.4 GiB 补丁整文件读入内存；校验通过 `spawn_blocking` 移出 tokio worker，文件写入改用 `tokio::fs`（2026-10-30）
- [ ] **UID 缓存**：缺失 `IUniqueIdCache` 实现（`X-Patch-Unique-Id` 缓存）

### P2-4 Wine 配置与启动环境 (`WineSettings.cs` / `CompatibilityTools.cs` → `src/wine.rs` + `src/config.rs`)

- [x] **`WineSettings` 配置模型**：`startup_type`（Auto/Managed/Custom/System）、`custom_path`、`prefix`、esync/fsync/msync、`debug_vars`、自定义 `env`、DXVK 设置（enabled/hud/frame_limit）、gamemode — `src/config.rs`
- [x] **配置持久化**：拆分存储 — `~/.eorzea/config.toml`（`AppConfig`：顶层 `game_path` + Wine 设置 + `[dalamud]`）+ `~/.eorzea/auth.toml`（账号，TOML）；旧 `settings.json`/`eorzea.toml` 自动迁移。`game_path` 与 Wine 无关，已从 `WineSettings` 上移到 `AppConfig`；CLI 的 `--game-path` 缺省时从 config 读取
- [x] **`WineTool::resolve(&WineSettings)`**：配置 → 运行时解析（Auto = 自定义→托管→系统→下载；Managed/Custom/System 显式分派），`custom_path` 支持 wine64 文件或 bin 目录归一化
- [x] **`WineTool::probe()`**：`wine64 --version` 校验可执行性
- [x] **`build_launch_env()`**：对齐 `CompatibilityTools.RunInPrefix` 环境变量（`WINEDLLOVERRIDES`、`WINEESYNC/WINEFSYNC/WINEMSYNC`、`WINEDEBUG`、`DXVK_STATE_CACHE_PATH`/`DXVK_CONFIG_FILE`/`DXVK_HUD`/`DXVK_FRAME_RATE`、`LD_PRELOAD` gamemode、自定义 env 覆盖）
- [x] **每次启动指定不同 wine**：`Launcher::with_wine_settings`（持久）+ `Launcher::launch_with_wine`（单次覆盖）
- [x] **CLI `--wine` 覆盖传递**：`eoz launch` 将 `launcher.wine_settings()` 传入共享 `LaunchRequest`，不再在创建游戏进程时重新加载并覆盖本次选定的 Wine 配置。
- [x] **Wine 非 ASCII 游戏路径预检**：Linux/macOS 启动前拒绝包含非 ASCII 字符的完整游戏路径并返回明确错误，避免游戏内报误导性的 5003「帐号认证发生了错误」
- [ ] **首次开场动画的媒体解码故障提示**：本机 nixpkgs `wineWow64Packages.staging` 默认未启用 GStreamer，Wine 11.21 的 `wmadmod.dll` 因依赖缺失加载失败。用户启用 GStreamer 并以新 Wine 执行 `wineboot --update` 后确认恢复，prefix 已补齐桥接 DLL。需媒体能力提示，不能把模块缺失误报为登录故障或静默关闭动画。完整根因、操作与验证边界见 [`docs/notes/wine-media.md`](docs/notes/wine-media.md)。
- [ ] **同版本 Wine 构建切换的 prefix 同步**：用户已确认启用 GStreamer 的 Wine 11.21 配合 `wineboot --update` 可修复旧 prefix 缺少新增组件的问题。`ensure_prefix()` 对已有 win64 prefix 仍不做构建切换同步；需设计不只依赖版本字符串的构建身份检测及安全更新，兼顾 DXVK 被覆盖的处理，禁止无条件删 prefix。见 [`docs/notes/wine-media.md`](docs/notes/wine-media.md)。
- [ ] **DXVK 变体选择**：当前固定 dxvk-async 1.10.1（CN 镜像）；上游已切换 dxvk-gplasync，未实现
- [ ] **同步设置的平台能力统一**：GUI 已按平台隐藏不支持的开关（esync/fsync、gamemode 仅 Linux 显示，msync 仅 macOS 显示；配置字段仍跨平台保留），esync/fsync 在模型层合并为互斥的 `SyncMode` 枚举（配置文件仍写 `esync`/`fsync` 两个 bool，旧配置同时为 true 时按 fsync 读入，GUI 用「关 / esync / fsync」三档滑块表示）；剩余不一致：GUI 在 macOS 隐藏 esync（上游 Linux/macOS 都传 WINEESYNC，仍依赖所选 Wine 构建支持），CLI 在 Linux/macOS 均接受 esync/fsync/msync/gamemode，运行时 fsync 未限制 Linux。应统一能力规则（fsync/gamemode 仅 Linux、msync 仅 macOS、esync 依 Wine 构建），导入其他平台配置时保留且提示忽略，显式修改不支持的项时拒绝或明确告警，并覆盖按平台测试。
- [ ] **现代 Wine 的 NTSYNC 识别**：Wine 10.16 开发版/11.0 稳定版已集成 NTSYNC，Linux 主线 6.14 起提供驱动，但实际可用还依赖 Wine 构建、内核配置/模块、/dev/ntsync 权限等。当前只有 esync/fsync/msync 三个布尔字段，不足以表达“Wine 默认选择”与“强制传统 wineserver”；三个字段全关不能据此推断 NTSYNC 关闭。需区分用户请求、环境能力和实际后端，不依据 Wine 或内核版本号单独承诺可用。
- [ ] **跨平台配置保真与语义校验**：Windows 的 AppConfig 忽略 Wine 字段，但 GUI/save_app 整体序列化后会丢弃这些外平台字段；CLI 原始 TOML 保存路径则可保留，行为不一致。类型校验不能保证其他平台的 game_path/install_root 等路径可用。需明确保留外平台配置的策略，并在实际使用本平台设置前校验路径与能力；不能把“能解析”当作“当前平台可用”。
- [ ] **NixOS 适配**（详见 `docs/notes/nixos.md`）：
  - [ ] `Auto`/`Managed` 下 `probe()` 失败回退系统 wine 并给出 NixOS 提示
  - [ ] 托管 wine 跑不起来且 PATH 有 `steam-run` 时自动用 steam-run 包装（需 `unset TZ` 规避 nixpkgs#279893）
  - [x] NixOS 用户上手指南已写入 `docs/notes/nixos.md`（系统 wine / nix-ld / steam-run 手动包装三条路径）
- [x] **wine/游戏日志重定向**：launch 时 wine/游戏输出写入 `~/.eorzea/logs/game-{ts}.log`，CLI 终端不再被污染；`GameLaunchResult.log_path` 暴露日志路径
- [ ] **`WineSettings.log_file` 字段**：可配置日志路径（当前为默认路径，GUI 配置项待加）
- [x] **prefix 架构检测修复**：`detect_prefix_arch` 之前只读 `system.reg` 第一行，而真实 wine 文件头是 `WINE REGISTRY Version 2`（`#arch=` 在第 3~4 行），导致每次启动都误判架构并**删除重建 prefix**（反复出现 "configuration in prefix is being updated"、DXVK 反复重装、Injector 启动时 dxgi.dll 丢失）；已改为扫描头部 8 行
- [x] **DXVK 安装检测修复**：`ensure_dxvk` 之前只看 `d3d11.dll` 是否存在，而 wineboot 重建 prefix 后会放回 builtin d3d11.dll（DXVK 已被覆盖），导致误判"已安装"并在 `dxgi=n` override 下报 `dxgi.dll not found`；现改为 d3d11.dll + `.dxvk-installed` 标记文件双重判断
- [x] **prefix 引导 `EnsurePrefix()`**：对齐 C#，wineboot 后运行 `cmd /c dir %userprofile%/Documents` 强制 prefix 初始化完成；修复 macOS 上新建 prefix 后首次 Dalamud 注入启动因 prefix 未就绪失败（"Game exited prematurely"）的竞态（2026-09-10）
- [ ] **`wineserver` 管理**：C# 有 `wineserver` 路径处理，未实现
- [x] **macOS 兼容性修正（2026-09-09）**：`flake.nix` devShell 支持 darwin（Linux-only 运行时库/LD_LIBRARY_PATH 门控，darwin 用 apple-sdk）；`build_launch_env` 的 gamemode 改为 Linux-only（macOS 忽略并告警）；wine 测试断言按平台分派
- [ ] **macOS 端到端验证**：xom wine + Rosetta 链路（下载 → prefix 初始化 → DXVK-macOS → 启动）代码就绪但未完整真机回归；偶发 Wine 死锁 `RtlpWaitForCriticalSection` 待排查

---

## P2-5 Dalamud 插件框架集成（新）

- [ ] **归档路径跨平台校验**：Linux 下 `dalamud::archive::tests::test_safe_relative_path_rejects_escape` 对 `C:\Windows\evil.dll` 的拒绝断言失败；需独立于宿主路径语义识别 Windows 盘符与反斜杠路径。

- [x] **调查报告**：`docs/notes/dalamud_integration.md`（加载机制、XIVLauncher 组件、Linux Wine 内注入、分阶段路线）
- [x] **阶段 0 骨架**：`src/dalamud/`（model/updater/runner）— release 元数据获取（`VersionInfo` API）、版本门控（`SupportedGameVer == 游戏版本`）、本机安装检测、Injector argv 构造、Wine 路径转换（`winepath --windows`）、Injector JSON 解析；`eoz dalamud status/launch`；`config.toml [dalamud]` section（flatten 兼容旧配置）
- [x] **release 下载安装（惰性）**：归档下载（进度回调）+ 关键文件校验 + `hashes.json` 完整性校验（远端 `Hash` = manifest 文件 MD5、逐文件 MD5、UTF-16/UTF-8 manifest 解码、拒绝 `..`/绝对路径）+ 原子安装（`Hooks/<AssemblyVersion>`）+ 写 `version.json` + 清理旧版本目录（保留 `dev`）；`launch` 时版本匹配但未安装自动下载（无需独立 install 命令也能跑）
- [x] **纯 Rust 归档解压（2026-09-12，去掉外部 7z）**：新增 `src/dalamud/archive.rs`，用 `sevenz-rust2` 解 7z（LZMA2/BCJ/COPY/DELTA/PPMd/Deflate）、`zip` crate 解 zip，按**文件头**识别格式而不是看扩展名；安全边界为拒绝绝对路径/`..`/盘符（zip-slip）、条目数与解压总量上限、只写临时目录后原子 rename。此前依赖 PATH 里的 `7zz/7z/7za`（缺失时直接 `NotImplemented` 报错），现在 Linux/macOS 不需要 `p7zip`、Windows 不需要 7-Zip；顺带把 `sevenz-rust2` 关掉默认 feature（`aes256` 会拉 getrandom 0.4 导致 `dx serve --platform web` 的 wasm32 构建失败）
- [x] **`eoz dalamud install` 预安装命令（2026-09-12）**：把「release → runtime → assets」三组件显式安装成 CLI 子命令，与 `launch` 的惰性安装共用 `updater::ensure_release` / `ensure_runtime` / `ensure_assets` 与同一套版本门控；差别只在失败行为（install 报错非零退出，launch 静默降级）。已安装且校验通过时跳过下载；`--game-path` 缺省读 config
- [x] **安装检测确定性（2026-09-12）**：`detect_local_install` 此前按 `read_dir` 顺序返回第一个含 `Dalamud.Injector.exe` 的目录，多个版本并存时可能取到旧版本（旧版本 hash 校验必然失败 → 每次启动都重下）；改为取版本号最大者，并在安装成功后清理其它版本目录
- [x] **启用即强制：去掉静默降级（2026-09-12）**：此前 `build_dalamud_config` 在元数据不可达/版本门控不匹配/下载校验失败/runtime 或 assets 缺失时都 `Ok(None)`，游戏照常启动但不加载插件——CLI 还先打印「走 Injector」，用户完全看不出来。现在改为 `Launcher::prepare_dalamud`：启用（配置或 `--dalamud`/GUI 本次开关）后任一步不满足都返回 `LauncherError::DalamudNotReady`，**报错且不启动游戏**，消息里给出原因与逃生口（`--no-dalamud` / GUI 关闭本次开关 / 关闭配置）；版本门控不匹配同样报错（此前是"安全降级"）。准备提前到**登录之前**（新增 `Launcher::launch_prepared`：prepare → 登录 → 启动），否则一次失败会让一次性 SSO ticket 作废、逼用户重新登录。CLI 的「Dalamud: …」提示改为在 prepare 之后打印真实结果，不再重复请求元数据
- [x] **远端元数据不可用时的本地降级（2026-09-12）**：`status()` 现在对 `VersionInfo` 请求重试（8s/次超时、4xx 不重试；**本机已有安装时只试 1 次**，避免离线启动白等数十秒），并打印真实错误（此前 `.ok()` 把错误吞掉，只留下「元数据不可用」）；仍失败则退回 `Hooks/<版本>/version.json`（安装时保存的服务端元数据）继续做版本门控，`assets` 元数据失败退回本地 `dalamudAssets/<asset.ver>`。修复用户实测：本机 15.0.3.4 已装且版本匹配，仅因一次元数据请求失败就 `launching without Dalamud`；现在本地三件套齐全时完全离线也能加载（`DalamudStatus.remote_from_cache` 标记来源，CLI/GUI 均显示）
- [x] **CLI 输出层重做（2026-09-12）**：新增 `packages/eorzea-cli/src/ui.rs` —— 统一 `==> 标题` / `✓` / `!` / `✗` / `·` 状态行与按显示宽度对齐的表格（中英混排不错位）；下载改用 **indicatif 进度条**（阶段名/百分比/速度/ETA），日志经 `tracing-indicatif` 的 `IndicatifWriter` 写出，写日志时自动挂起进度条不再花屏；非 TTY 或 `--no-progress` 自动退化为「阶段开始一行 + 完成一行（含大小）」，管道/CI 可读。**日志默认静默**（原默认 info，`INFO eorzea_auth::sdo: Creating SdoAuth client` 这类内部步骤直接糊在用户输出里），改由 `-v/-vv/-vvv` 或 `RUST_LOG` 打开；已处理的降级路径（元数据回退、assets 回退、单次重试失败）从 warn 降为 info/debug，不再刷噪音。stdout 只放结果、stderr 放日志与错误，`eoz config list > backup.toml` 之类不再混入日志。`Launcher::prepare_dalamud` 新增进度回调（`DalamudPhase`），`eoz launch` 与 GUI 状态栏都能显示「正在准备 release 本体 / runtime / assets」及字节进度
- [x] **资产元数据重试（2026-09-12）**：`fetch_asset_meta` 与 `VersionInfo` 一样做退避重试（3 次、4xx 不重试）——实测 aonyx 在并发/连续请求下会偶发 connection error，此前资产元数据一次失败就直接报错（`ensure_assets_real` 集成测试同时跑三个下载时稳定复现，现已连续通过）
- [ ] **GUI 缺少 Dalamud 安装入口**：设置页/主页目前只能靠「启动」触发惰性安装（失败即报错不启动），没有 `eoz dalamud install` 对应的显式按钮与进度展示
- [x] **Windows x64 .NET runtime 下载/组装**（`RuntimeVersion` 管理；NuGet/华为镜像，提取 native/lib 并组装 host/fxr；上游 Runtime/Hashes manifest 与 NuGet 实际文件不一致，未作为阻断条件）
- [x] **Dalamud assets 获取**（Asset Meta 版本管理 + 单文件 SHA1 校验/镜像 fallback；上游 Noto 字体 hash 与镜像文件不一致，按 C# 行为接受镜像文件并告警）
- [x] **launch backend 切换**：`launch_game` 支持 `dalamud` 配置时走 Injector（winepath 转换 + Injector 启动 + JSON 解析）；`Launcher::launch_with_options` 自动读 `[dalamud].enabled`（可被 CLI 覆盖）；`eoz launch --dalamud/--no-dalamud` 覆盖配置（2026-09-12 起启用即强制：未就绪直接报错，见下条「启用即强制」）
- [x] **Dalamud 启动链路纠错**：`build_dalamud_config` 从 `ffxiv_dx11.exe` 路径正确推导游戏根目录（此前把 exe 当根目录导致版本恒不匹配）；路径对齐上游 storage root（`dalamudConfig.json`/`logs`/`installedPlugins`/`dalamudAssets`，不再嵌套 `dalamud/`）；runner 宿主 `current_dir` 用 Unix Injector 目录（此前误传 `Z:\...`）；Injector stderr 后台排空避免管道阻塞；检测 Windows .NET runtime（本项目或 `~/.xlcore_cn` 版本匹配 fallback）与配套 assets，缺失时安全降级；release 元数据不可用不再 panic；`eoz dalamud launch --no-dalamud` 不再被忽略
- [ ] **阶段 2+**：Wine PID→Unix PID 映射、staging/beta、崩溃恢复（safe mode）
- [x] **原生 Windows Dalamud 启动（2026-09-12）**：`launch_game` 的 `#[cfg(target_os = "windows")]` 分支支持 `config.dalamud` —— 原生运行 `Dalamud.Injector.exe`（无 winepath/`XL_WINEON*`/`WINEPREFIX`，路径直接传递，`DALAMUD_RUNTIME`/`DOTNET_ROOT` 指向原生 runtime 路径），对应 C# `WindowsDalamudRunner.cs`；新增 `GameLaunchError::Dalamud` 变体。端到端注入未在真机验证。（该分支早先还顺带修过 `find_7z` 的 Windows 探测，现已随纯 Rust 解压整体移除，见上）
- [x] **Windows 收尾（2026-09-12）**：设置页已按平台隐藏 Wine 配置项（Windows 原生启动忽略）；`game_files::version` 测试临时目录已加进程号避免 Windows Temp 残留串扰 (resolved: 2026-09-12)
- [x] **Injector 平台标记修正（2026-09-09）**：`runner.rs` 启动 Injector 时按平台设置 `XL_WINEONMAC`/`XL_WINEONLINUX`（此前无条件 `XL_WINEONLINUX`）
- [ ] **macOS Dalamud 注入验证**：~~xom wine（CrossOver + Rosetta 跑 x64）下 Injector 注入未做端到端验证~~ 已验证可用（2026-09-10，macOS 26 + xom-4.17.1，Dalamud 15.0.3.3 注入成功）；新建 prefix 后的首次注入曾因初始化竞态失败，已由 `EnsurePrefix` 引导命令修复

## P3 — 国际服 (SE) 补全（低优先级）

### P3-1 SE OAuth 完善 (`se.rs` vs `Launcher.cs`)

- [ ] **`Referer` 头缺失**：OAuth Top 和 Login.send 请求应带 `Referer` 头（从 frontier URL 模板生成）
- [ ] **Steam 登录支持**：完全缺失 — Steam ticket 获取、Blowfish ECB 加密、CrtRand、SE Base64 变形
- [ ] **`login()` 流程**：当前返回 `LoginResult` 但未自动调用 `register_session`
- [ ] **`make_computer_id` 编码问题**：Rust 用 UTF-8 算 SHA1，C# 用 `Encoding.Unicode`（UTF-16 LE），生成的 ID 完全不同

### P3-2 Steam 加密 (`Ticket.cs`, `CrtRand.cs`)

- [ ] **CrtRand 伪随机**：MSVC CRT `rand()` 的 Rust 实现（seed = time ^ ticket_sum）
- [ ] **Steam Auth Session Ticket 加密**：完整实现 `Ticket.EncryptAuthSessionTicket`
- [ ] **SE Base64 变形**：已实现 `to_mangled_se_base64()`，但上传时需按 300 字符分块用逗号连接

---

## P4 — 代码质量与健壮性

### P4-1 日志与调试

- [ ] **敏感字段脱敏**：ticket、tgt、sessionId、sndaId 等在日志中应做掩码处理（如 `abc***xyz`）
- [ ] **请求/响应日志**：`log::debug!` 级别记录 URL 和返回码，方便调试

### P4-2 测试

- [ ] **`crypto.rs` 单元测试**：`make_computer_id` 的 UTF-16 LE 编码修复后需对照 C# 输出验证
- [ ] **`SdoArea` 反序列化测试**：用真实 serverlist_new.js 内容做 fixture 测试
- [ ] **SE OAuth HTML 解析测试**：用 fixture HTML 验证 `_STORED_` 和 `login=auth,ok,...` 提取
- [x] **macOS 发行构建 runner**：Release workflow 固定使用受支持的 `macos-15` Apple Silicon runner，目标保持 `aarch64-apple-darwin`；macOS 构建失败不阻塞其他平台的策略不变。
- [x] **GUI/CLI 独立发行附件**：Windows 分别提供 App/CLI ZIP，Linux 分别提供 tar.gz，macOS 为 App DMG 与 CLI tar.gz；包内可执行文件名固定。GitHub/TOS 独立清单包含 GUI 各平台的版本、commit、构建号、大小与 SHA-256，Dev 文件名包含构建身份，不静默复用下载 URL。见 [`docs/releases.md`](docs/releases.md)。
- [x] **启动器构建身份与更新检查**：CI 注入版本、通道、commit、run/attempt 和分发来源；App 设置提供启动/手动检查、stable/dev、国内镜像/GitHub 回退与发布说明。请求有超时/大小限制，成功清单按来源缓存 6 小时；不下载发行包，不影响游戏操作。稳定版按 SemVer、Dev 按 run/attempt 比较，无法排序的源码构建不误报降级；底栏提示新版，`--version` 无窗口输出构建身份。
- [x] **Nix App/CLI 打包与仅检查策略**：flake 提供 `eorzea-app`、`eorzea-cli`、默认 App 包与运行入口，Linux App 包装运行库并安装桌面入口/图标；编译时 `nix-package` 和 Nix 来源标记、`/nix/store/` 路径兜底，仅显示版本检查与包管理器升级指引，不执行自安装。Wine/FHS 适配独立，使用说明见 [`docs/nix.md`](docs/nix.md)。
- [ ] **启动器自动下载安装**：普通 Windows/Linux 发行包仍需手动安装；需签名更新元数据、校验/暂存、退出后的替换、恢复路径与关键任务/未保存设置协调。macOS 应用包替换及签名需单独验收；Nix 包必须继续禁止下载安装启动器本体。
- [ ] **Nix macOS 原生验收**：需验证 App/CLI 原生构建、动态库、字体与启动；Linux 的无窗口检查不等同于 macOS 可运行或 Wine 可用。

### P4-2.5 GUI UX 修正

- [x] **正式 GUI 使用 Iced/tiny-skia 软件渲染**：`eorzea-app` 使用 Iced 0.14，仅启用 tiny-skia，不含 wgpu/glow/egui/Dioxus/Blitz 或 sysinfo 运行依赖。主页、账号/扫码/推送/密码、Wine/Dalamud/下载设置复用真实核心与配置格式；业务和 Iced executor 共用两个 Tokio worker，结果/最新进度/日志通过 Notify 订阅唤醒。release 启用 thin LTO 和符号裁剪。架构及无窗口验证见 [`docs/gui.md`](docs/gui.md)。
- [ ] **Iced 原生与跨平台验收**：用户禁止擅自在工作桌面开窗口，当前仅编译/内存 Iced/tiny-skia 布局绘制与状态回归；真实窗口、中文 IME 候选、剪贴板/portal、无障碍和 Windows/macOS 需在获准或真正隔离环境验证，真实登录/发行补丁/启动也不由内存测试代替。不将研究原型的 RAM/CPU 数据当作正式程序结果。

- [x] **运行日志复制**：全局日志抽屉提供显式「复制全部」按钮，复制当前保留日志（按时间从旧到新）；显示空日志、复制成功及剪贴板失败反馈。
- [x] **日志文字选择**：使用只读 Iced TextEditor，支持跨行拖选、Ctrl/Cmd+A、Ctrl/Cmd+C 复制所选，保留各记录及多行详情的级别色；拒绝输入、粘贴、剪切和编辑操作。选中/滚动阅读时暂停文本区跟随，底栏继续显示最新日志；「跟随最新」恢复当前日志并清除选区。只读适配关闭 IME 和光标闪烁重绘，文本选择/复制使用内存控件与模拟剪贴板回归，原生剪贴板仍待平台验收。
- [x] **启动按钮居中与悬停反馈**：「启动游戏」「启动中…」在 56px 按钮内显式水平和垂直居中；主/次/弱/危险按钮、分段按钮及导航补充 hover/pressed 状态。可用按钮悬停改变表面/边框/文字，禁用按钮不高亮、不触发按下动画；无窗口布局和实际像素回归防止再次偏移或样式无变化。
- [x] **全局底边栏与日志抽屉**：36px 底栏左侧显示编译时启动器版本，右侧显示最新真实日志，单行预览/全文提示及日志级别色；按钮展开全局可滚动、可复制日志抽屉，切页保留展开和滚动状态，主页不再重复放日志卡。操作结果独立显示 6 秒提示，新的日志和过期的旧提示不会覆盖它。
- [x] **日志抽屉动画裁剪**：位移/缩放后的子控件接收逆变换后的固定裁剪视口，嵌套文字、选区和按钮图层不能越过底栏。CPU 像素回归在控件创建后建立合成时钟基准，复用控件树及动画帧布局，覆盖收起、完全关闭、重开和中途反转，并在普通/150% 缩放下逐像素检查底栏及动画结束帧。
- [x] **日志滚动边缘叠字**：本地补丁修正 `iced_tiny_skia` 编辑器将视口边界误当作字形边界、跳过裁剪的问题，阻止半行文字溢出到上下留白并在局部重绘后残留；无窗口像素测试覆盖滚动、换行及局部重绘。
- [x] **Iced 轻量动效与层级精修**：使用内置 `Animation`，抽屉 200ms ease-out 位移、按钮 100ms 按下反馈；状态保存在控件内，仅过渡时请求绘制，不发送逐帧应用消息、不使主页面每帧重排，完成后停止请求。`EORZEA_REDUCED_MOTION=1` 关闭动效。摘要卡改用轻底色，保留白色/zinc、现有导航与业务布局；未引入动画/组件库或 GPU features。调查见 [`docs/gui-motion.md`](docs/gui-motion.md)，真实平台流畅度仍待获准测试。

- [x] **GUI/CLI 共用账号启动流程与票据生命周期**：两者统一调用 `Launcher::launch_account(LaunchRequest, on_event)`，核心库负责游戏预检 → Dalamud 准备 → 读取最新凭据 → 自动登录 → 保存轮换 key → 创建游戏进程。GUI 移除 `AppState.tokens`，每次启动及失败重试都重新换取 ticket；界面只提供参数并展示事件。回归测试覆盖同一 Launcher 连续重试、默认账号/名称/ID 选择、认证失败及预检失败不启动。
- [x] **GUI/CLI 自动登录凭据同步**：共享启动流程每次读取磁盘凭据，保存时重读并仅合并本账号的轮换 key，保留其他账号、默认账号与大区；启动认证使用文件锁防止两次启动同时轮换，发现凭据被其他操作修改时中止而不覆盖。GUI 修改账号元数据前也重新读盘。回归测试覆盖外部凭据轮换、认证期间元数据变更与 key 冲突。
- [ ] **账号配置写入原子性与统一互斥**：`auth::save` 直接覆盖文件，非启动的账号管理写操作尚未接入启动认证锁。需统一读改写互斥及原子替换，覆盖保存中断和账号管理与启动同时写入；当前启动流程的重读合并与 key 比较不能消除最终检查到写入之间的竞争窗口。
- [ ] **GUI 产品级主路径与恢复设计**：审查与建议见 [`docs/gui-ux-review.md`](docs/gui-ux-review.md)。首次配置需按缺失条件引导；主操作首屏可见；登录失效提供绑定原账号的重新登录入口；错误应有原位恢复动作，而非只写可被覆盖的全局状态字符串。
- [x] **GUI 更新任务生命周期与启动互斥**：根 `App` 持有操作状态、补丁列表和进度，Tokio任务独立于页面；更新/检查/启动互斥，账号/设置保存也阻止与关键操作并发。普通关闭请求在更新/启动/保存期间被拦截，可等待任务完成后退出；不提供直接 abort 真实补丁提交的演示取消按钮。强杀/断电继续依赖核心队列恢复。
- [x] **GUI 添加账号持久化失败的原位恢复**：错误显示在登录弹窗，保存失败不更新账号镜像、不关闭弹窗；保留登录结果供「重试保存账号」，无需重新扫码。密码不进入配置或日志。
- [x] **GUI 设置草稿保护与字段校验**：草稿归根状态，切页不丢失；退出前确认未保存草稿。最大/最少并发及毫秒延迟均校验，非法输入保持dirty且禁止保存；使用真实 Iced 输入/按钮/复选框而非绘制占位，完整平台键盘/可访问性另行验收。
- [ ] **GUI 状态准确性**：大区失败状态与重试、更新完成版本刷新、本次Dalamud覆盖摘要已接入；Wine 卡片明确标注「本地探测」，detect 与所选 startup_type 最终解析仍不完全等价，需无下载副作用的精确摘要接口。
- [ ] **GUI 账号组选择**：核心支持账号组与 poll/finalize，但 GUI 使用 wait_for_scan/wait_for_confirm 直接完成默认账号；多账号组应提供选择步骤，单账号不增加操作。

- [x] **空输入框默认值提示**：Iced TextInput 的 placeholder 只显示实际默认值，没有默认值的游戏目录、自定义 Wine 路径、账号和密码保持空白；提示不写入 value。Prefix、Dalamud 安装目录与延迟提示来自运行时默认路径和 DalamudSettings::default()。

- [x] **默认大区改为账号级（2026-11-02）**：`auth.toml` 的每个账号新增可选 `area`；GUI 主页「大区」下拉框直接读写当前账号，切换账号时自动套用；设置页移除全局「默认大区」输入（旧 `config.toml` 的 `area` 仅作回退保留，不再编辑）。解析顺序：`--area` → 账号 `area` → 旧 `config.toml.area` → 大区列表第一个（隐式选择并提示）。新账号继承添加时选中的大区，session key 刷新不会清掉已保存的 area。
- [x] **大区只暴露名称（2026-11-02）**：`auth.toml` / 旧 `config.toml` 的 `area` 现在保存并展示大区名称（对齐 XIVLauncherCN 的 `AreaName`），数字 `Areaid` 只在内部构造 `-AreaID` 和补丁地址时使用；兼容读取旧数字 ID。`eoz areas` 不再输出 ID 列，`--area` 接受名称。GUI 新建账号、切换账号、修改大区都按名称读写。
- [x] **`eoz launch` 不再提供登录参数（2026-11-02）**：移除 `--method` / `--username` / `--qr-file`，启动统一使用已保存 session key 自动登录；新账号或 key 过期时先用 `eoz auth login qr|password`。错误提示同步指向 `auth login`。
- [ ] **CLI 新账号未继承大区**：`eoz auth login` 保存新账号时 `area` 为空（GUI 会继承当前选中大区）；下次 `launch` 会走隐式回退。可在登录时记录本次解析的大区，或在 `auth status` 中展示并允许设置。
- [x] **账号管理卡片与添加弹窗**：账号以卡片展示，移除冗余的手动自动登录按钮，当前默认按钮禁用并显示「默认」；扫码/推送/密码收进添加弹窗，默认自动获取二维码、可刷新，密码表单提供明确标签。切换方式或关闭弹窗取消本地任务，登录成功后留在账号页；设默认/删除失败不更新内存为成功状态。
- [x] **添加账号方式标签**：三个等宽按钮显式水平/垂直居中，名称明确为「叨鱼扫码登录」「叨鱼推送登录」「密码登录」。

- [x] **下拉菜单交互**：Iced 下拉内联展开、当前项勾选、Escape/背景点击收起、切页收起和全局展开互斥，支持自定义 Dalamud 通道回显。
- [ ] **原生文字区域细线复验**：Blitz 截图的淡色横线根因未确定；更换框架不能视作实机现象已确认消失。待原生验收；调查保留于 [`docs/notes/gui-span-rendering.md`](docs/notes/gui-span-rendering.md)。

- [x] **图标侧边栏**：导航宽 72px，游戏/账号使用 SVG 图标，设置固定底部；提供悬停提示与当前页高亮，不提供亮/暗主题切换入口。
- [ ] **Iced 无障碍与完整键盘导航**：Tab/Shift-Tab 控件焦点遍历与 Escape 已接入，secure input 合成事件通过无窗口测试；图标导航/分段按钮的键盘激活、模态焦点限制和系统读屏语义尚需平台验证，不能等同于完整无障碍支持。

- [x] **首次启动按缺失条件进入页面**：无账号进入账号页，已有账号但无游戏目录进入设置页，否则进入主页。
- [x] **启动流程不阻塞且跨页面保持**：根 App 的 Tokio runtime 执行真实共享启动流程，操作状态不属于页面；Wine/prefix/Injector/DLL/解压/校验等阻塞任务继续由核心移出async worker。GUI通过事件和合并的最新进度回主线程，不跨线程读写UI状态；切页不取消任务。核心mock启动回归与GUI内存状态回归分别覆盖业务和前台生命周期。
- [ ] **归档盘符校验**：Linux 上 `dalamud::archive::tests::test_safe_relative_path_rejects_escape` 失败，`safe_relative_path` 未拒绝 `C:\Windows\evil.dll`；需跨平台显式识别 Windows 盘符，不能只依赖宿主 `Path` 的组件解析。
- [x] **GUI 验证边界**：Iced 前端为原生应用，不提供 Dioxus/wasm 浏览器调试入口；核心库独立的 wasm 平台门控保留。测试用隔离根状态、内存控件和 CPU 绘图，不调用真实认证、补丁安装或游戏启动。
- [x] **全局运行日志**：`logging.rs` 的 tracing Layer + 1000行缓冲通过通知唤醒 Iced，增量汇入根 App（最多显示300行）；底栏预览最新一条，展开后最新在上并按级别着色；默认只开本项目 INFO，`RUST_LOG` 可覆盖。复制保留时间从旧到新的原始顺序。
- [x] **密码安全与重复提交**：密码登录成功后清空密码；登录/启动/检查更新增加 busy 防重入
- [x] **删除账号二次确认**：避免误触立即删除本地账号配置
- [x] **更新进度可读**：补丁下载进度显示 MiB/GiB 而非裸字节
- [x] **主页 Dalamud 状态卡**：改为异步获取 release 元数据 + 安装/版本/runtime 状态，不再在 render 中同步读盘并显示误导性的“启动时自动校验/更新”
- [x] **设置页补全**：新增 Dalamud section（enabled/load_method/track/delay/safe mode）与游戏路径即时校验、msync 开关；主页新增“本次启动加载 Dalamud”会话级开关
- [x] **原生 GUI 入口与调查解耦**：正式入口为 Iced/tiny-skia，Dioxus span 对照源码独立保存在文档 fixture 供调查参考，不作为 Cargo example 编译；核心 wasm 平台门控独立保留。
- [x] **Wine UI 按平台隐藏**：Windows 原生启动不经 wine，设置页 Wine 区块与主页 Wine 状态卡整体隐藏（此前“未检测到可用 wine”像故障）；macOS 隐藏 Linux 专属的 esync/fsync/gamemode 开关（msync/DXVK 保留）；隐藏开关的草稿值保存时原样写回 (resolved: 2026-09-12)
- [x] **blitz 下拉弹层层级**：改为不依赖 `position:absolute + z-index` 的文档流内联展开（把下方内容顶开），原生 blitz 与 web 渲染一致，不再被后续卡片遮挡 (resolved: 2026-09-12)
- [x] **下拉框交互缺陷**：展开状态收进全局 `AppState.open_dropdown`（同时只允许一个展开，切 Tab 自动收起）；点击弹层外区域收起走事件冒泡——根容器 onclick 收起、下拉容器 `stop_propagation` 阻断内部点击（全屏透明捕获层方案在 blitz 上会连弹层选项点击一并吞掉，已弃用） (resolved: 2026-09-12)
- [x] **blitz 字形缺字**：下拉箭头 `▾` 改 ASCII `v`/`^`（随展开状态切换），主题切换 `☾/☀` 改纯文字「暗色主题/亮色主题」，原生 blitz 不再渲染为方块 (resolved: 2026-09-12)
- [x] **设置页长表单**：滚动改为页面自持——主页/登录页各自包滚动容器，设置页拆为「表单滚动区 + 固定保存栏」，保存按钮在任何滚动位置都可见可点（此前的 sticky 方案在 blitz 上不生效，已弃用） (resolved: 2026-09-12)
- [x] **全角括号紧邻西文时渲染为方块**：根因是 blitz 无逐字符字体回退，默认拉丁字体缺全角标点字形；根容器显式指定中文字体栈（Microsoft YaHei / PingFang SC / Noto Sans CJK SC，覆盖三平台，web 端同样生效），「（插件）」「（release）」等混排均正常渲染 (resolved: 2026-09-12)
- [x] **设置页交互形态**：更新通道从自由文本改为下拉框（稳定版 release / 测试版 staging，回显当前值，配置为自定义通道时自动补进选项）；另有默认值的输入项（安装目录/初始化延迟/默认大区）以常显说明文字标注默认与留空语义——blitz 未实现 placeholder 渲染，原生端不能依赖 placeholder 表达默认值 (resolved: 2026-09-12)
- [x] **底栏长日志与操作结果**：底栏日志单行省略、悬停看全文；操作结果使用独立可关闭的短提示，更新失败和登录/设置错误同时保留原位显示，不让长 URL/路径撑破窗口。产品级恢复提示仍见上方待办。
- [x] **设置页开关在原生 blitz 下保存无效**：blitz 对 checkbox 点击只派发 input 事件（无 change），`onchange` 桌面端永不触发——勾选只是控件自身视觉切换、草稿 signal 不更新，保存实际写入旧值（实测启用 Dalamud 后 config.toml 仍为 `enabled = false`）；`Checkbox` 改用 `oninput`（web 端 checkbox 点击同样触发 input；dioxus-web 与 blitz 均把 value 归一为 "true"/"false"，`FormData::checked()` 两端可用） (resolved: 2026-09-12)
- [x] **设置页补全剩余配置项**：新增「默认大区」（`AppConfig.area`，此前设置页无此输入框、保存时只能原样保留；GUI 启动拉取大区列表后默认选中该大区，CLI `launch` 未传 `--area` 时也用它）与 Dalamud「安装目录」（`install_root`，留空用默认 `~/.eorzea/dalamud`） (resolved: 2026-09-12)
- [x] **设置页 Dalamud 项无说明**：加载方式/更新通道/安装目录/初始化延迟/插件禁用开关逐项补充说明文字（按注入器 `--mode`/`--without-dalamud`/`--no-plugin`/`--no-3rd-plugin`/`--dalamud-delay-initialize` 实际行为撰写）；「默认大区」下方动态列出可用大区 ID 对照（如 8=豆豆柴 · 1=陆行鸟） (resolved: 2026-09-12)
- [x] **首次运行创建配置模板**：`config.toml` 不存在时，`load_app_default`（GUI 与 `eoz` 共用）创建只含注释的模板，避免用户找不到配置文件位置；不写默认值，防止以后改默认值时被旧文件钉死；模板按平台区分（Windows 不列 Wine/DXVK/环境变量） (2026-10-30)

### P4-3 错误处理

- [x] **`SdoLoginData` 补全 `AccountArray` / `SndaIdArray`**：扫码成功后返回的账号列表（字段早已存在，现由 `get_account_group` 实际消费）
- [ ] **DC 跨服传送 API** (`DcTraveler.cs`)：`ff14bjz.sdo.com` 下的旅行/回归 API，暂不在 P0 范围内

### P4-4 Windows 平台

- [x] **编译与单元测试**：Windows（MSVC）上 `cargo check --workspace --all-targets` 通过、`cargo test --workspace` 全绿（eorzea 核心库 / eorzea-app / eorzea-auth / eoz 全部通过，2026-10-30 复核）
- [x] **Wine 模块按平台裁剪**：`src/wine.rs` 在 Windows 上不参与编译（`lib.rs` 的 `pub mod wine` 加 `cfg(not(windows))`）；Injector 启动/通信错误改用平台无关的 `dalamud::runner::InjectorError`（`WineError` 留在 wine 模块，原生 Windows 路径不再借用它）；`runner` 的 winepath/Injector-over-Wine 函数、`home` 的 Wine 状态卡、`wine_test` 示例均在 Windows 上不编译；`eoz launch --wine` 在 Windows 上不存在
- [x] **原生启动分支**：`game.rs` 的 `cfg(target_os = "windows")` 分支直启 `ffxiv_dx11.exe`，或原生运行 `Dalamud.Injector.exe`（无 winepath / `XL_WINEON*` / `WINEPREFIX`）；Wine 设置区块与主页 Wine 状态卡在 UI 隐藏
- [ ] **Iced GUI Windows 构建与启动冒烟**：需验证 tiny-skia/softbuffer、中文字体、窗口行为；不能沿用其他 GUI 框架的构建/运行结论。
- [ ] **真机端到端验证**：Windows 上实际登录 → 启动游戏 → Dalamud 注入尚未验证（与 C# `WindowsDalamudRunner` 的行为差异需实机确认）
- [ ] **Windows 设备指纹**：见 P0-1（当前 fallback 为 `unknown-<USERNAME>`）
- [ ] **发布版日志**：`windows_subsystem = "windows"` 后 GUI 无控制台，`tracing` 只写 stderr → release 版没有日志可查；需要文件日志（如滚动写到 `~/.eorzea/logs/`）
- [ ] **配置目录**：当前 `~/.eorzea`（`C:\Users\<用户>\.eorzea`），与 C# 的 `%APPDATA%\XIVLauncher` 不同，也不读取 C# 旧配置；是否改用 `dirs::config_dir()` 待定
- [ ] **安装器与代码签名**：`v*` tag 发布版本包，push `main` 或从 `main` 手动发布更新 Dev；PR 只构建不发布。Windows 目前为独立 App/CLI ZIP，仍无 MSI/NSIS 与代码签名，用户需解压后运行；macOS DMG 为实验性、未签名/公证且未真机验证。见 [`docs/releases.md`](docs/releases.md)。
- [x] **配置按平台拆分**：Windows 的 `AppConfig` 不含 Wine 字段（`#[cfg(not(windows))]`），`config.toml` 不写 `startup_type`/`custom_path`/`prefix`/`esync`/`fsync`/`msync`/`gamemode`/`[dxvk]`/`[env]`，`eoz config set` 对 Wine 键报「未知配置键」；读取 Linux/macOS 写出的配置时忽略这些键，`eoz config list` 只输出当前平台字段；首次运行模板也按平台区分（2026-10-30）

---

## 已完成

- [x] SDO 密码登录 (`static_login`)
- [x] SDO 推送登录 — 请求 (`slide_login_request`)
- [x] SDO 推送登录 — 轮询 (`slide_login_poll`)
- [x] SDO 扫码登录 — 获取二维码 (`qr_code_request`)
- [x] SDO 扫码登录 — 轮询 (`qr_code_poll`)
- [x] SDO 自动登录 (`auto_login`)
- [x] SDO SSO — TGT 换 ticket (`sso_login`)
- [x] SDO 激活权限 (`get_promotion_info`)
- [x] SDO 服务器列表获取 (`fetch_server_list`) — 已通过真实 API 验证
- [x] SE OAuth Top — 获取 `_STORED_` token
- [x] SE OAuth Login — 提交用户名/密码/OTP
- [x] SE Register Session — 版本检查与补丁检测
- [x] Feature gate (`sdo` default, `se` optional)
- [x] 补全 `se_login` example（此前 Cargo.toml 已声明但文件缺失，导致 `cargo fmt --all` 报错）
- [x] Cargo workspace 重构（根 crate = tauri，`packages/` 下子 crate）
- [x] **crate 与可执行文件命名**：根包与核心库均为 `eorzea`（无 bin），GUI 包与可执行文件均为 `eorzea-app`，CLI 包为 `eorzea-cli`、可执行文件为 `eoz`；代码引用、日志过滤与发布打包使用对应名称。默认成员为 `eorzea-cli`，GUI 用 `cargo run -p eorzea-app`。
- [x] Rust doc 注释覆盖所有公开 API
- [x] **`SdoLoginData.SndaId` 字段大小写修复**：`model.rs` 中 `snda_id` 和 `snda_id_array` 添加 `alias` 同时支持 `SndaId`/`sndaId` 两种大小写（C# 参考使用小写 `sndaId`）

---

## 备注

- 以上条目按优先级分组：P0 为核心登录链路必须打通的，P1 为扫码/推送完善，P2 为游戏启动所需，P3 为国际服（低优先级），P4 为质量提升。
- 每次修复后应在对应条目旁标注完成日期和 commit hash，保持 TODO.md 与实际代码同步。
- C# 参考文件路径均相对于 `XIVLauncher.Core/lib/FFXIVQuickLauncher/src/XIVLauncher.Common/Game/`。
