use super::*;
use iced::advanced::{Layout, layout, mouse, renderer, widget::Tree};

fn app() -> App {
    let runtime = Arc::new(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap(),
    );
    let mut app = App::empty(runtime);
    app.loaded = true;
    app.area_loading = false;
    app
}
fn draw(app: &App, width: f32) {
    // CPU-only layout and primitive generation. No event loop, compositor or native window.
    let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, 14.into());
    let mut element = app.view();
    let mut tree = Tree::new(element.as_widget());
    let size = iced::Size::new(width, 860.);
    let node = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(iced::Size::ZERO, size),
    );
    assert!(node.size().width <= width + 0.1);
    assert!(node.size().height <= 861.);
    element.as_widget().draw(
        &tree,
        &mut renderer,
        &theme(),
        &renderer::Style::default(),
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &iced::Rectangle::with_size(size),
    );
    assert!(!renderer.layers().is_empty());
}
#[test]
fn production_pages_and_dialogs_draw_without_windows_or_services() {
    let mut app = app();
    for width in [620., 720.] {
        for tab in [Tab::Home, Tab::Accounts, Tab::Settings] {
            app.tab = tab;
            draw(&app, width);
        }
        app.login.open = true;
        for method in [LoginMethod::Qr, LoginMethod::Push, LoginMethod::Password] {
            app.login.method = method;
            draw(&app, width);
        }
        app.login.close();
        app.confirm_delete = Some("test".into());
        draw(&app, width);
        app.confirm_delete = None;
        app.exit_dialog = true;
        draw(&app, width);
        app.exit_dialog = false;
    }
    assert!(app.launcher.is_none());
    assert!(app.rx.try_recv().is_err());
}
#[test]
fn drafts_and_operation_lifetime_are_root_owned() {
    let mut app = app();
    app.draft.game = "/unsaved".into();
    let generation = app.begin_operation(Operation::Updating).unwrap();
    let _ = app.update(Msg::Page(Tab::Settings));
    let _ = app.update(Msg::Page(Tab::Accounts));
    assert_eq!(app.draft.game, "/unsaved");
    assert_eq!(app.operation, Operation::Updating);
    assert!(app.begin_operation(Operation::Launching).is_none());
    app.accept(Event::Updated(generation - 1, Ok(0)));
    assert_eq!(app.operation, Operation::Updating);
}
#[test]
fn cancelled_login_rejects_late_qr_push_and_result() {
    let mut app = app();
    app.login.open = true;
    let generation = app.login.task.generation;
    app.login.password = "secret".into();
    let _ = app.update(Msg::CloseLogin);
    for event in [
        Event::Qr(generation, Vec::new()),
        Event::Push(generation, Some("1234".into())),
        Event::Login(generation, Err("stale".into())),
    ] {
        app.accept(event);
    }
    assert!(app.login.error.is_none());
    assert!(app.login.qr.is_none());
    assert!(app.login.serial.is_none());
    assert!(app.login.password.is_empty());
}
#[test]
fn login_save_failure_keeps_dialog_and_authenticated_result() {
    let mut app = app();
    app.login.open = true;
    app.login.phase = LoginPhase::Saving;
    let token = eorzea::launcher::LaunchToken {
        snda_id: "test".into(),
        username: None,
        auto_login_session_key: Some("private-key".into()),
        ticket: "ticket".into(),
        auto_login_max_age: None,
    };
    app.login.pending_token = Some(token);
    app.auth_busy = true;
    app.accept(Event::AuthSaved {
        login: Some(app.login.task.generation),
        selected: None,
        result: Err("read-only".into()),
    });
    assert!(app.login.open);
    assert!(app.login.pending_token.is_some());
    assert!(app.auth.accounts.is_empty());
    assert_eq!(app.login.phase, LoginPhase::Failed);
    draw(&app, 720.);
    let generation = app.login.task.generation;
    app.accept(Event::AuthSaved {
        login: Some(generation),
        selected: None,
        result: Ok(AuthConfig::default()),
    });
    assert!(!app.login.open);
    assert!(app.login.pending_token.is_none());
}
#[test]
fn settings_save_failure_keeps_draft_and_in_memory_config() {
    let mut app = app();
    let before = app.config.clone();
    app.draft.game = "/new-game".into();
    app.settings_busy = true;
    app.accept(Event::SettingsSaved(Err("permission denied".into())));
    assert_eq!(app.config, before);
    assert_eq!(app.draft.game, "/new-game");
    assert!(app.settings_error.is_some());
    let _ = app.update(Msg::Restore);
    assert_eq!(app.draft, Draft::new(&before));
}
#[test]
fn close_waits_for_critical_work_and_does_not_discard_dirty_draft() {
    let mut app = app();
    app.operation = Operation::Updating;
    let _ = app.update(Msg::CloseRequested);
    assert!(app.exit_dialog);
    let _ = app.update(Msg::ConfirmExit);
    assert!(app.exit_when_idle);
    assert_eq!(app.operation, Operation::Updating);
    app.draft.game = "unsaved".into();
    app.operation = Operation::Idle;
    let _ = app.update(Msg::Wake);
    assert!(app.exit_dialog);
    assert!(!app.exit_when_idle);
    let _ = app.update(Msg::CancelExit);
    assert!(!app.exit_dialog);
}
#[test]
fn launch_progress_is_coalesced_and_cannot_overwrite_terminal_result() {
    let mut app = app();
    app.operation = Operation::Launching;
    app.operation_generation = 8;
    for i in 0..100 {
        app.bus
            .send(Event::LaunchStatus(8, format!("progress {i}")));
    }
    assert!(app.rx.try_recv().is_err());
    app.drain();
    assert_eq!(app.status, "progress 99");
    app.bus.send(Event::LaunchStatus(8, "old progress".into()));
    app.bus.send(Event::Launched(8, Ok(123)));
    app.drain();
    assert_eq!(app.status, "游戏已启动（PID 123）");
    assert_eq!(app.operation, Operation::Idle);
}
#[test]
fn notification_is_stored_when_ui_has_not_started_waiting() {
    let app = app();
    app.bus.send(Event::LaunchStatus(1, "latest".into()));
    assert!(
        app.runtime
            .block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_millis(50),
                    app.bus.wake.notified(),
                )
                .await
            })
            .is_ok()
    );
}
#[test]
fn latest_progress_snapshot_keeps_install_and_download_separate() {
    let mut app = app();
    app.operation_generation = 4;
    let progress = UpdateProgress {
        install: eorzea::game_files::InstallProgress {
            done: 150,
            total: 400,
            completed: 1,
            total_patches: 4,
            current: Some(eorzea::game_files::InstallPatchProgress {
                index: 1,
                version: "install-version".into(),
                phase: eorzea::game_files::InstallPhase::Applying,
                done: 50,
                total: 100,
            }),
        },
        download: eorzea::game_files::patch_manager::DownloadProgress {
            done: 300,
            total: 400,
            completed: 2,
            skipped: 0,
            active: vec![eorzea::game_files::patch_manager::PatchProgress {
                index: 2,
                version: "download-version".into(),
                url: String::new(),
                done: 50,
                total: 100,
                resumed_from: 0,
                phase: eorzea::game_files::patch_manager::PatchPhase::Verifying,
            }],
        },
        ready: Vec::new(),
        waiting_for_download: false,
    };
    *app.bus.progress.lock().unwrap() = Some((3, progress.clone()));
    app.drain();
    assert!(app.update_progress.is_none());
    *app.bus.progress.lock().unwrap() = Some((4, progress));
    app.drain();
    let rows = home::progress_rows(app.update_progress.as_ref().unwrap());
    assert_eq!(rows[0].1, 150);
    assert!(
        rows.iter()
            .any(|(label, done, _)| label == "补丁下载" && *done == 300)
    );
    assert!(
        rows.iter()
            .any(|(label, _, _)| label.contains("download-version") && label.contains("校验中"))
    );
    draw(&app, 720.);
}
#[test]
fn malformed_and_valid_qr_decode_without_gpu() {
    assert!(decode_qr(b"not png").is_err());
    let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let mut app = app();
    app.login.open = true;
    let generation = app.login.task.generation;
    app.accept(Event::Qr(generation, png.into_inner()));
    assert!(app.login.qr.is_some());
    assert_eq!(app.login.phase, LoginPhase::Waiting);
    draw(&app, 720.);
}
#[test]
fn real_secure_input_accepts_synthetic_keyboard_event_without_window() {
    use iced::advanced::{Shell, text};
    let renderer = iced::Renderer::new(iced::Font::DEFAULT, 14.into());
    let mut element = input("password", "", "", true);
    let mut tree = Tree::new(element.as_widget());
    let node = element.as_widget_mut().layout(
        &mut tree,
        &renderer,
        &layout::Limits::new(iced::Size::ZERO, iced::Size::new(300., 50.)),
    );
    tree.state.downcast_mut::<iced::widget::text_input::State<<iced::Renderer as text::Renderer>::Paragraph>>().focus();
    let key = iced::keyboard::Key::Character("a".into());
    let event = iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key: key,
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyA),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::default(),
        text: Some("a".into()),
        repeat: false,
    });
    let mut messages = Vec::new();
    element.as_widget_mut().update(
        &mut tree,
        &event,
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &renderer,
        &mut iced::advanced::clipboard::Null,
        &mut Shell::new(&mut messages),
        &iced::Rectangle::with_size(iced::Size::new(300., 50.)),
    );
    assert!(
        messages
            .iter()
            .any(|m| matches!(m,Msg::Field("password",value) if value=="a"))
    );
    assert!(!format!("{:?}", Msg::Field("password", "secret".into())).contains("secret"));
}
#[test]
fn saving_and_modal_state_reject_background_changes() {
    let mut app = app();
    app.settings_busy = true;
    let before = app.draft.clone();
    let _ = app.update(Msg::Field("game", "discarded".into()));
    let _ = app.update(Msg::Toggle("dalamud", true));
    let _ = app.update(Msg::Segment("wine", 2));
    assert_eq!(app.draft, before);
    app.settings_busy = false;
    app.login.open = true;
    let tab = app.tab;
    let _ = app.update(Msg::Page(Tab::Settings));
    assert_eq!(app.tab, tab);
    let _ = app.update(Msg::Field("password", "secret".into()));
    assert_eq!(app.login.password, "secret");
    let _ = app.update(Msg::Segment("method", 1));
    assert!(app.login.password.is_empty());
}

