{
  description = "rust";

  # nixConfig = {
  #   extra-substituters = [
  #     "https://mirrors.ustc.edu.cn/nix-channels/store"
  #   ];
  #   trusted-substituters = [
  #     "https://mirrors.ustc.edu.cn/nix-channels/store"
  #   ];
  # };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    notist = {
      url = "github:AzurIce/notist";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
      inputs.crane.follows = "crane";
      # 保留 Notist 自己的 rust-overlay：项目的旧锁中没有它固定的 nightly。
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      crane,
      notist,
      rust-overlay,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        inherit (pkgs) lib;
        craneLib = (crane.mkLib pkgs).overrideToolchain (
          p:
          p.rust-bin.nightly.latest.default.override {
            targets = [ "wasm32-unknown-unknown" ];
            extensions = [ "rust-src" ];
          }
        );

        # 运行时系统库（仅 Linux；macOS 上 winit 走 AppKit、rfd 走系统框架、
        # wine 用 xom/CrossOver 构建自带 dylib，均不需要 nix 提供的库）。
        # Iced/tiny-skia (winit + softbuffer)：客户端和键盘库需供 dlopen 查找。
        guiSystemLibs = with pkgs; [
          openssl # auth crate's reqwest uses default TLS (openssl-sys)
          wayland # winit wayland backend
          libxkbcommon # 键盘输入
          fontconfig # 系统中文字体发现
          libx11 # winit X11 backend
          libxi
          libxcursor
          libxrandr
          libunwind # wine (ubuntu build) dlopens libunwind.so.8 at runtime
          dbus # rfd 的 xdg-portal 后端运行时 dlopen libdbus-1.so
        ];
        # Wine (ubuntu build) 运行所需的系统库：dlopen 加载，不在 ldd 里
        wineLibs = with pkgs; [
          freetype # TrueType 字体渲染
          fontconfig
          gnutls # 加密/pfx
          vulkan-loader # DXVK
          mesa # libGL
          libx11
          libxext
          libxrender
          libxrandr
          libxi
          libxcursor
          libxinerama
          libxxf86vm
          libxcb
          libpulseaudio # wine 声音（pulse/pipewire）
          alsa-lib # wine 声音（ALSA）
        ];
        runtimeLibs = lib.optionals pkgs.stdenv.hostPlatform.isLinux (guiSystemLibs ++ wineLibs);
      in
      {
        packages.notist = notist.packages.${system}.default;
        devShells.default = craneLib.devShell {
          packages =
            with pkgs; [
              notist.packages.${system}.default
              gh
              pkg-config
            ];
          buildInputs =
            runtimeLibs
            ++ lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              pkgs.apple-sdk # darwin 系统框架
            ];
          shellHook = lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
            # Make system libs discoverable at runtime (winit dlopens
            # libwayland-client/libxkbcommon; wine dlopens libunwind etc.)
            export LD_LIBRARY_PATH="${lib.makeLibraryPath runtimeLibs}:$LD_LIBRARY_PATH"
          '';
        };
      }
    );
}
