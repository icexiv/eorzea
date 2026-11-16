use super::Msg;
use iced::widget::{button, checkbox, column, container, row, text_input};
use iced::{Background, Border, Color, Element, Fill, Font, Theme};

pub const INK: Color = Color::from_rgb8(9, 9, 11);
pub const MUTED: Color = Color::from_rgb8(113, 113, 122);
pub const LINE: Color = Color::from_rgb8(228, 228, 231);
pub const ACTIVE: Color = Color::from_rgb8(244, 244, 245);
pub const GREEN: Color = Color::from_rgb8(22, 163, 74);
pub const RED: Color = Color::from_rgb8(220, 38, 38);
pub const WARNING: Color = Color::from_rgb8(161, 98, 7);

pub fn theme() -> Theme {
    Theme::custom(
        "Eorzea",
        iced::theme::Palette {
            background: Color::WHITE,
            text: INK,
            primary: INK,
            success: GREEN,
            danger: RED,
            warning: WARNING,
        },
    )
}
pub struct Fonts {
    pub font: Font,
    pub bytes: Vec<Vec<u8>>,
}
static DEFAULT_FONT: std::sync::OnceLock<Font> = std::sync::OnceLock::new();
/// System CJK font, without bundled megabytes or developer-specific paths.
pub fn system_fonts() -> Fonts {
    let mut db = fontdb::Database::new();
    let explicit = std::env::var_os("EORZEA_FONT").and_then(|p| std::fs::read(p).ok());
    if let Some(bytes) = explicit {
        db.load_font_data(bytes);
    } else {
        db.load_system_fonts();
    }
    let names = [
        "Microsoft YaHei",
        "PingFang SC",
        "Noto Sans CJK SC",
        "Noto Sans SC",
        "Source Han Sans SC",
        "HarmonyOS Sans SC",
    ];
    let id = names
        .iter()
        .find_map(|name| {
            db.query(&fontdb::Query {
                families: &[fontdb::Family::Name(name)],
                ..Default::default()
            })
        })
        .or_else(|| std::env::var_os("EORZEA_FONT").and_then(|_| db.faces().next().map(|f| f.id)));
    if let Some(id) = id {
        let family = db.face(id).unwrap().families[0].0.clone();
        let bold = db.query(&fontdb::Query {
            families: &[fontdb::Family::Name(&family)],
            weight: fontdb::Weight::BOLD,
            ..Default::default()
        });
        let mut bytes = Vec::new();
        for face in [Some(id), bold].into_iter().flatten() {
            if let Some(data) = db.with_face_data(face, |data, _| data.to_vec()) {
                if !bytes.contains(&data) {
                    bytes.push(data);
                }
            }
        }
        // Font families are process-lifetime identifiers in Iced's font API.
        let font = Font::with_name(Box::leak(family.into_boxed_str()));
        let _ = DEFAULT_FONT.set(font);
        Fonts { font, bytes }
    } else {
        tracing::warn!("没有找到系统中文字体；可安装 Noto Sans CJK 或设置 EORZEA_FONT");
        Fonts {
            font: Font::DEFAULT,
            bytes: Vec::new(),
        }
    }
}
pub fn text<'a>(value: impl iced::widget::text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    iced::widget::text(value).line_height(1.428_571_5)
}
pub fn heading<'a>(
    value: impl iced::widget::text::IntoFragment<'a>,
    size: u32,
) -> iced::widget::Text<'a> {
    text(value).size(size).font(Font {
        weight: iced::font::Weight::Semibold,
        ..DEFAULT_FONT.get().copied().unwrap_or(Font::DEFAULT)
    })
}
pub fn panel(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::WHITE.into()),
        text_color: Some(INK),
        border: Border {
            width: 1.,
            color: LINE,
            radius: 8.into(),
        },
        ..Default::default()
    }
}
pub fn divider(_: &Theme) -> container::Style {
    container::Style {
        background: Some(LINE.into()),
        ..Default::default()
    }
}
pub fn summary(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Color::from_rgb8(250, 250, 250).into()),
        border: Border {
            radius: 8.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
pub fn quiet(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: match status {
            button::Status::Hovered => Some(ACTIVE.into()),
            button::Status::Pressed => Some(LINE.into()),
            _ => None,
        },
        text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            INK
        } else {
            MUTED
        },
        border: Border {
            radius: 4.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
pub fn card<'a>(body: impl Into<Element<'a, Msg>>) -> iced::widget::Container<'a, Msg> {
    container(body).padding(20).width(Fill).style(panel)
}
pub fn section<'a>(title: &'a str, body: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    card(column![heading(title, 15), body.into()].spacing(12)).into()
}
pub fn primary(_: &Theme, status: button::Status) -> button::Style {
    let disabled = status == button::Status::Disabled;
    button::Style {
        background: Some(
            match status {
                button::Status::Disabled => LINE,
                button::Status::Active => Color::from_rgb8(24, 24, 27),
                button::Status::Hovered => Color::from_rgb8(48, 48, 52),
                button::Status::Pressed => INK,
            }
            .into(),
        ),
        text_color: if disabled { MUTED } else { Color::WHITE },
        border: Border {
            radius: 6.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
pub fn secondary(_: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(
            match status {
                button::Status::Active => Color::WHITE,
                button::Status::Hovered | button::Status::Disabled => ACTIVE,
                button::Status::Pressed => LINE,
            }
            .into(),
        ),
        text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
            INK
        } else {
            MUTED
        },
        border: Border {
            width: 1.,
            color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                Color::from_rgb8(161, 161, 170)
            } else {
                LINE
            },
            radius: 6.into(),
        },
        ..Default::default()
    }
}
pub fn danger(t: &Theme, s: button::Status) -> button::Style {
    let mut style = secondary(t, s);
    if s != button::Status::Disabled {
        style.text_color = RED;
        style.border.color = RED;
        if matches!(s, button::Status::Hovered | button::Status::Pressed) {
            style.background = Some(
                if s == button::Status::Pressed {
                    Color::from_rgb8(254, 226, 226)
                } else {
                    Color::from_rgb8(254, 242, 242)
                }
                .into(),
            );
        }
    }
    style
}
pub fn btn<'a>(label: &'a str, msg: Option<Msg>, black: bool) -> Element<'a, Msg> {
    let enabled = msg.is_some();
    super::motion::press(
        button(text(label).size(if black { 14 } else { 13 }))
            .height(36)
            .padding([8, 16])
            .on_press_maybe(msg)
            .style(if black { primary } else { secondary }),
        enabled,
    )
}
pub fn input_style(_: &Theme, s: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Background::Color(Color::WHITE),
        border: Border {
            width: 1.,
            color: if matches!(s, text_input::Status::Focused { .. }) {
                MUTED
            } else {
                Color::from_rgb8(212, 212, 216)
            },
            radius: 6.into(),
        },
        icon: MUTED,
        placeholder: MUTED,
        value: INK,
        selection: LINE,
    }
}
pub fn input<'a>(id: &'static str, value: &'a str, hint: &str, enabled: bool) -> Element<'a, Msg> {
    let mut input = text_input(hint, value)
        .id(id)
        .secure(id == "password")
        .size(14)
        .padding([10, 12])
        .style(input_style);
    if enabled {
        input = input.on_input(move |value| Msg::Field(id, value));
    }
    input.into()
}
pub fn field<'a>(label: &'a str, body: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    row![text(label).size(13).color(MUTED).width(92), body.into()]
        .spacing(12)
        .align_y(iced::Alignment::Center)
        .into()
}
pub fn check<'a>(label: &'a str, value: bool, id: &'static str, enabled: bool) -> Element<'a, Msg> {
    let mut check = checkbox(value)
        .label(label)
        .size(16)
        .text_size(14)
        .spacing(8);
    if enabled {
        check = check.on_toggle(move |value| Msg::Toggle(id, value));
    }
    check.into()
}
pub fn segments<'a>(
    id: &'static str,
    labels: &[&'a str],
    selected: usize,
    enabled: bool,
) -> Element<'a, Msg> {
    let method = id == "method";
    let mut items = row![].spacing(4);
    for (n, label) in labels.iter().enumerate() {
        items = items.push(
            button(container(text(*label).size(13)).center_x(if method {
                iced::Length::Fill
            } else {
                iced::Length::Shrink
            }))
            .width(if method {
                iced::Length::Fill
            } else {
                iced::Length::Shrink
            })
            .padding([5, 10])
            .on_press_maybe(enabled.then_some(Msg::Segment(id, n)))
            .style(move |_, status| button::Style {
                background: Some(
                    match status {
                        button::Status::Hovered | button::Status::Pressed => LINE,
                        _ if n == selected => Color::WHITE,
                        _ => ACTIVE,
                    }
                    .into(),
                ),
                text_color: if n == selected
                    || matches!(status, button::Status::Hovered | button::Status::Pressed)
                {
                    INK
                } else {
                    MUTED
                },
                border: Border {
                    radius: 4.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
        );
    }
    container(items)
        .padding(3)
        .width(if method {
            iced::Length::Fill
        } else {
            iced::Length::Shrink
        })
        .style(|_| container::Style {
            background: Some(ACTIVE.into()),
            border: Border {
                radius: 6.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}
