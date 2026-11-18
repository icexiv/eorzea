use super::{App, Msg, theme::*};
use eorzea::{
    config::{SdoLaunchMethod, SyncMode, WineStartupType},
    dalamud::model::DalamudLoadMethod,
};
use iced::widget::{column, container, row};
use iced::{Element, Fill};

impl App {
    pub(super) fn settings_page(&self) -> Element<'_, Msg> {
        let d = &self.draft;
        let enabled = !self.settings_busy && !self.modal_open();
        let mut body = column![
            self.launcher_update_section(),
            self.launch_section(enabled),
            section(
                "补丁下载",
                column![
                    field("最大并发", input("max", &d.max, "4", enabled)),
                    field("最少并发", input("min", &d.min, "0", enabled))
                ]
                .spacing(8)
            )
        ]
        .spacing(12);
        #[cfg(not(target_os = "windows"))]
        {
            let wine = match d.wine.startup_type {
                WineStartupType::Auto => 0,
                WineStartupType::Managed => 1,
                WineStartupType::Custom => 2,
                WineStartupType::System => 3,
            };
            let mut content = column![
                text("Linux / macOS").size(12).color(MUTED),
                field(
                    "启动方式",
                    segments("wine", &["自动", "托管", "自定义", "系统"], wine, enabled)
                )
            ]
            .spacing(12);
            if d.wine.startup_type == WineStartupType::Custom {
                content = content
                    .push(field("自定义路径", input("custom", &d.custom, "", enabled)))
                    .push(
                        text("指定 wine64 / wine 文件或包含它们的目录（也搜索 bin/）。")
                            .size(12)
                            .color(MUTED),
                    );
            }
            content = content.push(field(
                "Prefix",
                input(
                    "prefix",
                    &d.prefix,
                    &eorzea::wine::WineTool::default_prefix_path()
                        .display()
                        .to_string(),
                    enabled,
                ),
            ));
            #[cfg(target_os = "linux")]
            {
                let sync = match d.wine.sync.mode() {
                    SyncMode::None => 0,
                    SyncMode::Esync => 1,
                    SyncMode::Fsync => 2,
                };
                content = content
                    .push(field(
                        "同步方式",
                        segments("sync", &["关", "esync", "fsync"], sync, enabled),
                    ))
                    .push(check(
                        "gamemode · 接入 Feral GameMode",
                        d.wine.gamemode,
                        "gamemode",
                        enabled,
                    ));
            }
            #[cfg(target_os = "macos")]
            {
                content = content.push(check(
                    "msync · macOS Wine 同步",
                    d.wine.msync,
                    "msync",
                    enabled,
                ));
            }
            content = content.push(check(
                "DXVK · D3D9/10/11 转 Vulkan",
                d.wine.dxvk.enabled,
                "dxvk",
                enabled,
            ));
            body = body.push(section("Wine", content));
        }
        if let Err(error) = d.apply(&self.config) {
            body = body.push(text(error).size(13).color(RED));
        }
        if let Some(error) = &self.settings_error {
            body = body.push(text(error).size(13).color(RED));
        }
        body.into()
    }
    /// 启动区：游戏目录、登录途径与 Dalamud 开关。
    fn launch_section(&self, enabled: bool) -> Element<'_, Msg> {
        let d = &self.draft;
        let injector = d.launch.method == SdoLaunchMethod::Injector;
        let path_enabled = enabled && !self.folder_busy;
        let mut body = column![field(
            "游戏目录",
            row![
                input("game", &d.game, "", path_enabled),
                btn("浏览…", path_enabled.then_some(Msg::Browse), false)
            ]
            .spacing(12)
        )]
        .spacing(10);
        if !d.game.trim().is_empty() {
            let found = if super::state::path(&d.game) == self.config.game_path {
                Some(self.local.found)
            } else {
                self.draft_game_found
            };
            body = body.push(
                text(match found {
                    Some(true) => "已找到 game/ffxiv_dx11.exe",
                    Some(false) => "未找到 game/ffxiv_dx11.exe",
                    None => "正在检查游戏路径…",
                })
                .size(12)
                .color(if found == Some(true) { GREEN } else { MUTED }),
            );
        }
        body = body.push(line()).push(field(
            "启动途径",
            segments(
                "launch_method",
                &["Dalamud Injector（推荐）", "原版 DLL + shim"],
                usize::from(!injector),
                enabled,
            ),
        ));
        if injector {
            body = body.push(line()).push(check(
                "启用 Dalamud（插件框架）",
                d.dalamud.enabled,
                "dalamud",
                enabled,
            ));
            if d.dalamud.enabled {
                let load = match d.dalamud.load_method {
                    DalamudLoadMethod::EntryPoint => 0,
                    DalamudLoadMethod::DllInject => 1,
                    DalamudLoadMethod::AclOnly => 2,
                };
                body = body
                    .push(field(
                        "加载方式",
                        segments(
                            "load",
                            &["入口点 · 推荐", "DLL 注入", "兼容修复"],
                            load,
                            enabled,
                        ),
                    ))
                    .push(field(
                        "初始化延迟",
                        row![
                            input(
                                "delay",
                                &d.delay,
                                &eorzea::dalamud::model::DalamudSettings::default()
                                    .delay_initialize_ms
                                    .to_string(),
                                enabled
                            ),
                            text("毫秒").size(13).color(MUTED)
                        ]
                        .spacing(12)
                        .align_y(iced::Alignment::Center),
                    ))
                    .push(text("0 = 不延迟").size(12).color(MUTED))
                    .push(check(
                        "禁用所有插件（safe mode）",
                        d.dalamud.no_plugins,
                        "safe",
                        enabled,
                    ))
                    .push(check(
                        "禁用第三方插件",
                        d.dalamud.no_third_party_plugins,
                        "third",
                        enabled,
                    ));
            }
            let (track_index, track_labels): (usize, Vec<&'static str>) =
                match d.dalamud.track.as_str() {
                    "staging" => (1, vec!["稳定版", "测试版"]),
                    "release" => (0, vec!["稳定版", "测试版"]),
                    _ => (2, vec!["稳定版", "测试版", "自定义"]),
                };
            body = body
                .push(line())
                .push(field(
                    "更新通道",
                    segments("track", &track_labels, track_index, enabled),
                ))
                .push(field(
                    "安装目录",
                    input(
                        "install",
                        &d.install,
                        &eorzea::dalamud::updater::default_install_root()
                            .display()
                            .to_string(),
                        enabled,
                    ),
                ));
        }
        section("启动", body)
    }
    pub(super) fn save_bar(&self) -> Element<'_, Msg> {
        let enabled =
            self.loaded && !self.critical_busy() && !self.folder_busy && !self.modal_open();
        let changed = self.draft.dirty(&self.config);
        container(
            row![
                btn("恢复", (changed && enabled).then_some(Msg::Restore), false),
                btn(
                    if self.settings_busy {
                        "保存中…"
                    } else {
                        "保存设置"
                    },
                    (changed && enabled && self.draft.apply(&self.config).is_ok())
                        .then_some(Msg::Save),
                    true
                )
            ]
            .spacing(8),
        )
        .padding([12, 28])
        .width(Fill)
        .into()
    }
    pub(super) fn set_field(&mut self, id: &'static str, value: String) {
        if matches!(id, "username" | "password") {
            if !self.login.open
                || matches!(
                    self.login.phase,
                    super::LoginPhase::Requesting
                        | super::LoginPhase::Waiting
                        | super::LoginPhase::Saving
                )
            {
                return;
            }
            if id == "password" {
                self.password_input(value);
            } else {
                self.login.username = value;
            }
            return;
        }
        if self.settings_busy || self.modal_open() {
            return;
        }
        match id {
            "game" if !self.folder_busy => self.change_game_path(value),
            "max" => self.draft.max = value,
            "min" => self.draft.min = value,
            "custom" => self.draft.custom = value,
            "prefix" => self.draft.prefix = value,
            "install" => self.draft.install = value,
            "delay" => self.draft.delay = value,
            _ => {}
        }
    }
    pub(super) fn change_game_path(&mut self, value: String) {
        self.draft.game = value;
        self.path_generation += 1;
        self.draft_game_found = None;
        if let Some(root) = super::state::path(&self.draft.game) {
            let generation = self.path_generation;
            let bus = self.bus.clone();
            self.runtime.spawn_blocking(move || {
                bus.send(super::Event::GamePath(
                    generation,
                    root.join("game/ffxiv_dx11.exe").is_file(),
                ))
            });
        }
    }
    pub(super) fn set_toggle(&mut self, id: &'static str, value: bool) {
        if self.modal_open() {
            return;
        }
        if id == "load" {
            if !self.critical_busy() {
                self.set_dalamud_enabled(value);
            }
            return;
        }
        if self.settings_busy {
            return;
        }
        match id {
            "launcher_check" => self.draft.launcher_update.check_on_startup = value,
            "dalamud" => self.draft.dalamud.enabled = value,
            "safe" => self.draft.dalamud.no_plugins = value,
            "third" => self.draft.dalamud.no_third_party_plugins = value,
            "dxvk" => self.draft.wine.dxvk.enabled = value,
            #[cfg(target_os = "linux")]
            "gamemode" => self.draft.wine.gamemode = value,
            #[cfg(target_os = "macos")]
            "msync" => self.draft.wine.msync = value,
            _ => {}
        }
    }
    pub(super) fn set_segment(&mut self, id: &'static str, n: usize) {
        if id == "method" {
            if self.login.open {
                self.change_login_method(n);
            }
            return;
        }
        if self.settings_busy || self.modal_open() {
            return;
        }
        match id {
            "home_method" => self.set_launch_method(n),
            "launch_method" => {
                self.draft.launch.method = if n == 1 {
                    SdoLaunchMethod::SdoDll
                } else {
                    SdoLaunchMethod::Injector
                }
            }
            "track" => {
                let track = match n {
                    0 => "release",
                    1 => "staging",
                    // “自定义”只表示当前值不是上面两项，点击不改变它。
                    _ => return,
                };
                self.draft.dalamud.track = track.to_string();
            }
            "launcher_channel" => {
                self.draft.launcher_update.channel = match n {
                    1 => eorzea::config::LauncherUpdateChannel::Stable,
                    2 => eorzea::config::LauncherUpdateChannel::Dev,
                    _ => eorzea::config::LauncherUpdateChannel::Auto,
                }
            }
            "launcher_source" => {
                self.draft.launcher_update.source = match n {
                    1 => eorzea::config::LauncherUpdateSource::Mirror,
                    2 => eorzea::config::LauncherUpdateSource::Github,
                    _ => eorzea::config::LauncherUpdateSource::Auto,
                }
            }
            "wine" => {
                self.draft.wine.startup_type = match n {
                    0 => WineStartupType::Auto,
                    1 => WineStartupType::Managed,
                    2 => WineStartupType::Custom,
                    _ => WineStartupType::System,
                }
            }
            #[cfg(target_os = "linux")]
            "sync" => {
                self.draft.wine.sync = match n {
                    0 => SyncMode::None,
                    1 => SyncMode::Esync,
                    _ => SyncMode::Fsync,
                }
                .into()
            }
            "load" => {
                self.draft.dalamud.load_method = match n {
                    0 => DalamudLoadMethod::EntryPoint,
                    1 => DalamudLoadMethod::DllInject,
                    _ => DalamudLoadMethod::AclOnly,
                }
            }
            _ => {}
        }
    }
}
