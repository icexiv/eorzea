//! Production Iced/tiny-skia UI. Root-owned tasks survive page changes.
mod app_update;
mod backend;
mod footer;
mod home;
mod log_view;
mod login;
mod motion;
mod settings;
mod state;
#[cfg(test)]
mod tests;
pub mod theme;

use backend::{Bus, Event};
use eorzea::{
    auth::AuthConfig, config::AppConfig, dalamud::DalamudStatus, game_files::UpdateProgress,
    launcher::Launcher,
};
use eorzea_auth::{PatchListEntry, SdoArea};
use iced::widget::{
    button, column, container, mouse_area, opaque, row, scrollable, space, stack, svg, tooltip,
};
use iced::{Element, Fill, Subscription, Task};
use state::{Draft, Login, LoginMethod, LoginPhase, Operation, Tab};
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use theme::*;

static RUNTIME: LazyLock<Arc<tokio::runtime::Runtime>> = LazyLock::new(|| {
    Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("GUI async runtime"),
    )
});
/// Iced subscriptions and production workers share just two Tokio workers.
pub struct Executor(Arc<tokio::runtime::Runtime>);
impl iced::Executor for Executor {
    fn new() -> Result<Self, std::io::Error> {
        Ok(Self(RUNTIME.clone()))
    }
    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        self.0.spawn(future);
    }
    fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.enter();
        f()
    }
    fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        self.0.block_on(future)
    }
}
#[derive(Clone)]
struct Wake(Arc<tokio::sync::Notify>);
impl std::hash::Hash for Wake {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        std::ptr::hash(Arc::as_ptr(&self.0), h);
    }
}

