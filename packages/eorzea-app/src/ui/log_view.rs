//! Read-only native text selection across log records; live updates never replace a selection.
use super::{Msg, motion, theme::*};
use crate::logging::{LogEntry, LogLevel};
use iced::advanced::text::{Highlighter, highlighter};
use iced::widget::text_editor::{self, Action, Binding};
use iced::{Element, Fill};
use std::{ops::Range, sync::Arc};

#[derive(Default)]
pub struct LogView {
    content: text_editor::Content,
    rendered: String,
    levels: Arc<Vec<LogLevel>>,
    paused: bool,
    count: usize,
}
impl LogView {
    pub fn paused(&self) -> bool {
        self.paused
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn sync(&mut self, entries: &[LogEntry]) {
        if self.paused {
            return;
        }
        let mut text = String::new();
        let mut levels = Vec::new();
        for (index, entry) in entries.iter().rev().enumerate() {
            if index > 0 {
                text.push('\n');
            }
            let line = entry.text.replace("\r\n", "\n").replace('\r', "\n");
            levels.extend(line.split('\n').map(|_| entry.level));
            text.push_str(&line);
        }
        if text != self.rendered {
            self.content = text_editor::Content::with_text(&text);
            self.rendered = text;
        }
        self.levels = Arc::new(levels);
        self.count = entries.len();
    }
    pub fn action(&mut self, action: Action) {
        if !action.is_edit() {
            self.paused = true;
            self.content.perform(action);
        }
    }
    pub fn follow(&mut self, entries: &[LogEntry]) {
        self.paused = false;
        self.sync(entries);
        self.content
            .perform(Action::Move(text_editor::Motion::DocumentStart));
    }
    pub fn view(&self) -> Element<'_, Msg> {
        let editor = iced::widget::text_editor(&self.content)
            .id("selectable-logs")
            .height(Fill)
            .size(12)
            .padding(0)
            .line_height(iced::widget::text::LineHeight::Absolute(18.into()))
            .placeholder("暂无日志")
            .on_action(Msg::LogAction)
            .key_binding(|event| {
                Binding::from_key_press(event).filter(|binding| {
                    matches!(
                        binding,
                        Binding::Copy
                            | Binding::Move(_)
                            | Binding::Select(_)
                            | Binding::SelectWord
                            | Binding::SelectLine
                            | Binding::SelectAll
                            | Binding::Unfocus
                    )
                })
            })
            .highlight_with::<LogHighlighter>(self.levels.clone(), |level, _| highlighter::Format {
                color: Some(match level {
                    LogLevel::Info => MUTED,
                    LogLevel::Warn => WARNING,
                    LogLevel::Error => RED,
                }),
                font: None,
            })
            .style(|_, _| text_editor::Style {
                background: iced::Color::TRANSPARENT.into(),
                border: iced::Border::default(),
                placeholder: MUTED,
                // tiny-skia multiplies per-glyph alpha by this base color's alpha.
                value: MUTED,
                selection: iced::Color::from_rgba8(9, 9, 11, 0.14),
            });
        motion::read_only(editor)
    }
}

