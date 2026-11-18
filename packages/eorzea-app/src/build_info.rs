//! Explicit build identity; never infer release ordering from a Git hash or filesystem date.
use eorzea::config::LauncherUpdateChannel;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Dev,
}
impl Channel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stable => "稳定版",
            Self::Dev => "开发版",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribution {
    Nix,
    Portable,
    Source,
}
impl Distribution {
    pub fn label(self) -> &'static str {
        match self {
            Self::Nix => "Nix 包（仅检查更新）",
            Self::Portable => "官方发行包（手动安装）",
            Self::Source => "源码 / 未知来源（手动安装）",
        }
    }
}

#[derive(Debug, Clone)]
pub struct BuildInfo {
    pub version: &'static str,
    pub commit: Option<&'static str>,
    pub channel: Option<Channel>,
    pub build: Option<u64>,
    pub attempt: Option<u64>,
    pub distribution: Distribution,
}
impl BuildInfo {
    pub fn current() -> Self {
        Self {
            version: option_env!("EORZEA_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")),
            commit: option_env!("EORZEA_BUILD_COMMIT").filter(|v| !v.is_empty()),
            channel: match option_env!("EORZEA_BUILD_CHANNEL") {
                Some("stable") => Some(Channel::Stable),
                Some("dev") => Some(Channel::Dev),
                _ => None,
            },
            build: option_env!("EORZEA_BUILD_NUMBER").and_then(|v| v.parse().ok()),
            attempt: option_env!("EORZEA_BUILD_ATTEMPT").and_then(|v| v.parse().ok()),
            distribution: distribution(
                cfg!(feature = "nix-package"),
                option_env!("EORZEA_DISTRIBUTION"),
                std::env::current_exe().ok().as_deref(),
            ),
        }
    }
    pub fn channel_for(&self, choice: LauncherUpdateChannel) -> Channel {
        match choice {
            LauncherUpdateChannel::Stable => Channel::Stable,
            LauncherUpdateChannel::Dev => Channel::Dev,
            LauncherUpdateChannel::Auto => self.channel.unwrap_or(Channel::Dev),
        }
    }
    pub fn display(&self) -> String {
        let suffix = match self.distribution {
            Distribution::Nix => " · Nix",
            Distribution::Source => " · 源码",
            Distribution::Portable => "",
        };
        format!("Eorzea {}{suffix}", self.version)
    }
}

fn distribution(
    nix_feature: bool,
    declared: Option<&str>,
    executable: Option<&Path>,
) -> Distribution {
    // current_exe resolves profile and wrapper symlinks on supported platforms.
    if nix_feature
        || declared == Some("nix")
        || executable.is_some_and(|p| p.starts_with("/nix/store"))
    {
        Distribution::Nix
    } else if declared == Some("portable") {
        Distribution::Portable
    } else {
        Distribution::Source
    }
}

pub fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("windows-x86_64"),
        ("linux", "x86_64") => Some("linux-x86_64"),
        ("macos", "aarch64") => Some("macos-aarch64"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nix_policy_cannot_be_overridden_by_portable_metadata() {
        assert_eq!(
            distribution(true, Some("portable"), None),
            Distribution::Nix
        );
        assert_eq!(distribution(false, Some("nix"), None), Distribution::Nix);
        assert_eq!(
            distribution(
                false,
                Some("portable"),
                Some(Path::new("/nix/store/hash/bin/app"))
            ),
            Distribution::Nix
        );
        assert_eq!(
            distribution(false, None, Some(Path::new("/nix/store-other/app"))),
            Distribution::Source
        );
        assert_eq!(distribution(false, None, None), Distribution::Source);
    }
    #[test]
    fn auto_channel_follows_build_not_package_version() {
        let mut info = BuildInfo::current();
        info.channel = Some(Channel::Stable);
        assert_eq!(
            info.channel_for(LauncherUpdateChannel::Auto),
            Channel::Stable
        );
        assert_eq!(info.channel_for(LauncherUpdateChannel::Dev), Channel::Dev);
        info.channel = None;
        assert_eq!(info.channel_for(LauncherUpdateChannel::Auto), Channel::Dev);
    }
}