#[derive(Clone)]
pub enum Msg {
    Wake,
    Page(Tab),
    Drop(&'static str),
    Select(&'static str, String),
    Dismiss,
    Field(&'static str, String),
    Toggle(&'static str, bool),
    Segment(&'static str, usize),
    AddAccount,
    CloseLogin,
    Login,
    CancelLogin,
    RetrySave,
    Default(String),
    Delete(String),
    ConfirmDelete,
    Check,
    Update,
    Launch,
    Copy,
    LogAction(iced::widget::text_editor::Action),
    FollowLogs,
    ToggleLogs,
    DismissNotice,
    NoticeExpired(u64),
    Save,
    Restore,
    Browse,
    RetryAreas,
    CheckLauncher,
    ShowLauncherUpdate,
    ReleaseNotes,
    NixGuide,
    CloseRequested,
    ConfirmExit,
    CancelExit,
    Escape,
    TabKey(bool),
}
// Never expose password-bearing input messages in framework diagnostics.
impl std::fmt::Debug for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UI message (values redacted)")
    }
}

pub struct App {
    runtime: Arc<tokio::runtime::Runtime>,
    bus: Bus,
    rx: mpsc::Receiver<Event>,
    config: AppConfig,
    auth: AuthConfig,
    draft: Draft,
    loaded: bool,
    build_info: crate::build_info::BuildInfo,
    launcher_update: app_update::State,
    launcher: Option<Arc<Launcher>>,
    tab: Tab,
    areas: Vec<SdoArea>,
    area_loading: bool,
    area_error: Option<String>,
    selected_account: Option<String>,
    selected_area: Option<String>,
    status: String,
    operation: Operation,
    operation_generation: u64,
    patches: Vec<PatchListEntry>,
    update_message: String,
    update_error: bool,
    update_progress: Option<UpdateProgress>,
    login: Login,
    confirm_delete: Option<String>,
    auth_busy: bool,
    settings_busy: bool,
    folder_busy: bool,
    path_generation: u64,
    draft_game_found: Option<bool>,
    settings_error: Option<String>,
    /// 主页途径切换的写入中标记：避免重复提交，失败时恢复旧值。
    launch_method_pending: Option<(
        eorzea::config::SdoLaunchMethod,
        eorzea::config::SdoLaunchMethod,
    )>,
    /// 主页 Dalamud 开关写入中标记：保存失败时恢复旧值。
    dalamud_pending: Option<bool>,
    inspection_generation: u64,
    local: backend::LocalSummary,
    dalamud_loading: bool,
    dalamud: Option<DalamudStatus>,
    logs: Vec<crate::logging::LogEntry>,
    log_view: log_view::LogView,
    log_seq: u64,
    logs_open: bool,
    notice: Option<(u64, String)>,
    notice_generation: u64,
    notice_pending: bool,
    open_dropdown: Option<&'static str>,
    exit_dialog: bool,
    exit_when_idle: bool,
}
impl App {
    pub fn new() -> Self {
        let app = Self::empty(RUNTIME.clone());
        crate::logging::set_wakeup(app.bus.wake.clone());
        backend::initialize(&app.runtime, app.bus.clone());
        app
    }
    fn empty(runtime: Arc<tokio::runtime::Runtime>) -> Self {
        let (tx, rx) = mpsc::channel();
        let bus = Bus {
            tx,
            progress: Arc::new(Mutex::new(None)),
            launch_status: Arc::new(Mutex::new(None)),
            wake: Arc::new(tokio::sync::Notify::new()),
        };
        let config = AppConfig::default();
        let draft = Draft::new(&config);
        Self {
            runtime,
            bus,
            rx,
            config,
            auth: AuthConfig::default(),
            draft,
            loaded: false,
            build_info: crate::build_info::BuildInfo::current(),
            launcher_update: app_update::State::default(),
            launcher: None,
            tab: Tab::Home,
            areas: Vec::new(),
            area_loading: true,
            area_error: None,
            selected_account: None,
            selected_area: None,
            status: "正在初始化…".into(),
            operation: Operation::Idle,
            operation_generation: 0,
            patches: Vec::new(),
            update_message: String::new(),
            update_error: false,
            update_progress: None,
            login: Login::default(),
            confirm_delete: None,
            auth_busy: false,
            settings_busy: false,
            folder_busy: false,
            path_generation: 0,
            draft_game_found: None,
            settings_error: None,
            launch_method_pending: None,
            dalamud_pending: None,
            inspection_generation: 0,
            local: Default::default(),
            dalamud_loading: false,
            dalamud: None,
            logs: Vec::new(),
            log_view: log_view::LogView::default(),
            log_seq: 0,
            logs_open: false,
            notice: None,
            notice_generation: 0,
            notice_pending: false,
            open_dropdown: None,
            exit_dialog: false,
            exit_when_idle: false,
        }
    }
    pub fn subscription(&self) -> Subscription<Msg> {
        Subscription::batch([
            Subscription::run_with(Wake(self.bus.wake.clone()), |wake| {
                iced::futures::stream::unfold(wake.clone(), |wake| async move {
                    wake.0.notified().await;
                    Some((Msg::Wake, wake))
                })
            }),
            iced::event::listen_with(|event, status, _| match event {
                iced::Event::Window(iced::window::Event::CloseRequested) => {
                    Some(Msg::CloseRequested)
                }
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                    ..
                }) => Some(Msg::Escape),
                iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                    key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Tab),
                    modifiers,
                    ..
                }) if status == iced::event::Status::Ignored => {
                    Some(Msg::TabKey(modifiers.shift()))
                }
                _ => None,
            }),
        ])
    }
    fn critical_busy(&self) -> bool {
        self.operation.busy() || self.auth_busy || self.settings_busy
    }
    fn modal_open(&self) -> bool {
        self.login.open || self.confirm_delete.is_some() || self.exit_dialog
    }
    fn refresh_summary(&mut self) {
        self.inspection_generation += 1;
        let load_dalamud = self.config.launch.method == eorzea::config::SdoLaunchMethod::Injector
            && self.config.dalamud.enabled;
        self.dalamud_loading = load_dalamud;
        self.dalamud = None;
        backend::inspect(
            &self.runtime,
            self.bus.clone(),
            self.inspection_generation,
            self.config.clone(),
            load_dalamud,
        );
    }
    fn choose_account(&mut self, id: String) {
        let area = self
            .auth
            .find(&id)
            .and_then(|a| a.area.clone())
            .or_else(|| self.config.area.clone());
        self.selected_account = Some(id);
        self.select_area_value(area.as_deref());
    }
    fn select_area_value(&mut self, value: Option<&str>) {
        self.selected_area = value
            .and_then(|v| {
                self.areas
                    .iter()
                    .find(|a| a.area_id == v || a.area_name == v)
            })
            .or_else(|| self.areas.first())
            .map(|a| a.area_id.clone());
    }
    fn selected_area(&self) -> Option<SdoArea> {
        self.areas
            .iter()
            .find(|a| Some(a.area_id.as_str()) == self.selected_area.as_deref())
            .cloned()
    }
    fn drain(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            self.accept(event);
        }
        let progress = self
            .bus
            .progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some((generation, progress)) = progress {
            if generation == self.operation_generation {
                if self.config.game_path.is_some()
                    && self.update_progress.as_ref().map(|p| p.install.completed)
                        != Some(progress.install.completed)
                {
                    backend::refresh_local(
                        &self.runtime,
                        self.bus.clone(),
                        self.inspection_generation,
                        self.config.clone(),
                    );
                }
                self.update_progress = Some(progress);
            }
        }
        let status = self
            .bus
            .launch_status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some((generation, status)) = status {
            if generation == self.operation_generation && self.operation == Operation::Launching {
                self.status = status;
            }
        }
        let (entries, seq) = crate::logging::snapshot_since(self.log_seq);
        self.log_seq = seq;
        let changed = !entries.is_empty();
        self.logs.extend(entries);
        let overflow = self.logs.len().saturating_sub(crate::logging::MAX_UI_LINES);
        self.logs.drain(..overflow);
        if changed {
            self.log_view.sync(&self.logs);
        }
    }
    fn accept(&mut self, event: Event) {
        match event {
            Event::Config(config, auth) => {
                self.tab = if auth.accounts.is_empty() {
                    Tab::Accounts
                } else if config.game_path.is_none() {
                    Tab::Settings
                } else {
                    Tab::Home
                };
                self.selected_account = state::initial_account(&auth);
                self.auth = auth;
                self.draft = Draft::new(&config);
                self.config = config;
                self.loaded = true;
                self.status = "准备就绪".into();
                self.refresh_summary();
                if self.config.launcher_update.check_on_startup {
                    self.start_launcher_check(false);
                }
            }
            Event::Launcher(result) => match result {
                Ok(launcher) => {
                    self.launcher = Some(launcher);
                    if self.login.open
                        && self.login.method == LoginMethod::Qr
                        && self.login.phase == LoginPhase::Idle
                    {
                        self.start_login();
                    }
                }
                Err(e) => self.status = format!("初始化失败: {e}"),
            },
            Event::Areas(result) => {
                self.area_loading = false;
                match result {
                    Ok(areas) => {
                        self.area_error = None;
                        self.areas = areas;
                        let value = self
                            .selected_account
                            .as_deref()
                            .and_then(|id| self.auth.find(id))
                            .and_then(|a| a.area.clone())
                            .or_else(|| self.config.area.clone());
                        self.select_area_value(value.as_deref());
                    }
                    Err(e) => {
                        self.area_error = Some(e.clone());
                        self.status = format!("获取大区列表失败: {e}");
                    }
                }
            }
            Event::Local(generation, local) if generation == self.inspection_generation => {
                self.local = local
            }
            Event::Dalamud(generation, status) if generation == self.inspection_generation => {
                self.dalamud_loading = false;
                self.dalamud = status;
            }
            Event::Qr(generation, bytes)
                if self.login.open && self.login.task.accepts(generation) =>
            {
                match decode_qr(&bytes) {
                    Ok(qr) => {
                        self.login.qr = Some(qr);
                        self.login.phase = LoginPhase::Waiting;
                    }
                    Err(e) => {
                        self.login.task.cancel();
                        self.login.phase = LoginPhase::Failed;
                        self.login.error = Some(e);
                    }
                }
            }
            Event::Push(generation, serial)
                if self.login.open && self.login.task.accepts(generation) =>
            {
                self.login.serial = serial;
                self.login.phase = LoginPhase::Waiting;
            }
            Event::Login(generation, result)
                if self.login.open && self.login.task.accepts(generation) =>
            {
                match result {
                    Ok(token) => {
                        self.login.pending_token = Some(token);
                        self.persist_login();
                    }
                    Err(e) => {
                        self.login.phase = LoginPhase::Failed;
                        self.login.error = Some(e);
                    }
                }
            }
            Event::AuthSaved {
                login,
                selected,
                result,
            } => {
                self.notice_pending = true;
                self.auth_busy = false;
                match result {
                    Ok(auth) => {
                        self.auth = auth;
                        if let Some(id) = selected {
                            self.choose_account(id);
                        } else if self
                            .selected_account
                            .as_ref()
                            .is_some_and(|id| self.auth.find(id).is_none())
                        {
                            if let Some(id) = state::initial_account(&self.auth) {
                                self.choose_account(id);
                            } else {
                                self.selected_account = None;
                            }
                        }
                        self.status = "账号配置已保存".into();
                        if login.is_some_and(|g| self.login.task.accepts(g)) {
                            self.login.close();
                        }
                    }
                    Err(e) => {
                        self.status = e.clone();
                        if login.is_some_and(|g| self.login.task.accepts(g)) {
                            self.login.phase = LoginPhase::Failed;
                            self.login.error = Some(e);
                        }
                    }
                }
            }
            Event::SettingsSaved(result) => {
                self.notice_pending = true;
                self.settings_busy = false;
                match result {
                    Ok(config) => {
                        let update_changed = self.config.launcher_update != config.launcher_update;
                        self.config = config;
                        self.draft = Draft::new(&self.config);
                        self.settings_error = None;
                        self.status = "设置已保存".into();
                        self.patches.clear();
                        self.update_progress = None;
                        self.update_message.clear();
                        self.refresh_summary();
                        if update_changed {
                            self.launcher_update = app_update::State {
                                task: std::mem::take(&mut self.launcher_update.task),
                                ..Default::default()
                            };
                            self.launcher_update.task.cancel();
                            if self.config.launcher_update.check_on_startup {
                                self.start_launcher_check(false);
                            }
                        }
                    }
                    Err(e) => {
                        self.settings_error = Some(e.clone());
                        self.status = format!("保存设置失败: {e}");
                    }
                }
            }
            Event::LaunchMethodSaved(result) => {
                match result {
                    Ok(config) => {
                        let (requested, previous) = self
                            .launch_method_pending
                            .take()
                            .unwrap_or((config.launch.method, self.config.launch.method));
                        let load_before = previous.loads_dalamud() && self.config.dalamud.enabled;
                        let load_after =
                            config.launch.method.loads_dalamud() && self.config.dalamud.enabled;
                        self.config.launch.method = config.launch.method;
                        self.draft.launch.method = config.launch.method;
                        self.settings_error = None;
                        if requested != config.launch.method {
                            self.status = format!(
                                "启动途径仍为 {}（配置文件被其他程序修改）",
                                config.launch.method.label()
                            );
                        }
                        // 只有“是否真的加载 Dalamud”变化时才重新探测，避免整页闪一下。
                        if load_before != load_after {
                            self.refresh_summary();
                        }
                    }
                    Err(e) => {
                        if let Some((_, previous)) = self.launch_method_pending.take() {
                            self.config.launch.method = previous;
                            self.draft.launch.method = previous;
                        }
                        self.notice_pending = true;
                        self.settings_error = Some(e.clone());
                        self.status = format!("切换启动途径失败: {e}");
                    }
                }
            }
            Event::DalamudEnabledSaved(result) => match result {
                Ok(config) => {
                    let previous = self.dalamud_pending.take();
                    self.config.dalamud.enabled = config.dalamud.enabled;
                    self.draft.dalamud.enabled = config.dalamud.enabled;
                    self.settings_error = None;
                    if previous.is_some_and(|previous| previous != config.dalamud.enabled)
                        && self.config.launch.method.loads_dalamud()
                    {
                        self.refresh_summary();
                    }
                }
                Err(e) => {
                    if let Some(previous) = self.dalamud_pending.take() {
                        self.config.dalamud.enabled = previous;
                        self.draft.dalamud.enabled = previous;
                    }
                    self.notice_pending = true;
                    self.settings_error = Some(e.clone());
                    self.status = format!("保存 Dalamud 开关失败: {e}");
                }
            },
            Event::Folder(path) => {
                self.folder_busy = false;
                if let Some(path) = path {
                    self.change_game_path(path.to_string_lossy().into_owned());
                }
            }
            Event::GamePath(generation, found) if generation == self.path_generation => {
                self.draft_game_found = Some(found);
            }
            Event::Checked(generation, result) if generation == self.operation_generation => {
                self.operation = Operation::Idle;
                match result {
                    Ok((patches, pending)) => {
                        self.update_message = if patches.is_empty() {
                            "游戏已是最新。".into()
                        } else if pending {
                            "发现未完成更新，点击更新游戏恢复原队列".into()
                        } else {
                            format!("发现 {} 个补丁，点击更新游戏开始下载。", patches.len())
                        };
                        self.patches = patches;
                    }
                    Err(e) => {
                        self.update_error = true;
                        self.update_message = e;
                    }
                }
            }
            Event::Updated(generation, result) if generation == self.operation_generation => {
                self.operation = Operation::Idle;
                match result {
                    Ok(count) => {
                        self.patches.clear();
                        self.update_message = format!("更新完成（应用并清理 {count} 个补丁）");
                    }
                    Err(e) => {
                        self.update_error = true;
                        self.update_message =
                            format!("更新失败: {e}。队列已保留，重新检查更新可恢复。");
                    }
                }
                self.refresh_summary();
            }
            Event::LaunchStatus(generation, status)
                if generation == self.operation_generation
                    && self.operation == Operation::Launching =>
            {
                self.status = status
            }
            Event::AuthReload(auth) => self.auth = auth,
            Event::Launched(generation, result) if generation == self.operation_generation => {
                self.operation = Operation::Idle;
                self.status = match result {
                    Ok(pid) => {
                        tracing::info!(pid, "游戏进程已启动");
                        format!("游戏已启动（PID {pid}）")
                    }
                    Err(e) => {
                        tracing::error!(error=%e,"游戏启动失败");
                        format!("启动失败: {e}")
                    }
                };
            }
            Event::LauncherChecked(generation, result)
                if self.launcher_update.task.accepts(generation) =>
            {
                self.launcher_update.checking = false;
                self.launcher_update.task.handle = None;
                if let Err(error) = &result {
                    tracing::debug!(%error, "启动器更新检查失败，不影响游戏启动");
                }
                self.launcher_update.result = Some(result);
            }
            Event::LinkOpened(result) => {
                self.launcher_update.link_error = result.err();
            }
            _ => {}
        }
    }
    pub fn update(&mut self, msg: Msg) -> Task<Msg> {
        let previous_status = self.status.clone();
        let was_loaded = self.loaded;
        if !matches!(
            msg,
            Msg::Drop(_) | Msg::Wake | Msg::TabKey(_) | Msg::Escape | Msg::NoticeExpired(_)
        ) {
            self.open_dropdown = None;
        }
        match msg {
            Msg::Wake => self.drain(),
            Msg::Page(tab) if !self.modal_open() => {
                self.tab = tab;
                self.open_dropdown = None;
            }
            Msg::Drop(id) => {
                self.open_dropdown = if self.open_dropdown == Some(id) {
                    None
                } else {
                    Some(id)
                };
            }
            Msg::Dismiss => self.open_dropdown = None,
            Msg::Select(id, value) if !self.critical_busy() => {
                self.open_dropdown = None;
                match id {
                    "account" => self.choose_account(value),
                    "area" => {
                        if let Some(account) = self.selected_account.clone() {
                            if let Some(area) = self.areas.iter().find(|a| a.area_id == value) {
                                self.persist_auth(
                                    backend::AuthChange::Area(
                                        account.clone(),
                                        area.area_name.clone(),
                                    ),
                                    None,
                                    Some(account),
                                );
                            }
                        } else {
                            self.selected_area = Some(value);
                        }
                    }
                    "track" => self.draft.dalamud.track = value,
                    _ => {}
                }
            }
            Msg::Field(id, value) => self.set_field(id, value),
            Msg::Toggle(id, value) => self.set_toggle(id, value),
            Msg::Segment(id, n) => self.set_segment(id, n),
            Msg::AddAccount if !self.critical_busy() => {
                self.login.reset();
                self.login.open = true;
                self.login.method = LoginMethod::Qr;
                self.open_dropdown = None;
                if self.launcher.is_some() {
                    self.start_login();
                }
            }
            Msg::CloseLogin if self.login.phase != LoginPhase::Saving => self.login.close(),
            Msg::Login if self.login.open && self.login.phase != LoginPhase::Saving => {
                self.start_login()
            }
            Msg::CancelLogin if self.login.phase != LoginPhase::Saving => self.login.reset(),
            Msg::RetrySave => self.persist_login(),
            Msg::Default(id) => {
                self.persist_auth(backend::AuthChange::Default(id.clone()), None, Some(id))
            }
            Msg::Delete(id) if !self.critical_busy() => self.confirm_delete = Some(id),
            Msg::ConfirmDelete if !self.critical_busy() => {
                if let Some(id) = self.confirm_delete.take() {
                    self.persist_auth(backend::AuthChange::Delete(id), None, None);
                }
            }
            Msg::Check => self.check_update(),
            Msg::Update => self.run_update(),
            Msg::Launch => self.launch_game(),
            Msg::CheckLauncher if !self.modal_open() => self.start_launcher_check(true),
            Msg::ShowLauncherUpdate if !self.modal_open() => self.tab = Tab::Settings,
            Msg::ReleaseNotes if !self.modal_open() => {
                if let Some(Ok(result)) = &self.launcher_update.result {
                    if let Some(release) = &result.release {
                        backend::open_link(&self.runtime, self.bus.clone(), release.notes_url());
                    }
                }
            }
            Msg::NixGuide
                if !self.modal_open()
                    && self.build_info.distribution == crate::build_info::Distribution::Nix =>
            {
                backend::open_link(
                    &self.runtime,
                    self.bus.clone(),
                    crate::updates::NIX_GUIDE.into(),
                );
            }
            Msg::Copy => {
                self.notice_pending = true;
                let text = crate::logging::format_for_copy(&self.logs);
                self.status = if text.is_empty() {
                    "暂无可复制的日志".into()
                } else {
                    match crate::clipboard::copy_text(text) {
                        Ok(()) => "日志已复制（按时间从旧到新排列）".into(),
                        Err(e) => format!("复制日志失败: {e}"),
                    }
                };
            }
            Msg::LogAction(action) if self.logs_open && !self.modal_open() => {
                self.log_view.action(action)
            }
            Msg::FollowLogs if !self.modal_open() => self.log_view.follow(&self.logs),
            Msg::ToggleLogs if !self.modal_open() => self.logs_open = !self.logs_open,
            Msg::DismissNotice => self.notice = None,
            Msg::NoticeExpired(generation)
                if self
                    .notice
                    .as_ref()
                    .is_some_and(|(id, _)| *id == generation) =>
            {
                self.notice = None
            }
            Msg::Save if self.loaded && !self.critical_busy() && !self.folder_busy => {
                match self.draft.apply(&self.config) {
                    Ok(_) => {
                        self.settings_busy = true;
                        self.settings_error = None;
                        backend::save_settings(&self.runtime, self.bus.clone(), self.draft.clone());
                    }
                    Err(e) => self.settings_error = Some(e),
                }
            }
            Msg::Restore if !self.settings_busy && !self.folder_busy => {
                self.draft = Draft::new(&self.config);
                self.settings_error = None;
            }
            Msg::Browse if !self.settings_busy && !self.folder_busy => {
                self.folder_busy = true;
                let bus = self.bus.clone();
                self.runtime.spawn(async move {
                    let path = rfd::AsyncFileDialog::new()
                        .set_title("选择游戏根目录")
                        .pick_folder()
                        .await
                        .map(|f| f.path().to_path_buf());
                    bus.send(Event::Folder(path));
                });
            }
            Msg::RetryAreas if !self.area_loading => {
                self.area_loading = true;
                self.area_error = None;
                self.runtime.spawn(backend::fetch_areas(self.bus.clone()));
            }
            Msg::CloseRequested => {
                if self.critical_busy() || self.draft.dirty(&self.config) {
                    self.exit_dialog = true;
                } else {
                    return iced::exit();
                }
            }
            Msg::ConfirmExit => {
                self.exit_dialog = false;
                self.exit_when_idle = true;
                if !self.critical_busy() {
                    return iced::exit();
                }
            }
            Msg::CancelExit => {
                self.exit_dialog = false;
                self.exit_when_idle = false;
            }
            Msg::Escape => {
                if self.exit_dialog {
                    self.exit_dialog = false;
                    self.exit_when_idle = false;
                } else if self.confirm_delete.is_some() {
                    self.confirm_delete = None;
                } else if self.login.open && self.login.phase != LoginPhase::Saving {
                    self.login.close();
                } else {
                    if self.open_dropdown.take().is_none() && self.logs_open {
                        self.logs_open = false;
                    }
                }
            }
            Msg::TabKey(reverse) => {
                return if reverse {
                    iced::widget::operation::focus_previous()
                } else {
                    iced::widget::operation::focus_next()
                };
            }
            _ => {}
        }
        if self.exit_when_idle && !self.critical_busy() {
            if self.draft.dirty(&self.config) {
                self.exit_when_idle = false;
                self.exit_dialog = true;
            } else {
                return iced::exit();
            }
        }
        let force_notice = std::mem::take(&mut self.notice_pending);
        if was_loaded
            && self.loaded
            && (self.status != previous_status || force_notice)
            && !self.operation.busy()
        {
            self.notice_generation += 1;
            let generation = self.notice_generation;
            self.notice = Some((generation, self.status.clone()));
            Task::perform(
                async {
                    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
                },
                move |_| Msg::NoticeExpired(generation),
            )
        } else {
            Task::none()
        }
    }
    pub fn view(&self) -> Element<'_, Msg> {
        let sidebar = container(
            column![
                self.nav(Tab::Home, 0),
                self.nav(Tab::Accounts, 1),
                space().height(Fill),
                self.nav(Tab::Settings, 2)
            ]
            .spacing(8),
        )
        .padding([16, 10])
        .width(71)
        .height(Fill);
        let page = if !self.loaded {
            text("正在读取配置…").color(MUTED).into()
        } else {
            match self.tab {
                Tab::Home => self.home(),
                Tab::Accounts => self.accounts_page(),
                Tab::Settings => self.settings_page(),
            }
        };
        let mut right =
            column![scrollable(container(page).padding([24, 28]).width(Fill)).height(Fill)];
        if self.tab == Tab::Settings {
            right = right.push(self.save_bar());
        }
        let main = mouse_area(
            row![
                sidebar,
                container(space()).width(1).height(Fill).style(divider),
                right.width(Fill).height(Fill)
            ]
            .height(Fill),
        )
        .on_press(Msg::Dismiss);
        let drawer = self.logs_panel();
        let notice = container(opaque(self.notice_view()))
            .padding(16)
            .align_right(Fill)
            .align_bottom(Fill);
        let content = stack![
            main,
            container(drawer).width(Fill).align_bottom(Fill),
            notice
        ];
        let base: Element<'_, Msg> = column![content.height(Fill), self.footer()]
            .height(Fill)
            .into();
        let body = if self.exit_dialog {
            Some(self.exit_view())
        } else if self.confirm_delete.is_some() {
            Some(self.delete_view())
        } else if self.login.open {
            Some(self.login_view())
        } else {
            None
        };
        if let Some(body) = body {
            stack![
                base,
                opaque(
                    container(body)
                        .padding(16)
                        .center_x(Fill)
                        .center_y(Fill)
                        .style(|_| container::Style {
                            background: Some(iced::Color::from_rgba(0., 0., 0., 0.35).into()),
                            ..Default::default()
                        })
                )
            ]
            .into()
        } else {
            base
        }
    }
    fn nav(&self, tab: Tab, index: usize) -> Element<'_, Msg> {
        const PATHS: [&str; 3] = [
            "m3 10 9-7 9 7v10a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1Z",
            "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2M22 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75M13 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0Z",
            "m9 3-1 3-3 1-2 3 2 2-2 2 2 3 3 1 1 3h6l1-3 3-1 2-3-2-2 2-2-2-3-3-1-1-3ZM15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z",
        ];
        let selected = self.tab == tab;
        let color = if selected { "#09090b" } else { "#71717a" };
        let icon=svg(svg::Handle::from_memory(format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='{color}' stroke-width='1.75' stroke-linecap='round' stroke-linejoin='round'><path d='{}'/></svg>",PATHS[index]).into_bytes())).width(22).height(22);
        let navigation = button(container(icon).center_x(Fill).center_y(Fill))
            .width(52)
            .height(48)
            .padding(0)
            .on_press_maybe((!self.modal_open()).then_some(Msg::Page(tab)))
            .style(move |_, s| iced::widget::button::Style {
                background: Some(
                    match s {
                        iced::widget::button::Status::Pressed => LINE,
                        iced::widget::button::Status::Hovered if selected => LINE,
                        iced::widget::button::Status::Hovered => ACTIVE,
                        _ if selected => ACTIVE,
                        _ => iced::Color::WHITE,
                    }
                    .into(),
                ),
                border: iced::Border {
                    radius: 8.into(),
                    ..Default::default()
                },
                ..Default::default()
            });
        tooltip(
            navigation,
            text(["游戏", "账号管理", "设置"][index]).size(12),
            tooltip::Position::Right,
        )
        .style(panel)
        .into()
    }
    fn dropdown<'a>(
        &'a self,
        id: &'static str,
        items: Vec<(String, String)>,
        selected: Option<&str>,
        hint: &str,
        enabled: bool,
    ) -> Element<'a, Msg> {
        let label = items
            .iter()
            .find(|(key, _)| Some(key.as_str()) == selected)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| hint.into());
        let mut body = column![
            button(row![
                text(label),
                space().width(Fill),
                text("⌄").color(MUTED)
            ])
            .padding([10, 12])
            .width(Fill)
            .style(secondary)
            .on_press_maybe(enabled.then_some(Msg::Drop(id)))
        ]
        .spacing(4);
        if self.open_dropdown == Some(id) {
            let mut options = column![].spacing(2);
            for (key, label) in items {
                let label = if Some(key.as_str()) == selected {
                    format!("{label} ✓")
                } else {
                    label
                };
                options = options.push(
                    button(text(label))
                        .padding([8, 12])
                        .width(Fill)
                        .style(secondary)
                        .on_press_maybe(enabled.then_some(Msg::Select(id, key))),
                );
            }
            body = body.push(scrollable(options).height(iced::Length::Shrink));
        }
        body.width(Fill).into()
    }
    fn exit_view(&self) -> Element<'_, Msg> {
        let busy = self.critical_busy();
        card(
            column![
                heading(
                    if busy {
                        "任务进行中"
                    } else {
                        "有未保存的设置"
                    },
                    18
                ),
                text(if busy {
                    "为避免中断补丁提交或凭据保存，请等待任务完成后退出。"
                } else {
                    "退出会丢弃设置草稿，已保存的配置不受影响。"
                })
                .color(MUTED),
                row![
                    btn("继续使用", Some(Msg::CancelExit), false),
                    btn(
                        if busy {
                            "完成后退出"
                        } else {
                            "丢弃草稿并退出"
                        },
                        Some(Msg::ConfirmExit),
                        true
                    )
                ]
                .spacing(8)
            ]
            .spacing(20),
        )
        .max_width(440)
        .into()
    }
}
fn decode_qr(bytes: &[u8]) -> Result<iced::widget::image::Handle, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes));
    reader.set_format(image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|e| format!("二维码解码失败: {e}"))?
        .to_rgba8();
    Ok(iced::widget::image::Handle::from_rgba(
        image.width(),
        image.height(),
        image.into_raw(),
    ))
}
