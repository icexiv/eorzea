# Agent Guidelines

## Project Overview

`eorzea` is a Rust reimplementation of the FFXIV launcher, porting auth and game-launch logic from `XIVLauncher.Core` (C#). The project uses an Iced + tiny-skia Rust workspace:

```
eorzea/                     # workspace root（核心库 crate `eorzea`，不含 GUI）
├── src/                    # auth / config / launcher / game / wine / dalamud / game_files
├── packages/
│   ├── eorzea-app/         # GUI（bin `eorzea-app`，Iced + tiny-skia 软件渲染）
│   │   └── src/ui/         # GUI 页面（mod.rs 根组件/共享状态，login/home/settings）
│   ├── eorzea-auth/        # auth library (feature-gated)
│   └── eorzea-cli/         # CLI（bin `eoz`：游戏文件管理/账号/启动）
└── examples/               # sdo_launch / wine_test / hash_bench（核心库示例）
```

工作区默认成员是 `eorzea-cli`：`cargo run` / `cargo check` 只编译 CLI 链路，不碰 GUI；
GUI 用 `cargo run -p eorzea-app`，Iced 关闭默认 features，仅用 tiny-skia；不编译 wgpu、glow 或 sysinfo。

GUI 相关：`packages/eorzea-app/src/ui/mod.rs` 的 `App` 根状态持有账号/配置镜像、跨页设置草稿、
登录任务 generation、全局更新/启动互斥、进度和日志。`ui/backend.rs` 与 Iced executor 共享两个 worker 的 Tokio runtime，
执行登录/配置读写/更新/启动，以事件及合并的最新进度回到根状态，通过 `Notify` 订阅唤醒；空闲不轮询。
`ui/login.rs` 为真实扫码/推送/密码登录和账号持久化；`ui/home.rs` 复用 `GameFileManager` 和
`Launcher::launch_account`，不缓存游戏 ticket；`ui/settings.rs` 保存现有 config.toml 格式。
阻塞初始化和配置写入放 `spawn_blocking`，任务不随切页丢失。
`ui/footer.rs` 提供跨页版本/最新日志底栏与日志抽屉；操作结果用独立短提示。
`ui/motion.rs` 的 Iced Animation 状态在控件内，仅过渡期间请求绘制，不使用逐帧应用消息或常驻计时器；
`EORZEA_REDUCED_MOTION=1` 可关闭动效。

**桌面测试限制**：用户禁止擅自在其工作桌面打开 GUI、截图/portal、切工作区或抢焦点。
默认仅运行编译、纯状态测试和 Iced/tiny-skia 内存布局/绘制/控件事件测试，不启动 `cargo run`。
真实窗口/IME/平台测量须先取得用户明确许可或确认真正隔离的 compositor/headless 环境。

**Current focus: SDO (中国服/盛趣) authentication and game launch.** The `se` (international) feature is implemented but not the priority.

## Key Directives

1. **Keep TODO.md in sync** — `TODO.md` at the repository root tracks all known gaps between this Rust implementation and the upstream C# reference. Whenever you:
   - implement a missing feature,
   - fix a behavioural discrepancy, or
   - discover a *new* difference not yet documented,
   **you MUST update `TODO.md`** to reflect the current state (check off completed items or add new ones).

2. **Minimal changes** — Make the smallest change possible to achieve the goal. Do not restructure or refactor unrelated code.

3. **Follow existing style** — Match the formatting, naming, and error-handling patterns already present in the crate you are editing.

4. **Test what you change** — Run `cargo check -p eorzea-auth` (with appropriate features) after any change. If you add new functionality, add a test or verify it with an example binary.

## Feature Gates

`eorzea-auth` uses Cargo feature gates to separate server-region logic:

| Feature | Default | Description |
|---------|---------|-------------|
| `sdo` | Yes | 中国服 (SDO/盛趣) 登录：密码、推送、扫码、自动登录、SSO |
| `se` | No | 国际服 (Square Enix) OAuth 登录 |

```toml
# Default build — SDO only
cargo check -p eorzea-auth

# SE only
cargo check -p eorzea-auth --no-default-features --features se

# Both
cargo check -p eorzea-auth --features se
```

Example binaries are also feature-gated:
```bash
cargo run -p eorzea-auth --example sdo_login          # requires feature "sdo" (default)
cargo run -p eorzea-auth --example se_login            # requires feature "se"
```

## Architecture Notes

### `packages/eorzea-auth/`

| File | Feature | Purpose |
|------|---------|---------|
| `lib.rs` | — | Crate root, re-exports, feature-gated module includes |
| `model.rs` | — | Shared data structures (`LoginResult`, `OauthLoginResult`, `SdoArea`, `SdoLoginData`, etc.) |
| `error.rs` | — | `AuthError` enum covering both SDO and SE errors |
| `crypto.rs` | — | Shared crypto utilities (SHA1, MD5, computer ID, SE Base64 mangling) |
| `sdo.rs` | `sdo` | SDO auth client: `static_login`, `slide_login_*`, `qr_code_*`, `auto_login`, `sso_login`, `fetch_server_list` |
| `se.rs` | `se` | SE OAuth client: `login` (top + login.send), `register_session` |

### Reference Implementation (C#)

When in doubt about protocol details, check the corresponding files in `XIVLauncher.Core`:

> **参考仓库默认位置**：`~/Files/repos/XIVLauncher.Core`（主仓库，`cn` 分支）。
> 实际认证/启动代码在其 submodule `lib/FFXIVQuickLauncher/`（CN 分支）下，
> 例如 `SdoLauncher.cs` 位于
> `~/Files/repos/XIVLauncher.Core/lib/FFXIVQuickLauncher/src/XIVLauncher.Common/Game/SdoLauncher.cs`。
> 文档中凡提及 `XIVLauncher.Core/...` 均指该默认位置。

| Rust module | C# reference |
|-------------|--------------|
| `sdo.rs` | `SdoLauncher.cs`, `SdoUtils.cs`, `SdoArea.cs` |
| `se.rs` | `Launcher.cs` (OAuth section) |
| `crypto.rs` | `Ticket.cs`, `ArgumentBuilder.cs`, `SdoUtils.cs` |
| `model.rs` | `SdoArea.cs`, `SdoLoginResult.cs`, various model classes |
