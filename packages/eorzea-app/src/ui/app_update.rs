use super::{App, Msg, backend, state::TaskSlot, theme::*};
use crate::{
    build_info::Distribution,
    updates::{CheckResult, Comparison},
};
use eorzea::config::{LauncherUpdateChannel, LauncherUpdateSource};
use iced::{
    Element,
    widget::{column, row},
};

#[derive(Default)]
pub(super) struct State {
    pub task: TaskSlot,
    pub checking: bool,
    pub result: Option<Result<CheckResult, String>>,
    pub link_error: Option<String>,
}

impl App {
    pub(super) fn start_launcher_check(&mut self, force: bool) {
        if !self.loaded || self.launcher_update.checking {
            return;
        }
        self.launcher_update.task.cancel();
        self.launcher_update.checking = true;
        self.launcher_update.link_error = None;
        self.launcher_update.task.handle = Some(backend::check_launcher(
            &self.runtime,
            self.bus.clone(),
            self.launcher_update.task.generation,
            self.build_info.clone(),
            self.config.launcher_update.clone(),
            force,
        ));
    }
    pub(super) fn launcher_update_available(&self) -> bool {
        matches!(&self.launcher_update.result, Some(Ok(result)) if result.comparison == Comparison::Newer)
    }
    pub(super) fn launcher_update_section(&self) -> Element<'_, Msg> {
        let settings = &self.draft.launcher_update;
        let enabled = self.loaded && !self.settings_busy && !self.modal_open();
        let channel = match settings.channel {
            LauncherUpdateChannel::Auto => 0,
            LauncherUpdateChannel::Stable => 1,
            LauncherUpdateChannel::Dev => 2,
        };
        let source = match settings.source {
            LauncherUpdateSource::Auto => 0,
            LauncherUpdateSource::Mirror => 1,
            LauncherUpdateSource::Github => 2,
        };
        let mut content = column![
            text(self.build_info.display()).size(13),
            text(self.build_info.distribution.label())
                .size(12)
                .color(MUTED),
            check(
                "启动时检查启动器更新",
                settings.check_on_startup,
                "launcher_check",
                enabled
            ),
            field(
                "更新通道",
                segments(
                    "launcher_channel",
                    &["跟随构建", "稳定版", "开发版"],
                    channel,
                    enabled
                )
            ),
            field(
                "检查来源",
                segments(
                    "launcher_source",
                    &["自动", "国内镜像", "GitHub"],
                    source,
                    enabled
                )
            ),
        ]
        .spacing(12);
        if let Some(commit) = self.build_info.commit {
            content = content.push(text(format!("提交：{commit}")).size(12).color(MUTED));
        }
        let mut actions = row![btn(
            if self.launcher_update.checking {
                "正在检查…"
            } else {
                "检查启动器更新"
            },
            (self.loaded && !self.modal_open() && !self.launcher_update.checking)
                .then_some(Msg::CheckLauncher),
            false,
        )]
        .spacing(8);
        if let Some(result) = &self.launcher_update.result {
            match result {
                Ok(result) => {
                    content = content.push(text(result.message()).size(13).color(
                        if result.comparison == Comparison::Newer {
                            GREEN
                        } else {
                            INK
                        },
                    ));
                    let mut details = format!(
                        "{} · {}{}",
                        result.channel.label(),
                        result.source.label(),
                        if result.cached {
                            " · 缓存结果"
                        } else {
                            ""
                        }
                    );
                    if let Some(release) = &result.release {
                        details.push_str(&format!(" · 发布于 {}", release.published_at));
                        actions = actions.push(btn(
                            "查看发布说明",
                            (!self.modal_open()).then_some(Msg::ReleaseNotes),
                            false,
                        ));
                        if !result.platform_available {
                            content = content
                                .push(text("该版本没有当前平台的发行包。").size(12).color(MUTED));
                        }
                    }
                    content = content.push(text(details).size(12).color(MUTED));
                }
                Err(error) => {
                    content = content.push(text(format!("检查失败：{error}")).size(12).color(RED))
                }
            }
        }
        if self.build_info.distribution == Distribution::Nix {
            actions = actions.push(btn(
                "Nix 升级指南",
                (!self.modal_open()).then_some(Msg::NixGuide),
                false,
            ));
        }
        content = content.push(actions.wrap());
        if let Some(error) = &self.launcher_update.link_error {
            content = content.push(text(error).size(12).color(RED));
        }
        section("启动器更新", content)
    }
}