#[test]
fn global_log_drawer_survives_page_changes_and_does_not_cancel_work() {
    let mut app = app();
    app.operation = Operation::Updating;
    let _ = app.update(Msg::ToggleLogs);
    assert!(app.logs_open);
    for tab in [Tab::Accounts, Tab::Settings, Tab::Home] {
        let _ = app.update(Msg::Page(tab));
        assert!(app.logs_open);
        draw(&app, 620.);
    }
    assert_eq!(app.operation, Operation::Updating);
    app.open_dropdown = Some("area");
    let _ = app.update(Msg::Escape);
    assert!(app.logs_open);
    assert!(app.open_dropdown.is_none());
    let _ = app.update(Msg::Escape);
    assert!(!app.logs_open);
    app.login.open = true;
    let _ = app.update(Msg::ToggleLogs);
    assert!(!app.logs_open);
}
#[test]
fn latest_log_uses_chronological_tail_without_reordering_copy() {
    let mut app = app();
    app.logs = vec![
        crate::logging::LogEntry {
            level: crate::logging::LogLevel::Info,
            text: "first".into(),
        },
        crate::logging::LogEntry {
            level: crate::logging::LogLevel::Error,
            text: "latest\ndetails".into(),
        },
    ];
    assert_eq!(app.latest_log().unwrap().text, "latest\ndetails");
    assert_eq!(
        crate::logging::format_for_copy(&app.logs),
        "first\nlatest\ndetails"
    );
}
#[test]
fn launch_button_label_is_centered_in_both_axes() {
    let renderer = iced::Renderer::new(iced::Font::DEFAULT, 14.into());
    for label in ["启动游戏", "启动中…"] {
        for enabled in [true, false] {
            for width in [200., 520.] {
                let mut element = home::launch_button(label, enabled);
                let mut tree = Tree::new(element.as_widget());
                let node = element.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, 56.)),
                );
                let button = Layout::new(&node);
                let label = button.children().next().unwrap().children().next().unwrap();
                assert!((label.bounds().center().x - button.bounds().center().x).abs() < 0.5);
                assert!((label.bounds().center().y - button.bounds().center().y).abs() < 0.5);
            }
        }
    }
}
#[test]
fn hover_and_pressed_styles_are_visible_for_each_button_role() {
    use iced::widget::button::Status;
    for style in [primary, secondary, quiet, danger] {
        let active = style(&theme(), Status::Active);
        let hovered = style(&theme(), Status::Hovered);
        let pressed = style(&theme(), Status::Pressed);
        assert_ne!(active.background, hovered.background);
        assert_ne!(hovered.background, pressed.background);
        let disabled = style(&theme(), Status::Disabled);
        assert_ne!(disabled.text_color, iced::Color::WHITE);
    }
}
#[test]
fn launch_hover_changes_actual_pixels_and_disabled_buttons_stay_quiet() {
    use iced::advanced::{Renderer as _, Shell};
    let size = iced::Size::new(520., 56.);
    let viewport = iced::Rectangle::with_size(size);
    for enabled in [true, false] {
        let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, 14.into());
        let mut element = home::launch_button("启动游戏", enabled);
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        let mut messages = Vec::new();
        let mut colors = Vec::new();
        for (name, cursor) in [
            ("LaunchIdle", mouse::Cursor::Unavailable),
            (
                "LaunchHover",
                mouse::Cursor::Available(iced::Point::new(10., 28.)),
            ),
        ] {
            if cursor.position().is_some() {
                let mut shell = Shell::new(&mut messages);
                element.as_widget_mut().update(
                    &mut tree,
                    &iced::Event::Mouse(mouse::Event::CursorMoved {
                        position: iced::Point::new(10., 28.),
                    }),
                    Layout::new(&node),
                    cursor,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &viewport,
                );
                if enabled {
                    assert_ne!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
                } else {
                    assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
                }
            }
            element.as_widget_mut().update(
                &mut tree,
                &iced::Event::Window(iced::window::Event::RedrawRequested(
                    iced::time::Instant::now(),
                )),
                Layout::new(&node),
                cursor,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &viewport,
            );
            renderer.reset(viewport);
            element.as_widget().draw(
                &tree,
                &mut renderer,
                &theme(),
                &renderer::Style::default(),
                Layout::new(&node),
                cursor,
                &viewport,
            );
            let mut pixels = tiny_skia::Pixmap::new(520, 56).unwrap();
            let mut mask = tiny_skia::Mask::new(520, 56).unwrap();
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &iced::advanced::graphics::Viewport::with_physical_size(
                    iced::Size::new(520, 56),
                    1.,
                ),
                &[viewport],
                iced::Color::WHITE,
            );
            colors.push(pixels.pixels()[28 * 520 + 10]);
            if enabled {
                if let Some(directory) = std::env::var_os("EORZEA_TEST_RENDER_DIR") {
                    for pixel in pixels.data_mut().chunks_exact_mut(4) {
                        pixel.swap(0, 2);
                    }
                    std::fs::create_dir_all(&directory).unwrap();
                    pixels
                        .save_png(std::path::PathBuf::from(directory).join(format!("{name}.png")))
                        .unwrap();
                }
            }
        }
        if enabled {
            assert_ne!(colors[0], colors[1]);
        } else {
            assert_eq!(colors[0], colors[1]);
        }
        assert!(messages.is_empty(), "hover must never launch the game");
    }
}
#[test]
fn operation_notice_is_independent_of_logs_and_old_expiry() {
    let mut app = app();
    app.operation = Operation::Launching;
    app.operation_generation = 4;
    app.bus.send(Event::Launched(4, Ok(42)));
    let _ = app.update(Msg::Wake);
    let first = app.notice.as_ref().unwrap().0;
    app.logs.push(crate::logging::LogEntry {
        level: crate::logging::LogLevel::Info,
        text: "new unrelated log".into(),
    });
    let _ = app.update(Msg::Wake);
    assert!(app.notice.as_ref().unwrap().1.contains("PID 42"));
    app.bus.send(Event::SettingsSaved(Err("read-only".into())));
    let _ = app.update(Msg::Wake);
    let second = app.notice.as_ref().unwrap().0;
    assert!(second > first);
    let _ = app.update(Msg::NoticeExpired(first));
    assert_eq!(app.notice.as_ref().unwrap().0, second);
    let _ = app.update(Msg::NoticeExpired(second));
    assert!(app.notice.is_none());
    app.bus.send(Event::SettingsSaved(Err("read-only".into())));
    let _ = app.update(Msg::Wake);
    assert!(
        app.notice.is_some(),
        "repeated identical operation results still need feedback"
    );
}

