use super::{App, Msg, backend, state::Operation, theme::*};
use eorzea::{
    dalamud::InstallState,
    game_files::{InstallPhase, UpdateProgress, patch_manager::PatchPhase},
};
use iced::widget::{button, column, container, progress_bar, row};
use iced::{Element, Fill};

impl App {
    pub(super) fn home(&self) -> Element<'_, Msg> {
        let enabled = !self.critical_busy() && !self.modal_open();
        let accounts = self
            .auth
            .accounts
            .iter()
            .map(|a| (a.snda_id.clone(), a.display_name().to_string()))
            .collect();
        let areas = self
            .areas
            .iter()
            .map(|a| (a.area_id.clone(), a.area_name.clone()))
            .collect();
        let mut launch = column![
            row![
                row![
                    text("账号").size(13).color(MUTED),
                    self.dropdown(
                        "account",
                        accounts,
                        self.selected_account.as_deref(),
                        "请选择账号",
                        enabled
                    )
                ]
                .spacing(12)
                .width(Fill)
                .align_y(iced::Alignment::Center),
                row![
                    text("大区").size(13).color(MUTED),
                    self.dropdown(
                        "area",
                        areas,
                        self.selected_area.as_deref(),
                        if self.area_loading {
                            "大区列表加载中…"
                        } else {
                            "请选择大区"
                        },
                        enabled
                    )
                ]
                .spacing(12)
                .width(Fill)
                .align_y(iced::Alignment::Center)
            ]
            .spacing(16)
            .align_y(iced::Alignment::Start)
        ]
        .spacing(8);
        if let Some(error) = &self.area_error {
            launch = launch.push(text(error).size(12).color(RED)).push(btn(
                "重新获取大区",
                (!self.area_loading).then_some(Msg::RetryAreas),
                false,
            ));
        }
        let mut body = column![section("账号与大区", launch)].spacing(12);
        let cards = self.cards();
        for pair in cards.chunks(2) {
            let mut stats = row![].spacing(12);
            for (title, main, detail, color) in pair {
                stats = stats.push(
                    container(
                        column![
                            text(*title).size(12).color(MUTED),
                            text(main.clone()).size(14).color(*color),
                            text(detail.clone()).size(12).color(MUTED)
                        ]
                        .spacing(4),
                    )
                    .padding(12)
                    .width(Fill)
                    .style(summary),
                );
            }
            body = body.push(stats);
        }
        let mut update = column![
            row![
                btn(
                    if matches!(self.operation, Operation::Checking | Operation::Updating) {
                        "更新任务进行中…"
                    } else {
                        "检查更新"
                    },
                    enabled.then_some(Msg::Check),
                    true
                ),
                btn(
                    "更新游戏",
                    (enabled && !self.patches.is_empty()).then_some(Msg::Update),
                    true
                )
            ]
            .spacing(8)
        ]
        .spacing(12);
        if !self.update_message.is_empty() {
            update = update.push(
                text(&self.update_message)
                    .size(13)
                    .color(if self.update_error { RED } else { MUTED }),
            );
        }
        if let Some(progress) = &self.update_progress {
            update = update.push(progress_panel(progress));
        }
        body.push(section("游戏更新", update))
            .push(section("启动", self.home_launch_body(enabled)))
            .into()
    }
    /// 主页的启动区：途径选择与本次插件开关放在启动按钮旁。
    fn home_launch_body(&self, enabled: bool) -> Element<'_, Msg> {
        let injector = self.config.launch.method == eorzea::config::SdoLaunchMethod::Injector;
        let mut body = column![field(
            "启动途径",
            segments(
                "home_method",
                &["Dalamud Injector（推荐）", "原版 DLL + shim"],
                usize::from(!injector),
                enabled,
            ),
        )]
        .spacing(10);
        if injector {
            body = body.push(check(
                "启用 Dalamud（插件框架）",
                self.config.dalamud.enabled,
                "load",
                enabled,
            ));
        }
        body.push(launch_button(
            if self.operation == Operation::Launching {
                "启动中…"
            } else {
                "启动游戏"
            },
            enabled && self.launcher.is_some(),
        ))
        .push(if self.operation == Operation::Launching {
            text(&self.status).size(13).color(MUTED)
        } else {
            text("").size(1)
        })
        .into()
    }
    fn cards(&self) -> Vec<(&'static str, String, String, iced::Color)> {
        let mut cards = vec![
            (
                "游戏位置",
                self.config
                    .game_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "未配置".into()),
                if self.config.game_path.is_none() {
                    "请到设置页选择游戏根目录。".into()
                } else if self.local.found {
                    "已找到 game/ffxiv_dx11.exe".into()
                } else {
                    "未找到 game/ffxiv_dx11.exe".into()
                },
                if self.local.found { GREEN } else { WARNING },
            ),
            (
                "游戏版本",
                self.local
                    .versions
                    .as_ref()
                    .map(|(_, game)| format!("game: {game}"))
                    .unwrap_or_else(|| "未配置".into()),
                self.local
                    .versions
                    .as_ref()
                    .map(|(boot, _)| format!("boot: {boot}"))
                    .unwrap_or_default(),
                INK,
            ),
        ];
        let (main, detail, color) =
            if self.config.launch.method != eorzea::config::SdoLaunchMethod::Injector {
                (
                    "不可用".into(),
                    "原版 DLL 启动途径不加载 Dalamud。".into(),
                    MUTED,
                )
            } else if !self.config.dalamud.enabled {
                ("已关闭".into(), "在「启动」区启用后加载".into(), MUTED)
            } else if self.dalamud_loading {
                ("检查中…".into(), "正在核对本地安装与游戏版本".into(), MUTED)
            } else if let Some(status) = &self.dalamud {
                match &status.install_state {
                    InstallState::Ready => (
                        format!(
                            "已安装 {}",
                            status
                                .local_assembly_version
                                .as_deref()
                                .unwrap_or("未知版本")
                        ),
                        if status.remote_from_cache {
                            "版本匹配 · 使用本地元数据缓存".into()
                        } else {
                            "版本匹配，启动时加载".into()
                        },
                        GREEN,
                    ),
                    InstallState::Missing => {
                        ("未安装".into(), "启动时自动准备 release".into(), WARNING)
                    }
                    InstallState::OutOfDate => (
                        "需要更新".into(),
                        format!("本地游戏版本 {}", status.local_game_ver),
                        WARNING,
                    ),
                    InstallState::Unsupported => (
                        "暂不兼容".into(),
                        format!("release 尚未支持游戏版本 {}", status.local_game_ver),
                        RED,
                    ),
                    InstallState::RuntimeMissing => (
                        "缺少 .NET Runtime".into(),
                        "启动时自动准备 Windows x64 runtime".into(),
                        WARNING,
                    ),
                    InstallState::AssetsMissing => {
                        ("缺少 Assets".into(), "启动时自动准备资源".into(), WARNING)
                    }
                    InstallState::Failed(error) => ("安装异常".into(), error.clone(), RED),
                }
            } else {
                (
                    "无法获取状态".into(),
                    "启动时重新检查；准备失败不会启动游戏。".into(),
                    WARNING,
                )
            };
        cards.push(("Dalamud", main, detail, color));
        #[cfg(not(target_os = "windows"))]
        cards.push((
            "Wine",
            format!(
                "启动方式：{}",
                match self.config.settings.startup_type {
                    eorzea::config::WineStartupType::Auto => "自动",
                    eorzea::config::WineStartupType::Managed => "托管",
                    eorzea::config::WineStartupType::Custom => "自定义",
                    eorzea::config::WineStartupType::System => "系统",
                }
            ),
            self.local.wine.clone(),
            INK,
        ));
        cards
    }
    pub(super) fn begin_operation(&mut self, operation: Operation) -> Option<u64> {
        if self.critical_busy() {
            self.status = "请等待当前任务完成".into();
            return None;
        }
        self.operation_generation += 1;
        self.operation = operation;
        Some(self.operation_generation)
    }
    pub(super) fn check_update(&mut self) {
        let Some(root) = self.config.game_path.clone() else {
            self.status = "请先在设置页配置游戏目录".into();
            return;
        };
        let Some(generation) = self.begin_operation(Operation::Checking) else {
            return;
        };
        self.update_message = "正在检查更新…".into();
        self.update_error = false;
        self.update_progress = None;
        self.patches.clear();
        backend::check(
            &self.runtime,
            self.bus.clone(),
            generation,
            root,
            self.selected_area(),
        );
    }
    pub(super) fn run_update(&mut self) {
        let Some(root) = self.config.game_path.clone() else {
            return;
        };
        if self.patches.is_empty() {
            return;
        }
        if let Err(e) = self.config.download.validate() {
            self.status = format!("下载配置无效: {e}");
            return;
        }
        let Some(generation) = self.begin_operation(Operation::Updating) else {
            return;
        };
        self.update_message = "正在更新游戏…".into();
        self.update_error = false;
        self.update_progress = None;
        backend::update(
            &self.runtime,
            self.bus.clone(),
            generation,
            root,
            self.patches.clone(),
            self.config.download.clone(),
        );
    }
    pub(super) fn launch_game(&mut self) {
        let Some(account) = self.selected_account.clone() else {
            self.status = "请先选择账号；没有账号请到账号管理添加".into();
            return;
        };
        let Some(area) = self.selected_area() else {
            self.status = "请选择大区".into();
            return;
        };
        let Some(root) = self.config.game_path.clone() else {
            self.status = "请先在设置页配置游戏目录".into();
            return;
        };
        let Some(launcher) = self.launcher.clone() else {
            self.status = "启动器尚未初始化完成".into();
            return;
        };
        let Some(generation) = self.begin_operation(Operation::Launching) else {
            return;
        };
        self.status = "正在准备启动…".into();
        #[cfg(not(target_os = "windows"))]
        let wine = self.config.settings.clone();
        #[cfg(target_os = "windows")]
        let wine = Default::default();
        backend::launch(
            &self.runtime,
            self.bus.clone(),
            generation,
            launcher,
            backend::Launch {
                root,
                account,
                area,
                areas: self.areas.clone(),
                wine,
                dalamud: self.config.launch.method
                    == eorzea::config::SdoLaunchMethod::Injector
                    && self.config.dalamud.enabled,
                launch_method: self.config.launch.method,
            },
        );
    }
    /// 主页开关与设置页共用同一份配置，点击后立即保存。
    pub(super) fn set_dalamud_enabled(&mut self, value: bool) {
        if self.critical_busy() || self.modal_open() || self.dalamud_pending.is_some() {
            return;
        }
        let previous = self.config.dalamud.enabled;
        if previous == value {
            return;
        }
        self.config.dalamud.enabled = value;
        self.draft.dalamud.enabled = value;
        self.dalamud_pending = Some(previous);
        backend::save_dalamud_enabled(&self.runtime, self.bus.clone(), value);
    }
    /// 主页切换途径立即持久化，不改动 Dalamud 开关。
    /// 先更新界面再写盘，避免控件在保存往返期间变灰或跳回旧值。
    pub(super) fn set_launch_method(&mut self, index: usize) {
        if self.critical_busy() || self.modal_open() || self.launch_method_pending.is_some() {
            return;
        }
        let method = if index == 1 {
            eorzea::config::SdoLaunchMethod::SdoDll
        } else {
            eorzea::config::SdoLaunchMethod::Injector
        };
        let previous = self.config.launch.method;
        if previous == method {
            return;
        }
        self.config.launch.method = method;
        self.draft.launch.method = method;
        self.launch_method_pending = Some((method, previous));
        backend::save_launch_method(&self.runtime, self.bus.clone(), method);
    }
}
pub(super) fn launch_button(label: &str, enabled: bool) -> Element<'_, Msg> {
    super::motion::press(
        button(
            container(text(label).size(18))
                .center_x(Fill)
                .center_y(Fill),
        )
        .padding(0)
        .height(56)
        .width(Fill)
        .on_press_maybe(enabled.then_some(Msg::Launch))
        .style(primary),
        enabled,
    )
}
fn human_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024. * 1024. * 1024.))
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024. * 1024.))
    }
}
pub(super) fn progress_rows(progress: &UpdateProgress) -> Vec<(String, u64, u64)> {
    let install = &progress.install;
    let download = &progress.download;
    let mut rows = vec![(
        format!(
            "补丁应用（{} / {}）",
            install.completed, install.total_patches
        ),
        install.done,
        install.total,
    )];
    if let Some(current) = &install.current {
        rows.push((
            format!(
                "#{} {} · {}",
                current.index + 1,
                current.version,
                match current.phase {
                    InstallPhase::Verifying => "校验中",
                    InstallPhase::Applying => "应用中",
                    InstallPhase::Committing => "提交 / 清理中",
                }
            ),
            current.done,
            current.total,
        ));
    } else if progress.waiting_for_download {
        rows.push(("等待队头补丁下载和校验完成…".into(), 0, 0));
    }
    rows.push(("补丁下载".into(), download.done, download.total));
    for file in &download.active {
        rows.push((
            format!(
                "#{} {} · {}",
                file.index + 1,
                file.version,
                match file.phase {
                    PatchPhase::Downloading => "下载中",
                    PatchPhase::Verifying => "校验中",
                }
            ),
            file.done,
            file.total,
        ));
    }
    for file in &progress.ready {
        rows.push((format!("✓ 等待应用：{}", file.version), 0, 0));
    }
    rows
}
fn progress_panel(progress: &UpdateProgress) -> Element<'_, Msg> {
    let mut body = column![].spacing(6);
    for (label, done, total) in progress_rows(progress) {
        let fraction = if total > 0 {
            (done as f64 / total as f64).clamp(0., 1.) as f32
        } else {
            0.
        };
        body = body.push(
            text(if total > 0 {
                format!(
                    "{label} · {} / {} · {:.1}%",
                    human_bytes(done),
                    human_bytes(total),
                    fraction * 100.
                )
            } else {
                label
            })
            .size(12)
            .color(MUTED),
        );
        if total > 0 {
            body = body.push(progress_bar(0.0..=1.0, fraction).girth(4));
        }
    }
    body.into()
}
