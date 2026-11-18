// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod build_info;
mod clipboard;
mod logging;
mod ui;
mod updates;

fn main() -> iced::Result {
    if matches!(std::env::args().nth(1).as_deref(), Some("--version" | "-V")) {
        let info = build_info::BuildInfo::current();
        println!("{}\n{}", info.display(), info.distribution.label());
        if let Some(commit) = info.commit {
            println!("commit: {commit}");
        }
        return Ok(());
    }
    logging::init();
    tracing::info!("GUI renderer: Iced / tiny-skia (CPU software rendering)");
    let fonts = ui::theme::system_fonts();
    let mut app = iced::application(ui::App::new, ui::App::update, ui::App::view)
        .executor::<ui::Executor>()
        .title("FFXIV 国服启动器")
        .window(iced::window::Settings {
            size: iced::Size::new(720., 860.),
            min_size: Some(iced::Size::new(620., 600.)),
            exit_on_close_request: false,
            ..Default::default()
        })
        .settings(iced::Settings {
            default_text_size: 14.into(),
            ..Default::default()
        })
        .default_font(fonts.font)
        .theme(ui::theme::theme())
        .subscription(ui::App::subscription);
    for bytes in fonts.bytes {
        app = app.font(bytes);
    }
    app.run()
}