#[test]
fn software_rasterization_draws_pixels_without_compositor() {
    let mut app = app();
    app.draft.dalamud.enabled = true;
    for (name, tab, open) in [
        ("Home", Tab::Home, false),
        ("Accounts", Tab::Accounts, false),
        ("Settings", Tab::Settings, false),
        ("Logs", Tab::Home, true),
        ("SettingsLogs", Tab::Settings, true),
        ("Notice", Tab::Home, false),
    ] {
        app.tab = tab;
        app.logs_open = open;
        app.notice = if name == "Notice" {
            Some((1, "设置已保存".into()))
        } else {
            None
        };
        if open {
            app.logs = vec![
                crate::logging::LogEntry {
                    level: crate::logging::LogLevel::Info,
                    text: "[    0.01s] INFO  已读取本地配置".into(),
                },
                crate::logging::LogEntry {
                    level: crate::logging::LogLevel::Warn,
                    text: "[    0.02s] WARN 尚未配置游戏目录，请在设置中选择游戏根目录".into(),
                },
            ];
        }
        app.log_view.sync(&app.logs);
        let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, 14.into());
        let mut element = app.view();
        let mut tree = Tree::new(element.as_widget());
        let size = iced::Size::new(720., 860.);
        let viewport = iced::Rectangle::with_size(size);
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, size),
        );
        let mut messages = Vec::new();
        element.as_widget_mut().update(
            &mut tree,
            &iced::Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            )),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut iced::advanced::Shell::new(&mut messages),
            &viewport,
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
        let mut pixels = tiny_skia::Pixmap::new(720, 860).unwrap();
        let mut mask = tiny_skia::Mask::new(720, 860).unwrap();
        renderer.draw(
            &mut pixels.as_mut(),
            &mut mask,
            &iced::advanced::graphics::Viewport::with_physical_size(iced::Size::new(720, 860), 1.),
            &[viewport],
            iced::Color::WHITE,
        );
        assert!(
            pixels
                .pixels()
                .iter()
                .any(|pixel| pixel.red() < 100 && pixel.green() < 100)
        );
        if let Some(directory) = std::env::var_os("EORZEA_TEST_RENDER_DIR") {
            // The native tiny-skia framebuffer is BGRA for softbuffer; PNG expects RGBA.
            for pixel in pixels.data_mut().chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
            std::fs::create_dir_all(&directory).unwrap();
            pixels
                .save_png(std::path::PathBuf::from(directory).join(format!("{name}.png")))
                .unwrap();
        }
    }
}

