//! Renderer-independent worker bridge. Results wake Iced; progress snapshots are coalesced.
use super::state::Draft;
use eorzea::{
    auth::{self, AuthConfig},
    config::{self, AppConfig, WineSettings},
    dalamud::DalamudStatus,
    game_files::{CheckResult, GameFileManager, UpdateProgress, default_patch_dir},
    launcher::{LaunchEvent, LaunchRequest, LaunchToken, Launcher},
};
use eorzea_auth::{PatchListEntry, SdoArea};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
};
use tokio::runtime::Runtime;

pub enum Event {
    LauncherChecked(u64, Result<crate::updates::CheckResult, String>),
    LinkOpened(Result<(), String>),
    Config(AppConfig, AuthConfig),
    Launcher(Result<Arc<Launcher>, String>),
    Areas(Result<Vec<SdoArea>, String>),
    Local(u64, LocalSummary),
    Dalamud(u64, Option<DalamudStatus>),
    Qr(u64, Vec<u8>),
    Push(u64, Option<String>),
    Login(u64, Result<LaunchToken, String>),
    AuthSaved {
        login: Option<u64>,
        selected: Option<String>,
        result: Result<AuthConfig, String>,
    },
    SettingsSaved(Result<AppConfig, String>),
    LaunchMethodSaved(Result<AppConfig, String>),
    DalamudEnabledSaved(Result<AppConfig, String>),
    Folder(Option<PathBuf>),
    GamePath(u64, bool),
    Checked(u64, Result<(Vec<PatchListEntry>, bool), String>),
    Updated(u64, Result<usize, String>),
    LaunchStatus(u64, String),
    AuthReload(AuthConfig),
    Launched(u64, Result<u32, String>),
}
pub fn check_launcher(
    runtime: &Runtime,
    bus: Bus,
    generation: u64,
    info: crate::build_info::BuildInfo,
    settings: eorzea::config::LauncherUpdateSettings,
    force: bool,
) -> tokio::task::JoinHandle<()> {
    runtime.spawn(async move {
        let result = crate::updates::check(&info, &settings, force).await;
        bus.send(Event::LauncherChecked(generation, result));
    })
}
pub fn open_link(runtime: &Runtime, bus: Bus, url: String) {
    runtime.spawn_blocking(move || {
        bus.send(Event::LinkOpened(
            open::that_detached(url).map_err(|e| format!("打开链接失败: {e}")),
        ));
    });
}
#[derive(Default)]
pub struct LocalSummary {
    pub found: bool,
    pub versions: Option<(String, String)>,
    pub wine: String,
}
impl LocalSummary {
    fn read(config: &AppConfig) -> Self {
        let versions = config.game_path.as_ref().map(|p| {
            let v = GameFileManager::new().status(p);
            (v.boot, v.ffxiv)
        });
        let found = config
            .game_path
            .as_ref()
            .is_some_and(|p| p.join("game/ffxiv_dx11.exe").is_file());
        #[cfg(not(target_os = "windows"))]
        let wine = eorzea::wine::WineTool::detect(config.settings.custom_path.as_deref())
            .map(|w| format!("本地探测：{}", w.wine64_path.display()))
            .unwrap_or_else(|| "未检测到可用 Wine".into());
        #[cfg(target_os = "windows")]
        let wine = String::new();
        Self {
            versions,
            found,
            wine,
        }
    }
}
pub fn refresh_local(runtime: &Runtime, bus: Bus, generation: u64, config: AppConfig) {
    runtime.spawn_blocking(move || bus.send(Event::Local(generation, LocalSummary::read(&config))));
}
#[derive(Clone)]
pub struct Bus {
    pub tx: mpsc::Sender<Event>,
    pub progress: Arc<Mutex<Option<(u64, UpdateProgress)>>>,
    pub launch_status: Arc<Mutex<Option<(u64, String)>>>,
    pub wake: Arc<tokio::sync::Notify>,
}
impl Bus {
    pub fn send(&self, event: Event) {
        if let Event::LaunchStatus(generation, status) = event {
            *self.launch_status.lock().unwrap_or_else(|e| e.into_inner()) =
                Some((generation, status));
            self.wake.notify_one();
            return;
        }
        if self.tx.send(event).is_ok() {
            self.wake.notify_one();
        }
    }
    fn progress(&self, generation: u64, progress: UpdateProgress) {
        *self.progress.lock().unwrap_or_else(|e| e.into_inner()) = Some((generation, progress));
        self.wake.notify_one();
    }
}
pub fn initialize(runtime: &Runtime, bus: Bus) {
    runtime.spawn(async move {
        match tokio::task::spawn_blocking(|| {
            (config::load_app_default(), auth::load(&auth::config_path()))
        })
        .await
        {
            Ok((config, auth)) => bus.send(Event::Config(config, auth)),
            Err(e) => bus.send(Event::Launcher(Err(format!("读取配置失败: {e}")))),
        }
        let launcher = tokio::task::spawn_blocking(Launcher::new).await;
        bus.send(Event::Launcher(match launcher {
            Ok(Ok(l)) => Ok(Arc::new(l)),
            Ok(Err(e)) => Err(e.to_string()),
            Err(e) => Err(e.to_string()),
        }));
        fetch_areas(bus).await;
    });
}
pub async fn fetch_areas(bus: Bus) {
    let list = eorzea_auth::sdo::SdoAuth::fetch_server_list()
        .await
        .map(|mut list| {
            list.sort_by_key(|a| a.area_order);
            list
        })
        .map_err(|e| e.to_string());
    bus.send(Event::Areas(list));
}
pub fn inspect(
    runtime: &Runtime,
    bus: Bus,
    generation: u64,
    config: AppConfig,
    load_dalamud: bool,
) {
    runtime.spawn(async move {
        let local = config.clone();
        let summary = tokio::task::spawn_blocking(move || LocalSummary::read(&local))
            .await
            .unwrap_or_default();
        bus.send(Event::Local(generation, summary));
        let status = if load_dalamud {
            if let Some(root) = config.game_path.as_ref() {
                let install = config
                    .dalamud
                    .install_root
                    .clone()
                    .unwrap_or_else(eorzea::dalamud::updater::default_install_root);
                Some(
                    eorzea::dalamud::updater::status(
                        &reqwest::Client::new(),
                        &install,
                        root,
                        &config.dalamud.track,
                    )
                    .await,
                )
            } else {
                None
            }
        } else {
            None
        };
        bus.send(Event::Dalamud(generation, status));
    });
}
pub enum AuthChange {
    Add(LaunchToken, Option<String>),
    Default(String),
    Delete(String),
    Area(String, String),
}
pub fn change_auth(path: &std::path::Path, change: AuthChange) -> Result<AuthConfig, String> {
    let mut cfg = auth::load(path);
    match change {
        AuthChange::Add(token, area) => {
            let make_default = cfg.default_account().is_none();
            cfg.upsert(
                auth::Account {
                    snda_id: token.snda_id,
                    username: token.username,
                    auto_login_session_key: token.auto_login_session_key,
                    area,
                },
                make_default,
            );
        }
        AuthChange::Default(id) => {
            cfg.default_account = Some(cfg.find(&id).ok_or("账号已被移除")?.display_name().into());
        }
        AuthChange::Delete(id) => {
            if !cfg.remove(&id) {
                return Err("账号已被移除".into());
            }
        }
        AuthChange::Area(id, area) => {
            cfg.accounts
                .iter_mut()
                .find(|a| a.snda_id == id)
                .ok_or("账号已被移除")?
                .area = Some(area);
        }
    }
    auth::save(path, &cfg).map_err(|e| format!("保存账号配置失败: {e}"))?;
    Ok(cfg)
}
pub fn save_auth(
    runtime: &Runtime,
    bus: Bus,
    change: AuthChange,
    login: Option<u64>,
    selected: Option<String>,
) {
    runtime.spawn(async move {
        let result = tokio::task::spawn_blocking(move || change_auth(&auth::config_path(), change))
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
        bus.send(Event::AuthSaved {
            login,
            selected,
            result,
        });
    });
}
pub fn save_settings(runtime: &Runtime, bus: Bus, draft: Draft) {
    runtime.spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            let config = draft.apply(&config::load_app_default())?;
            config::save_app(&config::settings_path(), &config).map_err(|e| e.to_string())?;
            Ok(config)
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
        bus.send(Event::SettingsSaved(result));
    });
}
/// 主页切换启动途径：只写入途径，不改动 Dalamud 开关。
/// 不提交设置页里尚未保存的其他草稿。
pub fn save_launch_method(runtime: &Runtime, bus: Bus, method: eorzea::config::SdoLaunchMethod) {
    runtime.spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            let path = config::settings_path();
            let mut config = config::load_app_default();
            config.launch.method = method;
            config::save_app(&path, &config).map_err(|e| e.to_string())?;
            Ok(config)
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
        bus.send(Event::LaunchMethodSaved(result));
    });
}
/// 主页启用/关闭 Dalamud：只写入该开关，不改动启动途径。
pub fn save_dalamud_enabled(runtime: &Runtime, bus: Bus, enabled: bool) {
    runtime.spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            let path = config::settings_path();
            let mut config = config::load_app_default();
            config.dalamud.enabled = enabled;
            config::save_app(&path, &config).map_err(|e| e.to_string())?;
            Ok(config)
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
        bus.send(Event::DalamudEnabledSaved(result));
    });
}
pub fn check(runtime: &Runtime, bus: Bus, generation: u64, root: PathBuf, area: Option<SdoArea>) {
    runtime.spawn(async move {
        let result = async {
            let mgr = GameFileManager::new();
            if let Some(pending) = mgr.pending_update(&root).await.map_err(|e| e.to_string())? {
                return Ok((pending.patches, true));
            }
            let area = area.ok_or("请先选择大区")?;
            match mgr
                .check_update(&area, &root, false, 5)
                .await
                .map_err(|e| e.to_string())?
            {
                CheckResult::UpToDate { .. } => Ok((Vec::new(), false)),
                CheckResult::NeedsPatch { patches, .. } => Ok((patches, false)),
                CheckResult::NeedsPatchBoot => Err("boot 需要更新（暂不支持）".into()),
            }
        }
        .await;
        bus.send(Event::Checked(generation, result));
    });
}
pub fn update(
    runtime: &Runtime,
    bus: Bus,
    generation: u64,
    root: PathBuf,
    list: Vec<PatchListEntry>,
    download: config::DownloadSettings,
) {
    runtime.spawn(async move {
        let patch_dir = default_patch_dir(&root);
        let result = GameFileManager::new()
            .update_with_min_concurrency(
                &list,
                &patch_dir,
                &root,
                download.concurrency,
                download.min_concurrency,
                |progress| bus.progress(generation, progress),
            )
            .await
            .map(|s| s.installed.len())
            .map_err(|e| e.to_string());
        bus.send(Event::Updated(generation, result));
    });
}
pub struct Launch {
    pub root: PathBuf,
    pub account: String,
    pub area: SdoArea,
    pub areas: Vec<SdoArea>,
    pub wine: WineSettings,
    pub dalamud: bool,
    pub launch_method: eorzea::config::SdoLaunchMethod,
}
pub fn launch(
    runtime: &Runtime,
    bus: Bus,
    generation: u64,
    launcher: Arc<Launcher>,
    input: Launch,
) {
    runtime.spawn(async move {
        let auth_path = auth::config_path();
        let result = launcher
            .launch_account(
                LaunchRequest {
                    game_root: &input.root,
                    auth_path: &auth_path,
                    account: Some(&input.account),
                    area: input.area,
                    areas: input.areas,
                    wine: &input.wine,
                    dalamud: Some(input.dalamud),
                    launch_method: Some(input.launch_method),
                },
                |event| {
                    let status = match event {
                        LaunchEvent::DalamudProgress(phase, done, total) => {
                            if total > 0 {
                                format!("正在准备 {}… {} / {}", phase.label(), done, total)
                            } else {
                                format!("正在准备 {}…", phase.label())
                            }
                        }
                        LaunchEvent::LoggingIn { account_name } => {
                            format!("正在自动登录 {account_name}…")
                        }
                        LaunchEvent::SessionRefreshed { .. } => {
                            bus.send(Event::AuthReload(auth::load(&auth_path)));
                            "自动登录成功，凭据已保存".into()
                        }
                        LaunchEvent::Starting { .. } => "正在启动游戏…".into(),
                    };
                    bus.send(Event::LaunchStatus(generation, status));
                },
            )
            .await
            .map(|r| r.child.id())
            .map_err(|e| {
                let hint = match &e {
                    eorzea::launcher::LauncherError::DalamudNotReady(_) => {
                        "（可关闭本次加载 Dalamud 后重试）"
                    }
                    eorzea::launcher::LauncherError::Auth(_) => "（请到账号管理重新登录）",
                    _ => "",
                };
                format!("{e}{hint}")
            });
        bus.send(Event::Launched(generation, result));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_updates_reread_and_preserve_rotated_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.toml");
        let mut config = AuthConfig::default();
        config.upsert(
            auth::Account {
                snda_id: "one".into(),
                username: None,
                auto_login_session_key: Some("rotated".into()),
                area: None,
            },
            true,
        );
        auth::save(&path, &config).unwrap();
        let saved = change_auth(&path, AuthChange::Area("one".into(), "陆行鸟".into())).unwrap();
        assert_eq!(
            saved.find("one").unwrap().auto_login_session_key.as_deref(),
            Some("rotated")
        );
        assert!(change_auth(&path, AuthChange::Default("missing".into())).is_err());
    }
}
