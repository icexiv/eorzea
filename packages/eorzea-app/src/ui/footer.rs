//! Global version/latest-log strip and a fixed-size, translated log drawer.
use super::{App, Msg, motion, theme::*};
use crate::logging::{LogEntry, LogLevel};
use iced::widget::{button, column, container, opaque, responsive, row, space, svg, tooltip};
use iced::{Element, Fill};

pub const HEIGHT: f32 = 36.;
pub const DRAWER_HEIGHT: f32 = 240.;
const VERSION: &str = concat!("Eorzea ", env!("CARGO_PKG_VERSION"));

fn log_color(level: LogLevel) -> iced::Color {
    match level {
        LogLevel::Info => MUTED,
        LogLevel::Warn => WARNING,
        LogLevel::Error => RED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_is_single_line_unicode_safe_and_ellipsized() {
        assert_eq!(preview("第一行\n第二行", 200.), "第一行 第二行");
        assert_eq!(preview("陆行鸟大区初始化完成", 50.), "陆行鸟…");
        assert_eq!(preview("short", 100.), "short");
        assert_eq!(preview("long", 1.), "…");
        assert!(VERSION.ends_with(env!("CARGO_PKG_VERSION")));
    }
}
/// Conservative width budget; clipping also protects against unusual font metrics.
pub(super) fn preview(value: &str, width: f32) -> String {
    let mut result = String::new();
    let mut used = 0.;
    let budget = (width - 14.).max(0.);
    for character in value.chars() {
        let character = if character.is_control() {
            ' '
        } else {
            character
        };
        used += if character.is_ascii() { 6.5 } else { 12. };
        if used > budget {
            result.push('…');
            break;
        }
        result.push(character);
    }
    result
}
impl App {
    pub(super) fn latest_log(&self) -> Option<&LogEntry> {
        self.logs.last()
    }
    pub(super) fn footer(&self) -> Element<'_, Msg> {
        let (latest, color) = self
            .latest_log()
            .map(|entry| (entry.text.as_str(), log_color(entry.level)))
            .unwrap_or((self.status.as_str(), MUTED));
        let log = responsive(move |size| {
            container(
                text(preview(latest, size.width))
                    .size(12)
                    .color(color)
                    .align_x(iced::alignment::Horizontal::Right)
                    .width(Fill)
                    .wrapping(iced::widget::text::Wrapping::None),
            )
            .width(Fill)
            .center_y(Fill)
            .clip(true)
            .into()
        });
        let path = if self.logs_open {
            "m4 6 4 4 4-4"
        } else {
            "m4 10 4-4 4 4"
        };
        let chevron=svg(svg::Handle::from_memory(format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16' fill='none' stroke='#71717a' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'><path d='{path}'/></svg>").into_bytes())).width(16).height(16);
        let enabled = !self.modal_open();
        let toggle = motion::press(
            button(
                row![text("日志").size(12), chevron]
                    .spacing(6)
                    .align_y(iced::Alignment::Center),
            )
            .height(28)
            .padding([4, 8])
            .style(quiet)
            .on_press_maybe(enabled.then_some(Msg::ToggleLogs)),
            enabled,
        );
        column![
            container(space()).height(1).width(Fill).style(divider),
            container(
                row![
                    text(VERSION).size(12).color(MUTED),
                    space().width(12),
                    tooltip(
                        log,
                        container(text(latest).size(12))
                            .padding(10)
                            .max_width(520)
                            .style(panel),
                        tooltip::Position::Top
                    ),
                    toggle
                ]
                .spacing(12)
                .align_y(iced::Alignment::Center)
            )
            .height(HEIGHT - 1.)
            .padding([0, 16])
            .width(Fill)
        ]
        .height(HEIGHT)
        .width(Fill)
        .into()
    }
    pub(super) fn logs_panel(&self) -> Element<'_, Msg> {
        let enabled = !self.modal_open();
        let paused = self.log_view.paused();
        let mut actions = row![].spacing(8);
        if paused {
            actions = actions.push(btn("跟随最新", enabled.then_some(Msg::FollowLogs), false));
        }
        actions = actions
            .push(btn("复制全部", enabled.then_some(Msg::Copy), false))
            .push(btn("收起", enabled.then_some(Msg::ToggleLogs), false));
        let panel = container(
            column![
                container(space()).height(1).width(Fill).style(divider),
                container(
                    row![
                        heading("运行日志", 14),
                        text(format!(
                            "{} 条 · {}",
                            self.log_view.count(),
                            if paused { "已暂停" } else { "最新在上" }
                        ))
                        .size(12)
                        .color(MUTED),
                        space().width(Fill),
                        actions
                    ]
                    .spacing(12)
                    .align_y(iced::Alignment::Center)
                )
                .padding([8, 16]),
                container(self.log_view.view())
                    .padding([8, 16])
                    .height(Fill),
            ]
            .spacing(0),
        )
        .height(DRAWER_HEIGHT)
        .width(Fill)
        .style(|_| iced::widget::container::Style {
            background: Some(iced::Color::from_rgb8(250, 250, 250).into()),
            ..Default::default()
        });
        motion::reveal(opaque(panel), self.logs_open, enabled)
    }
    pub(super) fn notice_view(&self) -> Element<'_, Msg> {
        if let Some((_, message)) = &self.notice {
            container(
                row![
                    text(message).size(13).width(Fill),
                    button(text("×").size(16))
                        .padding([2, 8])
                        .style(quiet)
                        .on_press(Msg::DismissNotice)
                ]
                .spacing(12)
                .align_y(iced::Alignment::Center),
            )
            .padding([12, 16])
            .max_width(480)
            .style(panel)
            .into()
        } else {
            space().into()
        }
    }
}
