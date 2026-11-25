{
  description = "Eorzea FFXIV launcher and CLI";

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
      self,
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

        packageCraneLib = (crane.mkLib pkgs).overrideToolchain (p: p.rust-bin.stable.latest.default);
        version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
        revision = self.rev or (self.dirtyRev or "unknown");
        packageArgs = {
          src = lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (packageCraneLib.filterCargoSources path type)
              || builtins.elem (baseNameOf path) [ "eorzea.png" "eorzea.ico" "resources.rc" ];
          };
          inherit version;
          pname = "eorzea";
          strictDeps = true;
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [
            pkgs.openssl
          ]
          ++ lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.fontconfig ]
          ++ lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.apple-sdk ];
          # Limit memory use even on hosts exposing many cores.
          CARGO_BUILD_JOBS = "2";
        };
        mkPackage =
          package: binary:
          let
            args = packageArgs // {
              cargoExtraArgs =
                "-p ${package}" + lib.optionalString (package == "eorzea-app") " --features nix-package";
            };
          in
          packageCraneLib.buildPackage (
            args
            // {
              pname = package;
              # A CLI-only build must not compile Iced or the GUI dependency chain.
              cargoArtifacts = packageCraneLib.buildDepsOnly args;
              doCheck = false; # Tests are run separately, without a display or user credentials.
              EORZEA_DISTRIBUTION = "nix";
              EORZEA_BUILD_VERSION = "${version}-nix.${builtins.substring 0 12 revision}";
              EORZEA_BUILD_COMMIT = if self ? rev then self.rev else "";
              # Nix/source revisions do not have a GitHub Actions run number; never invent one.
              EORZEA_BUILD_CHANNEL = "dev";
              nativeBuildInputs = packageArgs.nativeBuildInputs ++ [ pkgs.makeWrapper ];
              postInstall = lib.optionalString (package == "eorzea-app") (
                lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
                  wrapProgram "$out/bin/eorzea-app" \
                    --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath guiSystemLibs}" \
                    --prefix PATH : "${lib.makeBinPath [ pkgs.xdg-utils ]}"
                  install -Dm644 ${./assets/eorzea.desktop} "$out/share/applications/eorzea.desktop"
                  install -Dm644 ${./assets/eorzea.png} "$out/share/icons/hicolor/512x512/apps/eorzea.png"
                ''
              );
              meta = {
                description = if package == "eorzea-app" then "FFXIV SDO launcher" else "FFXIV SDO launcher CLI";
                homepage = "https://github.com/icexiv/eorzea";
                mainProgram = binary;
                platforms = lib.platforms.linux ++ lib.platforms.darwin;
              };
            }
          );
        app = mkPackage "eorzea-app" "eorzea-app";
        cli = mkPackage "eorzea-cli" "eoz";
      in
      {
        packages.notist = notist.packages.${system}.default;
        packages.eorzea-app = app;
        packages.eorzea-cli = cli;
        packages.default = app;
        apps.default = {
          type = "app";
          program = "${app}/bin/eorzea-app";
        };
        apps.eorzea-app = {
          type = "app";
          program = "${app}/bin/eorzea-app";
        };
        apps.eorzea-cli = {
          type = "app";
          program = "${cli}/bin/eoz";
        };
        devShells.default = craneLib.devShell {
          packages = with pkgs; [
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
