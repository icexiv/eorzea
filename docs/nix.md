# Nix 打包与更新

## 包与入口

`flake.nix` 提供：

- `packages.eorzea-app`：图形界面；`packages.default` 指向此包。
- `packages.eorzea-cli`：命令行工具 `eoz`，不编译或安装 GUI。
- `apps.default`、`apps.eorzea-app`、`apps.eorzea-cli`：对应的 `nix run` 入口。
- `devShells.default`：开发环境，不等同于安装包。

在仓库中构建（不启动 GUI）：

```sh
nix build .#eorzea-app
nix build .#eorzea-cli
```

Linux App 包包含桌面入口、图标以及 Wayland/X11、fontconfig 等动态库包装，不要求用户先进入 devShell。系统中文字体仍需用户安装。`eorzea-app --version` 只打印构建版本和分发来源，不创建窗口。

包使用 flake.lock 固定的 Rust 工具链和 Cargo.lock；构建阶段不下载游戏、Wine 或 Dalamud，不读取用户账号。macOS 包定义仍需原生构建与运行验证。

## NixOS / Home Manager

在系统或用户配置的 flake 中引用本项目：

```nix
{
  inputs.eorzea.url = "github:icexiv/eorzea/main";
  outputs = { self, nixpkgs, eorzea, ... }: {
    # 在已有 NixOS / Home Manager 配置中使用：
    # environment.systemPackages = [ eorzea.packages.${pkgs.system}.eorzea-app ];
    # home.packages = [ eorzea.packages.${pkgs.system}.eorzea-cli ];
  };
}
```

App 与 CLI 可独立安装，也可同时安装；配置共享 `~/.eorzea/`。

## 启动器更新策略

Nix 包在编译时启用 `nix-package`，并注入 Nix 分发标记；可执行文件位于 `/nix/store/` 时也按 Nix 包处理，不依据是否使用 NixOS 判断。

设置页「启动器更新」提供启动检查、手动检查、stable/dev 通道、国内镜像/GitHub 来源及发布说明。**只请求版本清单，不下载发行归档、不写入或替换启动器，也不执行 Nix 升级命令。**没有通过配置或运行时环境变量启用自安装的开关。此策略不限制游戏补丁或 Dalamud 的更新。

- **NixOS / Home Manager**：在管理配置的仓库更新本项目对应的 flake input，再按原来的流程 rebuild / switch；input 名以自己的配置为准。
- **Nix profile**：检查 profile 中该项目的条目，使用 Nix 对应版本的 profile upgrade 流程升级。
- **本地源码**：更新源码后重新构建。

上游发布与 Nix 包源更新是两回事：固定旧 ref、未更新 flake.lock 或第三方包源尚未收录时，不会获得新版。Nix/source 构建没有 GitHub Actions 构建序号；仅凭不同 commit 无法判断先后，界面会如实显示「与当前构建不同，无法判断先后」，而不是推荐覆盖或降级。

## Wine 与游戏运行

启动器原生打包不等于托管 Wine 的 FHS 兼容问题已经解决。NixOS 优先使用 nixpkgs 的 Wine，并在启动器中选择「系统」或明确指定自定义 Wine；托管预编译 Wine 仍可能需要 nix-ld 或 FHS 包装。详见 [NixOS 与 Wine 说明](notes/nixos.md)。

游戏文件、Wine prefix 与 Dalamud 安装在用户可写目录，不放进 Nix store。
