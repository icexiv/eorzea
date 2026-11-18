//! UI-owned state. No filesystem, network, window or GPU side effects.
use eorzea::{auth::AuthConfig, config::AppConfig, launcher::LaunchToken};
use std::path::PathBuf;
use tokio::task::JoinHandle;
use zeroize::Zeroize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Home,
    Accounts,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Idle,
    Checking,
    Updating,
    Launching,
}
impl Operation {
    pub fn busy(self) -> bool {
        self != Self::Idle
    }
}

#[derive(Default)]
pub struct TaskSlot {
    pub generation: u64,
    pub handle: Option<JoinHandle<()>>,
}
impl TaskSlot {
    pub fn cancel(&mut self) {
        self.generation += 1;
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
    pub fn accepts(&self, generation: u64) -> bool {
        self.generation == generation
    }
}
impl Drop for TaskSlot {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginMethod {
    Qr,
    Push,
    Password,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginPhase {
    Idle,
    Requesting,
    Waiting,
    Saving,
    Failed,
}
pub struct Login {
    pub open: bool,
    pub method: LoginMethod,
    pub phase: LoginPhase,
    pub username: String,
    pub password: String,
    pub serial: Option<String>,
    pub qr: Option<iced::widget::image::Handle>,
    pub error: Option<String>,
    pub pending_token: Option<LaunchToken>,
    pub task: TaskSlot,
}
impl Default for Login {
    fn default() -> Self {
        Self {
            open: false,
            method: LoginMethod::Qr,
            phase: LoginPhase::Idle,
            username: String::new(),
            password: String::new(),
            serial: None,
            qr: None,
            error: None,
            pending_token: None,
            task: TaskSlot::default(),
        }
    }
}
impl Login {
    pub fn reset(&mut self) {
        self.task.cancel();
        self.password.zeroize();
        self.phase = LoginPhase::Idle;
        self.serial = None;
        self.qr = None;
        self.error = None;
        self.pending_token = None;
    }
    pub fn close(&mut self) {
        self.reset();
        self.open = false;
    }
}
impl Drop for Login {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub game: String,
    pub max: String,
    pub min: String,
    pub launcher_update: eorzea::config::LauncherUpdateSettings,
    pub launch: eorzea::config::LaunchSettings,
    pub wine: eorzea::config::WineSettings,
    pub custom: String,
    pub prefix: String,
    pub dalamud: eorzea::dalamud::model::DalamudSettings,
    pub install: String,
    pub delay: String,
}
fn path_string(path: &Option<PathBuf>) -> String {
    path.as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}
pub fn path(value: &str) -> Option<PathBuf> {
    (!value.trim().is_empty()).then(|| PathBuf::from(value.trim()))
}
impl Draft {
    pub fn new(config: &AppConfig) -> Self {
        #[cfg(not(target_os = "windows"))]
        let wine = config.settings.clone();
        #[cfg(target_os = "windows")]
        let wine = eorzea::config::WineSettings::default();
        Self {
            game: path_string(&config.game_path),
            max: config.download.concurrency.to_string(),
            min: config.download.min_concurrency.to_string(),
            launcher_update: config.launcher_update.clone(),
            launch: config.launch.clone(),
            custom: path_string(&wine.custom_path),
            prefix: path_string(&wine.prefix),
            wine,
            install: path_string(&config.dalamud.install_root),
            delay: config.dalamud.delay_initialize_ms.to_string(),
            dalamud: config.dalamud.clone(),
        }
    }
    /// Only overwrite fields owned by this form; retain unexposed Wine/DXVK options.
    pub fn apply(&self, base: &AppConfig) -> Result<AppConfig, String> {
        let mut config = base.clone();
        config.game_path = path(&self.game);
        config.download.concurrency = self.max.trim().parse().map_err(|_| "最大并发需要正整数")?;
        config.download.min_concurrency = self
            .min
            .trim()
            .parse()
            .map_err(|_| "最少并发需要非负整数")?;
        config.download.validate()?;
        config.launcher_update = self.launcher_update.clone();
        config.launch = self.launch.clone();
        #[cfg(not(target_os = "windows"))]
        {
            let s = &mut config.settings;
            s.startup_type = self.wine.startup_type;
            s.custom_path = path(&self.custom);
            s.prefix = path(&self.prefix);
            #[cfg(target_os = "linux")]
            {
                s.sync = self.wine.sync;
                s.gamemode = self.wine.gamemode;
            }
            #[cfg(target_os = "macos")]
            {
                s.msync = self.wine.msync;
            }
            s.dxvk.enabled = self.wine.dxvk.enabled;
        }
        config.dalamud = self.dalamud.clone();
        config.dalamud.install_root = path(&self.install);
        config.dalamud.delay_initialize_ms = if self.delay.trim().is_empty() {
            eorzea::dalamud::model::DalamudSettings::default().delay_initialize_ms
        } else {
            self.delay
                .trim()
                .parse()
                .map_err(|_| "初始化延迟需要 0 到 4294967295 的整数（毫秒）")?
        };
        Ok(config)
    }
    pub fn dirty(&self, base: &AppConfig) -> bool {
        self.apply(base).as_ref().ok() != Some(base)
    }
}

pub fn initial_account(auth: &AuthConfig) -> Option<String> {
    auth.default_account().map(|a| a.snda_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn draft_roundtrips_launch_method_independently_of_dalamud() {
        let config = AppConfig::default();
        let mut draft = Draft::new(&config);
        draft.launch.method = eorzea::config::SdoLaunchMethod::SdoDll;
        draft.dalamud.enabled = true;
        let saved = draft.apply(&config).unwrap();
        assert_eq!(saved.launch.method, eorzea::config::SdoLaunchMethod::SdoDll);
        assert!(saved.dalamud.enabled);
        assert_eq!(Draft::new(&saved), draft);
    }
    #[test]
    fn invalid_values_are_dirty_and_cannot_save() {
        let config = AppConfig::default();
        let mut d = Draft::new(&config);
        assert!(!d.dirty(&config));
        d.delay = "not a number".into();
        assert!(d.dirty(&config));
        assert!(d.apply(&config).is_err());
        d = Draft::new(&config);
        d.min = "5".into();
        assert!(d.apply(&config).is_err());
    }
    #[test]
    fn draft_preserves_non_ui_configuration() {
        let mut config = AppConfig::default();
        config.area = Some("旧大区".into());
        #[cfg(not(target_os = "windows"))]
        {
            config.settings.env.insert("KEEP".into(), "value".into());
            config.settings.dxvk.hud = Some("fps".into());
        }
        let mut d = Draft::new(&config);
        d.game = " /game ".into();
        let saved = d.apply(&config).unwrap();
        assert_eq!(saved.area, config.area);
        #[cfg(not(target_os = "windows"))]
        {
            assert_eq!(saved.settings.env, config.settings.env);
            assert_eq!(saved.settings.dxvk.hud, config.settings.dxvk.hud);
        }
        assert_eq!(saved.game_path, Some(PathBuf::from("/game")));
    }
    #[test]
    fn cancelling_rejects_old_login_results_and_clears_password() {
        let mut login = Login::default();
        login.open = true;
        login.password = "private".into();
        let generation = login.task.generation;
        login.close();
        assert!(!login.task.accepts(generation));
        assert!(login.password.is_empty());
        assert!(!login.open);
    }
}
