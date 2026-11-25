<p align="center">
  <img src="assets/eorzea.png" width="160" height="160" alt="Eorzea：水晶塔与 IceXIV 冰方块" />
</p>

# eorzea

使用 Rust 编写的《最终幻想 XIV》国服（盛趣）第三方启动器，提供图形界面和命令行工具。

## 项目标识

Logo 以游戏内水晶塔的主塔、晶体切面与入口为主体，左上后方露出部分 IceXIV 冰方块，使用透明背景。
原图位于 `assets/logo.png`；`assets/eorzea.png` 为 512px 展示版，供 README、窗口图标与 Linux 桌面菜单使用。
Windows 可执行文件嵌入 `assets/eorzea.ico`，macOS App 包使用 `assets/eorzea.icns`；IceXIV 主页与本仓库共用同一份 512px PNG。

## 功能

- 叨鱼扫码、密码和自动登录，支持多账号管理
- 游戏更新、补丁下载与文件完整性校验
- Windows 原生启动，Linux / macOS 通过 Wine 启动
- Dalamud 插件框架集成
- 图形界面与命令行工具共享账号和配置
- 启动器版本检查，支持稳定版/开发版和国内镜像；Nix 包仅检查、不下载安装

## 下载

前往 [Releases](https://github.com/icexiv/eorzea/releases)，按程序和平台选择独立附件：

- **`eorzea-app`**：图形界面（GUI），普通用户通常选择这个。
- **`eoz`**：命令行工具（CLI），在终端中使用，不需要同时下载 GUI。

Windows 提供独立 App / CLI ZIP，Linux 提供独立 tar.gz，解压后运行其中的 `eorzea-app` 或 `eoz`。macOS App 使用 DMG，CLI 使用 tar.gz。旧开发构建附件可能仍为裸二进制，以实际文件名为准。

提供 Linux x86_64、Windows x86_64 和 macOS arm64 构建。macOS 尚未经过真机验证。

最新开发构建见 [Development build](https://github.com/icexiv/eorzea/releases/tag/dev-build)，可能不稳定。

Nix 用户可构建本项目 flake 的 `eorzea-app`（默认）或 `eorzea-cli` 包，见 [Nix 安装与更新](docs/nix.md)。GUI 的「设置 → 启动器更新」可手动检查，也可在启动时后台检查；当前阶段所有分发均需手动安装，Nix 包只提供包管理器升级指引。

## 快速开始

### 图形界面

运行 `eorzea-app`，在设置中选择游戏目录，添加账号并登录，然后启动游戏。

GUI 使用 **Iced + tiny-skia 软件渲染**，不使用 WebView 或 GPU 渲染后端。Linux 需要 Wayland/X11
运行库；GUI 本身不需要 Vulkan（通过 Wine/DXVK 运行游戏的要求独立）。中文字体从系统查找；
缺少时安装 Noto Sans CJK，或通过 `EORZEA_FONT=/path/to/font.ttf`
指定字体文件。账号和配置格式与 CLI 保持兼容。

### 命令行

将 `eoz` 加入 `PATH`，或在下列命令中使用它的完整路径：

```bash
# 设置游戏根目录（包含 boot、game、sdo 文件夹）
eoz config set game_path /games/ffxiv

# 使用叨鱼 App 扫码登录
eoz auth login qr

# 更新并启动游戏
eoz game update
eoz launch
```

运行 `eoz --help` 查看全部命令。账号与配置保存在 `~/.eorzea/`。

Linux / macOS 通过 Wine 运行游戏，配置与排错请参阅 [Wine 使用说明](docs/wine.md)。Apple Silicon Mac 还需要安装 Rosetta 2。

## 从源码运行

安装 Rust 工具链后，在仓库目录中执行：

```bash
# 图形界面
cargo run -p eorzea-app

# 命令行
cargo run -p eorzea-cli -- --help
```

仅编译/无窗口验证：

```bash
cargo check --locked -p eorzea-app -p eorzea-auth -p eorzea-cli -j2
cargo test --locked -p eorzea-app -j2 -- --test-threads=2
```

GUI 测试使用内存 Iced 控件及 tiny-skia，不打开桌面窗口，不读取真实账号或执行登录/更新/启动。
开发架构及验证边界见 [GUI 文档](docs/gui.md)。

发行构建使用 `cargo build --locked --release -p eorzea-app -j2`，启用 thin LTO 和符号裁剪。
debug 文件含调试信息，不能当作发行体积；可执行文件大小也不等于运行内存。

## 文档

- [命令行使用](docs/cli.md)
- [配置说明](docs/config.md)
- [Wine 与 DXVK](docs/wine.md)
- [Dalamud](docs/dalamud.md)
- [NixOS](docs/notes/nixos.md)
- [Nix 安装与更新](docs/nix.md)
- [已知问题与待办](TODO.md)

## 致谢

认证与游戏启动逻辑参考了 [XIVLauncher.Core](https://github.com/goatcorp/XIVLauncher.Core)。本项目与 SQUARE ENIX、盛趣游戏无隶属关系。