fn drawer_frame(
    element: &mut Element<'_, Msg>,
    renderer: &mut iced::Renderer,
    tree: &mut Tree,
    node: &layout::Node,
    at: iced::time::Instant,
    scale: f32,
    focus_logs: bool,
) -> (tiny_skia::Pixmap, iced::window::RedrawRequest) {
    use iced::advanced::{Renderer as _, Shell, widget::operation};
    let bounds = node.bounds();
    if focus_logs {
        element.as_widget_mut().operate(
            tree,
            Layout::new(node),
            renderer,
            &mut operation::focusable::focus::<()>(iced::widget::Id::new("selectable-logs")),
        );
    }
    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    element.as_widget_mut().update(
        tree,
        &iced::Event::Window(iced::window::Event::RedrawRequested(at)),
        Layout::new(node),
        mouse::Cursor::Unavailable,
        renderer,
        &mut iced::advanced::clipboard::Null,
        &mut shell,
        &bounds,
    );
    let redraw = shell.redraw_request();
    assert!(!shell.is_layout_invalid());
    assert!(!shell.are_widgets_invalid());
    assert!(messages.is_empty());
    renderer.reset(bounds);
    element.as_widget().draw(
        tree,
        renderer,
        &theme(),
        &renderer::Style::default(),
        Layout::new(node),
        mouse::Cursor::Unavailable,
        &bounds,
    );
    let size = iced::Size::new(
        (bounds.width * scale) as u32,
        (bounds.height * scale) as u32,
    );
    let mut pixels = tiny_skia::Pixmap::new(size.width, size.height).unwrap();
    let mut mask = tiny_skia::Mask::new(size.width, size.height).unwrap();
    renderer.draw(
        &mut pixels.as_mut(),
        &mut mask,
        &iced::advanced::graphics::Viewport::with_physical_size(size, scale),
        &[bounds],
        iced::Color::WHITE,
    );
    (pixels, redraw)
}