struct LogHighlighter {
    levels: Arc<Vec<LogLevel>>,
    line: usize,
}
impl Highlighter for LogHighlighter {
    type Settings = Arc<Vec<LogLevel>>;
    type Highlight = LogLevel;
    type Iterator<'a> = std::iter::Once<(Range<usize>, LogLevel)>;
    fn new(settings: &Self::Settings) -> Self {
        Self {
            levels: settings.clone(),
            line: 0,
        }
    }
    fn update(&mut self, settings: &Self::Settings) {
        self.levels = settings.clone();
        self.line = 0;
    }
    fn change_line(&mut self, line: usize) {
        self.line = line;
    }
    fn highlight_line(&mut self, line: &str) -> Self::Iterator<'_> {
        let level = self
            .levels
            .get(self.line)
            .copied()
            .unwrap_or(LogLevel::Info);
        self.line += 1;
        std::iter::once((0..line.len(), level))
    }
    fn current_line(&self) -> usize {
        self.line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn records() -> Vec<LogEntry> {
        vec![
            LogEntry {
                level: LogLevel::Info,
                text: "第一条日志".into(),
            },
            LogEntry {
                level: LogLevel::Error,
                text: "第二条日志\n详细信息".into(),
            },
        ]
    }
    #[test]
    fn logs_select_across_lines_without_allowing_edits() {
        let mut view = LogView::default();
        view.sync(&records());
        view.action(Action::SelectAll);
        assert_eq!(
            view.content.selection().as_deref(),
            Some("第二条日志\n详细信息\n第一条日志")
        );
        for edit in [
            text_editor::Edit::Insert('x'),
            text_editor::Edit::Delete,
            text_editor::Edit::Paste(Arc::new("replaced".into())),
        ] {
            view.action(Action::Edit(edit));
        }
        assert_eq!(view.content.text(), "第二条日志\n详细信息\n第一条日志");
    }
    #[test]
    fn arriving_logs_preserve_selection_until_follow_is_requested() {
        let mut entries = records();
        let mut view = LogView::default();
        view.sync(&entries);
        view.action(Action::SelectAll);
        let selection = view.content.selection();
        entries.push(LogEntry {
            level: LogLevel::Warn,
            text: "最新日志".into(),
        });
        view.sync(&entries);
        assert!(view.paused());
        assert_eq!(view.content.selection(), selection);
        assert_eq!(view.count(), 2);
        view.follow(&entries);
        assert!(!view.paused());
        assert_eq!(view.count(), 3);
        assert!(view.content.text().starts_with("最新日志\n"));
        assert!(view.content.selection().is_none());
    }
    #[test]
    fn multiline_records_keep_their_severity_color() {
        let mut view = LogView::default();
        view.sync(&records());
        let mut highlighter = LogHighlighter::new(&view.levels);
        assert_eq!(
            highlighter.highlight_line("第二条日志").next().unwrap().1,
            LogLevel::Error
        );
        assert_eq!(
            highlighter.highlight_line("详细信息").next().unwrap().1,
            LogLevel::Error
        );
        assert_eq!(
            highlighter.highlight_line("第一条日志").next().unwrap().1,
            LogLevel::Info
        );
    }
    #[test]
    fn follow_clears_selection_even_without_new_logs() {
        let entries = records();
        let mut view = LogView::default();
        view.sync(&entries);
        view.action(Action::SelectAll);
        view.follow(&entries);
        assert!(view.content.selection().is_none());
        assert!(!view.paused());
    }
    #[derive(Default)]
    struct Clipboard {
        copied: Option<String>,
    }
    impl iced::advanced::Clipboard for Clipboard {
        fn read(&self, _kind: iced::advanced::clipboard::Kind) -> Option<String> {
            panic!("read-only log view must not read pasted clipboard content")
        }
        fn write(&mut self, _kind: iced::advanced::clipboard::Kind, text: String) {
            self.copied = Some(text);
        }
    }
    fn key(
        character: &str,
        code: iced::keyboard::key::Code,
        modifiers: iced::keyboard::Modifiers,
    ) -> iced::Event {
        let key = iced::keyboard::Key::Character(character.into());
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: iced::keyboard::key::Physical::Code(code),
            location: iced::keyboard::Location::Standard,
            modifiers,
            text: Some(character.into()),
            repeat: false,
        })
    }
    fn dispatch(
        view: &mut LogView,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &iced::Renderer,
        event: &iced::Event,
        cursor: iced::advanced::mouse::Cursor,
        clipboard: &mut Clipboard,
    ) -> iced::window::RedrawRequest {
        use iced::advanced::{InputMethod, Layout, Shell, layout};
        let mut element = view.view();
        tree.diff(element.as_widget());
        let size = iced::Size::new(400., 160.);
        let node = element.as_widget_mut().layout(
            tree,
            renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        element.as_widget_mut().update(
            tree,
            event,
            Layout::new(&node),
            cursor,
            renderer,
            clipboard,
            &mut shell,
            &iced::Rectangle::with_size(size),
        );
        let redraw = shell.redraw_request();
        assert!(matches!(shell.input_method(), InputMethod::Disabled));
        drop(shell);
        drop(element);
        for message in messages {
            if let Msg::LogAction(action) = message {
                view.action(action);
            }
        }
        redraw
    }
    #[test]
    fn native_drag_and_copy_work_without_a_desktop_clipboard_or_caret_timer() {
        use iced::advanced::{mouse, widget::Tree};
        let mut view = LogView::default();
        view.sync(&records());
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, 12.into());
        let mut tree = Tree::new(view.view().as_widget());
        let mut clipboard = Clipboard::default();
        let start = iced::Point::new(2., 2.);
        let end = iced::Point::new(180., 32.);
        dispatch(
            &mut view,
            &mut tree,
            &renderer,
            &iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            mouse::Cursor::Available(start),
            &mut clipboard,
        );
        dispatch(
            &mut view,
            &mut tree,
            &renderer,
            &iced::Event::Mouse(mouse::Event::CursorMoved { position: end }),
            mouse::Cursor::Available(end),
            &mut clipboard,
        );
        dispatch(
            &mut view,
            &mut tree,
            &renderer,
            &iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            mouse::Cursor::Available(end),
            &mut clipboard,
        );
        assert!(
            view.content.selection().unwrap().contains('\n'),
            "drag should select across records/lines"
        );
        let command = if cfg!(target_os = "macos") {
            iced::keyboard::Modifiers::LOGO
        } else {
            iced::keyboard::Modifiers::CTRL
        };
        dispatch(
            &mut view,
            &mut tree,
            &renderer,
            &key("a", iced::keyboard::key::Code::KeyA, command),
            mouse::Cursor::Available(end),
            &mut clipboard,
        );
        dispatch(
            &mut view,
            &mut tree,
            &renderer,
            &key("c", iced::keyboard::key::Code::KeyC, command),
            mouse::Cursor::Available(end),
            &mut clipboard,
        );
        assert_eq!(
            clipboard.copied.as_deref(),
            Some("第二条日志\n详细信息\n第一条日志")
        );
        for event in [
            key("v", iced::keyboard::key::Code::KeyV, command),
            key("x", iced::keyboard::key::Code::KeyX, command),
            key(
                "z",
                iced::keyboard::key::Code::KeyZ,
                iced::keyboard::Modifiers::default(),
            ),
        ] {
            dispatch(
                &mut view,
                &mut tree,
                &renderer,
                &event,
                mouse::Cursor::Available(end),
                &mut clipboard,
            );
        }
        assert_eq!(view.content.text(), "第二条日志\n详细信息\n第一条日志");
        let redraw = dispatch(
            &mut view,
            &mut tree,
            &renderer,
            &iced::Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            )),
            mouse::Cursor::Available(end),
            &mut clipboard,
        );
        assert_eq!(
            redraw,
            iced::window::RedrawRequest::Wait,
            "read-only selection must not keep scheduling caret blink redraws"
        );
    }
    #[test]
    fn software_renderer_draws_visible_highlighted_log_glyphs() {
        use iced::advanced::{Layout, layout, mouse, renderer, widget::Tree};
        let mut view = LogView::default();
        view.sync(&records());
        let mut element = view.view();
        let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, 12.into());
        let mut tree = Tree::new(element.as_widget());
        let size = iced::Size::new(400., 160.);
        let viewport = iced::Rectangle::with_size(size);
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        element.as_widget().draw(
            &tree,
            &mut renderer,
            &theme(),
            &renderer::Style::default(),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &viewport,
        );
        let mut pixels = tiny_skia::Pixmap::new(400, 160).unwrap();
        let mut mask = tiny_skia::Mask::new(400, 160).unwrap();
        renderer.draw(
            &mut pixels.as_mut(),
            &mut mask,
            &iced::advanced::graphics::Viewport::with_physical_size(iced::Size::new(400, 160), 1.),
            &[viewport],
            iced::Color::WHITE,
        );
        // Renderer output is BGRA: a red ERROR glyph has a large third channel.
        assert!(
            pixels
                .pixels()
                .iter()
                .any(|p| p.blue() > 150 && p.red() < 100 && p.green() < 100),
            "ERROR text must be visible and colored, not transparent"
        );
    }
}
