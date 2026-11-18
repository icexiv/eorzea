//! Read-only launcher release discovery. No archive downloads or installation entry points.
use crate::build_info::{BuildInfo, Channel};
use eorzea::config::{LauncherUpdateSettings, LauncherUpdateSource};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

const MIRROR: &str = "https://icexiv.top/downloads/eorzea.json";
const GITHUB: &str = "https://raw.githubusercontent.com/icexiv/eorzea/main/downloads/github.json";
const MAX_MANIFEST: usize = 256 * 1024;
const CACHE_TTL: u64 = 6 * 60 * 60;
pub const NIX_GUIDE: &str = "https://github.com/icexiv/eorzea/blob/main/docs/nix.md";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Mirror,
    Github,
}
impl Source {
    fn key(self) -> &'static str {
        match self {
            Self::Mirror => "tos",
            Self::Github => "github",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Mirror => "国内镜像",
            Self::Github => "GitHub",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    schema: u32,
    source: String,
    channels: Channels,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Channels {
    // null is a valid unpublished channel, not "already up to date".
    stable: Option<Release>,
    dev: Option<Release>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    tag: String,
    commit: String,
    build: u64,
    pub published_at: String,
    assets: BTreeMap<String, Asset>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Asset {
    filename: String,
    url: String,
    sha256: String,
    size: u64,
    experimental: bool,
}
impl Release {
    pub fn notes_url(&self) -> String {
        // tag is validated before constructing any externally opened URL.
        format!("https://github.com/icexiv/eorzea/releases/tag/{}", self.tag)
    }
    fn dev_order(&self) -> Option<(u64, u64)> {
        // Schema 1 embeds the attempt in the existing immutable filename version.
        let mut parts = self.version.strip_prefix("dev-")?.split('-');
        let commit = parts.next()?;
        let build: u64 = parts.next()?.parse().ok()?;
        let attempt: u64 = parts.next()?.parse().ok()?;
        (parts.next().is_none()
            && commit.len() >= 7
            && commit.len() <= 40
            && self.commit.starts_with(commit)
            && build == self.build
            && attempt > 0)
            .then_some((build, attempt))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Current,
    Newer,
    Older,
    /// Source/Nix builds or a different channel without an authoritative ordering.
    Different,
}

fn compare(info: &BuildInfo, channel: Channel, release: &Release) -> Comparison {
    use std::cmp::Ordering;
    if info.channel == Some(channel) {
        let order = match channel {
            Channel::Stable => semver::Version::parse(info.version.trim_start_matches('v'))
                .ok()
                .zip(semver::Version::parse(release.version.trim_start_matches('v')).ok())
                .map(|(local, remote)| remote.cmp_precedence(&local)),
            Channel::Dev => info
                .build
                .zip(info.attempt)
                .zip(release.dev_order())
                .map(|(local, remote)| remote.cmp(&local)),
        };
        if let Some(order) = order {
            return match order {
                Ordering::Less => Comparison::Older,
                Ordering::Equal => Comparison::Current,
                Ordering::Greater => Comparison::Newer,
            };
        }
    }
    if info.commit == Some(release.commit.as_str()) {
        Comparison::Current
    } else {
        Comparison::Different
    }
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub channel: Channel,
    pub release: Option<Release>,
    pub comparison: Comparison,
    pub platform_available: bool,
    pub source: Source,
    pub cached: bool,
}
impl CheckResult {
    pub fn message(&self) -> String {
        let Some(release) = &self.release else {
            return format!("{}通道尚未发布版本。", self.channel.label());
        };
        match self.comparison {
            Comparison::Current => format!("当前版本与上游一致（{}）。", release.version),
            Comparison::Newer => format!("发现启动器新版本：{}", release.version),
            Comparison::Older => format!("上游版本为 {}，不建议降级。", release.version),
            Comparison::Different => format!(
                "上游{}：{}。与当前构建不同，无法判断先后。",
                self.channel.label(),
                release.version
            ),
        }
    }
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
}
fn valid_hash(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|b| b.is_ascii_hexdigit())
}
impl Manifest {
    fn validate(&self, source: Source) -> Result<(), String> {
        if self.schema != 1 || self.source != source.key() {
            return Err("不支持的更新清单格式或来源".into());
        }
        for (channel, release) in [
            (Channel::Stable, &self.channels.stable),
            (Channel::Dev, &self.channels.dev),
        ] {
            let Some(release) = release else { continue };
            if !valid_token(&release.version)
                || !valid_token(&release.tag)
                || !valid_hash(&release.commit, 40)
                || release.published_at.is_empty()
                || release.published_at.len() > 64
                || release.published_at.chars().any(char::is_control)
            {
                return Err("更新清单的版本信息无效".into());
            }
            match channel {
                Channel::Stable => {
                    let version = semver::Version::parse(release.version.trim_start_matches('v'))
                        .map_err(|_| "稳定版版本号无效")?;
                    if !version.pre.is_empty() {
                        return Err("稳定版通道包含预发布版本".into());
                    }
                }
                Channel::Dev if release.dev_order().is_none() => {
                    return Err("开发版构建标识无效".into());
                }
                Channel::Dev => {}
            }
            for asset in release.assets.values() {
                let url = reqwest::Url::parse(&asset.url).map_err(|_| "发行包地址无效")?;
                if !valid_token(&asset.filename)
                    || !valid_hash(&asset.sha256, 64)
                    || asset.size == 0
                    || url.scheme() != "https"
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err("更新清单的发行包信息无效".into());
                }
            }
        }
        Ok(())
    }
    fn result(
        self,
        info: &BuildInfo,
        channel: Channel,
        platform: Option<&str>,
        source: Source,
        cached: bool,
    ) -> CheckResult {
        let release = match channel {
            Channel::Stable => self.channels.stable,
            Channel::Dev => self.channels.dev,
        };
        CheckResult {
            comparison: release
                .as_ref()
                .map(|r| compare(info, channel, r))
                .unwrap_or(Comparison::Different),
            platform_available: release
                .as_ref()
                .is_some_and(|r| platform.is_some_and(|p| r.assets.contains_key(p))),
            release,
            channel,
            source,
            cached,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CachedManifest {
    checked_at: u64,
    manifest: Manifest,
}
struct Checker {
    client: reqwest::Client,
    endpoints: Vec<(Source, String)>,
    cache: Option<PathBuf>,
}
impl Checker {
    fn cache_path(&self, source: Source) -> Option<PathBuf> {
        self.cache
            .as_ref()
            .map(|p| p.join(format!("{}.json", source.key())))
    }
    async fn cached(&self, source: Source, now: u64) -> Option<Manifest> {
        let path = self.cache_path(source)?;
        tokio::task::spawn_blocking(move || {
            if std::fs::metadata(&path).ok()?.len() > (MAX_MANIFEST * 2) as u64 {
                return None;
            }
            let cache: CachedManifest = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
            let age = now.checked_sub(cache.checked_at)?;
            (age < CACHE_TTL && cache.manifest.validate(source).is_ok()).then_some(cache.manifest)
        })
        .await
        .ok()
        .flatten()
    }
    async fn manifest(&self, source: Source, url: &str, now: u64) -> Result<Manifest, String> {
        let mut response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        if response
            .content_length()
            .is_some_and(|size| size > MAX_MANIFEST as u64)
        {
            return Err("更新清单过大".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            if bytes.len() + chunk.len() > MAX_MANIFEST {
                return Err("更新清单过大".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let manifest: Manifest =
            serde_json::from_slice(&bytes).map_err(|e| format!("更新清单解析失败: {e}"))?;
        manifest.validate(source)?;
        if let Some(path) = self.cache_path(source) {
            let saved = CachedManifest {
                checked_at: now,
                manifest: manifest.clone(),
            };
            // Atomic cache replacement, including on Windows. Never touch application files.
            let result = tokio::task::spawn_blocking(move || -> Result<(), std::io::Error> {
                let parent = path.parent().expect("cache parent");
                std::fs::create_dir_all(parent)?;
                let mut file = tempfile::NamedTempFile::new_in(parent)?;
                serde_json::to_writer(file.as_file_mut(), &saved)?;
                file.persist(path).map_err(|e| e.error)?;
                Ok(())
            })
            .await;
            if !matches!(result, Ok(Ok(()))) {
                tracing::debug!(?result, "启动器更新缓存不可写，忽略");
            }
        }
        Ok(manifest)
    }
    async fn check(
        &self,
        info: &BuildInfo,
        channel: Channel,
        platform: Option<&str>,
        force: bool,
        now: u64,
    ) -> Result<CheckResult, String> {
        if !force {
            // A valid fallback cache should not require contacting a failed primary on every launch.
            for (source, _) in &self.endpoints {
                if let Some(manifest) = self.cached(*source, now).await {
                    return Ok(manifest.result(info, channel, platform, *source, true));
                }
            }
        }
        let mut failures = Vec::new();
        for (source, url) in &self.endpoints {
            match self.manifest(*source, url, now).await {
                Ok(manifest) => {
                    return Ok(manifest.result(info, channel, platform, *source, false));
                }
                Err(e) => {
                    tracing::debug!(source = source.label(), error = %e, "启动器更新清单请求失败");
                    failures.push(format!("{}: {e}", source.label()));
                }
            }
        }
        Err(failures.join("；"))
    }
}

pub async fn check(
    info: &BuildInfo,
    settings: &LauncherUpdateSettings,
    force: bool,
) -> Result<CheckResult, String> {
    let endpoints = match settings.source {
        LauncherUpdateSource::Auto => vec![
            (Source::Mirror, MIRROR.into()),
            (Source::Github, GITHUB.into()),
        ],
        LauncherUpdateSource::Mirror => vec![(Source::Mirror, MIRROR.into())],
        LauncherUpdateSource::Github => vec![(Source::Github, GITHUB.into())],
    };
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .user_agent(concat!("eorzea-app/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    Checker {
        client,
        endpoints,
        cache: dirs::cache_dir().map(|p| p.join("eorzea/launcher-updates")),
    }
    .check(
        info,
        info.channel_for(settings.channel),
        crate::build_info::platform(),
        force,
        now,
    )
    .await
}

#[cfg(test)]
mod tests;