#[test]
fn drawer_animation_never_paints_text_selection_or_controls_over_footer() {
    use iced::time::{Duration, Instant};
    use iced::widget::text_editor::Action;
    // Keep the same tree, cached layout and renderer throughout closing and reopening.
    // Independent static snapshots cannot exercise translated nested clipping layers.
    for (width, height, scale) in [(620., 620., 1.), (960., 860., 1.5)] {
        let mut app = app();
        app.logs = (0..40)
            .map(|index| crate::logging::LogEntry {
                level: if index % 2 == 0 {
                    crate::logging::LogLevel::Error
                } else {
                    crate::logging::LogLevel::Warn
                },
                text: format!(
                    "日志 {index:02}: selected ERROR/WARN text must stay above the footer"
                ),
            })
            .collect();
        app.log_view.sync(&app.logs);
        app.log_view.action(Action::SelectAll);
        let mut renderer = iced::Renderer::new(iced::Font::DEFAULT, 14.into());
        let mut element = app.view();
        let mut tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, height)),
        );
        let start = Instant::now();
        let (closed, _) = drawer_frame(
            &mut element,
            &mut renderer,
            &mut tree,
            &node,
            start,
            scale,
            false,
        );
        drop(element);
        // A separate initially-open tree supplies a stable reference (including the chevron).
        app.logs_open = true;
        let mut element = app.view();
        let mut opened_tree = Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut opened_tree,
            &renderer,
            &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, height)),
        );
        // New Transition states use the real clock. Start synthetic animation time
        // after creating them, not before rasterizing the closed reference frame.
        let start = Instant::now();
        let (opened, _) = drawer_frame(
            &mut element,
            &mut renderer,
            &mut opened_tree,
            &node,
            start,
            scale,
            true,
        );
        drop(element);
        tree = opened_tree;
        let footer_start = ((height - footer::HEIGHT) * scale) as usize * opened.width() as usize;
        let mut failures = Vec::new();
        for (open, phase, phase_start, offsets) in [
            (
                false,
                "Closing",
                100,
                &[0, 16, 40, 80, 120, 180, 240, 400][..],
            ),
            (
                true,
                "Reopening",
                1000,
                &[0, 16, 40, 80, 120, 180, 240, 400][..],
            ),
            (false, "ReverseClosing", 2000, &[0, 40, 80][..]),
            (
                true,
                "ReverseOpening",
                2080,
                &[0, 16, 40, 80, 120, 240, 400][..],
            ),
        ] {
            app.logs_open = open;
            let mut element = app.view();
            tree.diff(element.as_widget());
            // A new view is laid out once; redraw-only frames keep that widget and layout.
            let node = element.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, height)),
            );
            for &offset in offsets {
                let (pixels, redraw) = drawer_frame(
                    &mut element,
                    &mut renderer,
                    &mut tree,
                    &node,
                    start + Duration::from_millis(phase_start + offset),
                    scale,
                    false,
                );
                let reference = if open { &opened } else { &closed };
                let changed = pixels.pixels()[footer_start..]
                    .iter()
                    .zip(&reference.pixels()[footer_start..])
                    .filter(|(actual, expected)| actual != expected)
                    .count();
                if changed > 0 {
                    failures.push(format!(
                        "{phase} {offset}ms: {changed} footer pixels changed"
                    ));
                }
                if offset >= 240 {
                    assert_eq!(
                        redraw,
                        iced::window::RedrawRequest::Wait,
                        "settled {phase} {offset}ms at {width}px/{scale}x"
                    );
                    assert_eq!(pixels.data(), reference.data(), "settled {phase} frame");
                }
                if let Some(directory) = std::env::var_os("EORZEA_TEST_RENDER_DIR") {
                    let directory = std::path::PathBuf::from(directory);
                    std::fs::create_dir_all(&directory).unwrap();
                    let mut png = pixels.clone();
                    for pixel in png.data_mut().chunks_exact_mut(4) {
                        pixel.swap(0, 2);
                    }
                    png.save_png(directory.join(format!("Drawer{phase}-{width}-{offset}ms.png")))
                        .unwrap();
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
