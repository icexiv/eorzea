//! 原版 + shim 启动途径的独立准备过程。不运行官方启动器或从 ZIP 部署其他文件。
//!
//! 原版来源是官方启动器全文件清单中的 Launcher3Modules 入口（不是另一个同名
//! 的 46 KiB 入口）。只接受与当前清单长度/MD5 一致、提供三个本地导出的 x64 DLL。

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

mod pe;

pub const ENTRY: &str = "sdo/sdologin/sdologinentry64.dll";
pub const ORIGINAL: &str = "sdo/sdologin/sdologinentry64.sdo.dll";
const INDEX: &str =
    "https://ff.autopatch.sdo.com/v3launcher/build/ver2data/791000814/8821/-1/ver2.dat";
const MANIFEST: &str = "https://ff.autopatch.sdo.com/v3launcher/build/791000814/8821/client-all-files-list/client_all_files_list.dat";
const MODULE: &str = "Launcher3Update/Launcher3Modules/sdologinentry64.dll";
const MEMBER: &str = "Pkg/Launcher3Update/Launcher3Modules/sdologinentry64.dll";
const SHIM_URL: &str = "https://raw.githubusercontent.com/ottercorp/XIVLauncher.Core/cn/src/XIVLauncher.Core/Resources/binaries/sdologinentry64.dll";
const SHIM_MD5: &str = "101f6566c2182977f2f40ef57be311ed";
const MAX_DLL: usize = 4 * 1024 * 1024;
const MAX_META: usize = 2 * 1024 * 1024;
const MAX_ZIP: usize = 256 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum EntryError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("下载失败：{0}")]
    Network(#[from] reqwest::Error),
    #[error("归档无效：{0}")]
    Zip(#[from] zip::result::ZipError),
}
type Result<T> = std::result::Result<T, EntryError>;
fn invalid(message: impl Into<String>) -> EntryError {
    EntryError::Invalid(message.into())
}
fn md5_matches(bytes: &[u8], expected: &str) -> bool {
    expected.len() == 32 && format!("{:x}", md5::compute(bytes)).eq_ignore_ascii_case(expected)
}
fn is_shim(bytes: &[u8]) -> bool {
    md5_matches(bytes, SHIM_MD5)
        || bytes.windows(9).any(|w| w == b"ottercorp")
        || bytes
            .windows(18)
            .any(|w| w == b"o\0t\0t\0e\0r\0c\0o\0r\0p\0")
}

fn read_dll(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((MAX_DLL + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_DLL {
        return Err(invalid("登录 DLL 超过大小限制"));
    }
    pe::validate(&bytes)?;
    Ok(bytes)
}
fn read_original(path: &Path) -> Result<Vec<u8>> {
    let bytes = read_dll(path)?;
    if is_shim(&bytes) {
        return Err(invalid(format!(
            "原版备份不能是 shim：{}；请保留文件并另行检查备份来源",
            path.display()
        )));
    }
    Ok(bytes)
}

/// 静态预检：文件结构和导出，不代表在所有 Wine 构建上都可成功加载。
pub fn validate_installation(root: &Path) -> Result<()> {
    let shim = read_dll(&root.join(ENTRY))?;
    if !md5_matches(&shim, SHIM_MD5) {
        return Err(invalid("登录入口不是受支持的 ottercorp shim"));
    }
    read_original(&root.join(ORIGINAL))?;
    Ok(())
}

/// 先准备原版，再部署 shim。已有有效原版绝不覆盖；无效备份报错，不静默丢弃。
pub async fn prepare(root: &Path, progress: impl FnMut(u64, u64)) -> Result<()> {
    let tools = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".eorzea/tools");
    prepare_with_tools(root, &tools, progress).await
}

async fn prepare_with_tools(
    root: &Path,
    tools: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<()> {
    let root = root.to_path_buf();
    let check_root = root.clone();
    let (_lock, original, shim_ready) = crate::blocking::run(move || -> Result<_> {
        std::fs::create_dir_all(check_root.join(".eorzea"))?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(check_root.join(".eorzea/sdo-entry.lock"))?;
        lock.try_lock()
            .map_err(|_| invalid("另一个任务正在准备 SDO 登录入口"))?;
        let backup = check_root.join(ORIGINAL);
        let entry = check_root.join(ENTRY);
        let original = if backup.exists() {
            Some(read_original(&backup)?)
        } else if entry.exists() {
            let bytes = read_dll(&entry)?;
            (!is_shim(&bytes)).then_some(bytes)
        } else {
            None
        };
        let shim_ready = read_dll(&entry).is_ok_and(|bytes| md5_matches(&bytes, SHIM_MD5));
        Ok((lock, original, shim_ready))
    })
    .await??;

    let mut builder = reqwest::Client::builder();
    #[cfg(not(target_arch = "wasm32"))]
    {
        builder = builder
            .timeout(Duration::from_secs(180))
            .redirect(reqwest::redirect::Policy::none());
    }
    let client = builder.build()?;
    let original = match original {
        Some(bytes) => bytes,
        None => download_original(&client, tools, &mut progress).await?,
    };
    let shim = if shim_ready {
        None
    } else {
        Some(download_shim(&client, tools, &mut progress).await?)
    };
    crate::blocking::run(move || -> Result<()> {
        // 即使调用方取消等待，阻塞提交完成前也必须持有安装锁。
        let _lock = _lock;
        // 网络请求期间不触碰安装；所有材料验证后才提交。
        if !root.join(ORIGINAL).exists() {
            atomic_write(&root.join(ORIGINAL), &original)?;
        }
        if let Some(shim) = shim {
            atomic_write(&root.join(ENTRY), &shim)?;
        }
        validate_installation(&root)
    })
    .await??;
    Ok(())
}

async fn download_shim(
    client: &reqwest::Client,
    tools: &Path,
    progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<u8>> {
    let cache = tools.join("sdologinentry64.dll");
    let cached = cache.clone();
    if let Some(bytes) =
        crate::blocking::run(move || read_dll(&cached).ok().filter(|b| md5_matches(b, SHIM_MD5)))
            .await?
    {
        return Ok(bytes);
    }
    let bytes = download(client, SHIM_URL, MAX_DLL, progress).await?;
    if !md5_matches(&bytes, SHIM_MD5) {
        return Err(invalid("shim 摘要不匹配，未修改游戏文件"));
    }
    pe::validate(&bytes)?;
    let saved = bytes.clone();
    crate::blocking::run(move || atomic_write(&cache, &saved)).await??;
    Ok(bytes)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Index {
    base_url: String,
    packages: Vec<Package>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Package {
    file_list_url: String,
    md5: String,
    to: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileList {
    base_url: String,
    file_list: Vec<Archive>,
}
#[derive(Deserialize)]
struct Archive {
    url: String,
    md5: String,
    size: String,
}
#[derive(Debug)]
struct Expected {
    size: usize,
    md5: String,
    version: String,
}

fn manifest_entry(bytes: &[u8]) -> Result<Expected> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("官方清单不是 UTF-8"))?;
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    let header = lines.next().ok_or_else(|| invalid("官方清单为空"))?;
    let version = header
        .split('|')
        .nth(2)
        .ok_or_else(|| invalid("官方清单缺少版本"))?;
    let mut found = None;
    for line in lines {
        let fields: Vec<_> = line.trim().split('|').collect();
        if fields
            .first()
            .is_none_or(|p| p.replace('\\', "/") != MODULE)
        {
            continue;
        }
        if found.is_some() || fields.len() != 3 {
            return Err(invalid("官方入口清单重复或无效"));
        }
        let size = fields[1]
            .parse::<usize>()
            .map_err(|_| invalid("官方入口长度无效"))?;
        if size == 0
            || size > MAX_DLL
            || fields[2].len() != 32
            || !fields[2].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(invalid("官方入口长度或摘要超出允许范围"));
        }
        found = Some(Expected {
            size,
            md5: fields[2].into(),
            version: version.into(),
        });
    }
    found.ok_or_else(|| invalid("官方清单不包含 Launcher3Modules 登录入口"))
}

// 官方列表的 /suffix 是相对 baseUrl 的路径，不是网站根路径。
fn official_url(base: &str, suffix: &str) -> Result<String> {
    let url = format!(
        "{}/{}",
        base.trim_end_matches('/'),
        suffix.trim_start_matches('/')
    );
    let parsed = reqwest::Url::parse(&url).map_err(|_| invalid("官方更新 URL 无效"))?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("ff.autopatch.sdo.com")
        || parsed.port().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !parsed.path().starts_with("/v3launcher/build/")
        || suffix.split(['/', '\\']).any(|s| s == ".." || s == ".")
        || suffix.contains(['?', '#', '%', ':', '\\'])
    {
        return Err(invalid("拒绝非官方或不安全的更新 URL"));
    }
    Ok(url)
}
fn version_key(version: &str) -> Result<Vec<u32>> {
    let parts: Vec<_> = version
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| invalid("官方组件版本无效"))?;
    if parts.len() != 4 {
        return Err(invalid("官方组件版本应为四段数字"));
    }
    Ok(parts)
}

async fn download_original(
    client: &reqwest::Client,
    tools: &Path,
    progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<u8>> {
    let expected = manifest_entry(&download(client, MANIFEST, MAX_META, &mut |_, _| {}).await?)?;
    let cache = tools
        .join("sdo")
        .join(format!("{}.dll", expected.md5.to_ascii_lowercase()));
    let cached_path = cache.clone();
    let hash = expected.md5.clone();
    if let Some(bytes) = crate::blocking::run(move || {
        read_original(&cached_path)
            .ok()
            .filter(|b| md5_matches(b, &hash))
    })
    .await?
    {
        return Ok(bytes);
    }

    let index_bytes = download(client, INDEX, MAX_META, &mut |_, _| {}).await?;
    let index: Index =
        serde_json::from_slice(&index_bytes).map_err(|e| invalid(format!("官方索引无效：{e}")))?;
    if index.packages.is_empty() || index.packages.len() > 64 {
        return Err(invalid("官方更新包数量异常"));
    }
    let target = version_key(&expected.version)?;
    let mut packages = index
        .packages
        .into_iter()
        .map(|p| Ok((version_key(&p.to)?, p)))
        .collect::<Result<Vec<_>>>()?;
    packages.sort_by(|a, b| b.0.cmp(&a.0));
    let mut budget = 512 * 1024 * 1024usize;
    for (version, package) in packages {
        if version > target {
            continue;
        }
        let list_url = official_url(&index.base_url, &package.file_list_url)?;
        let list_bytes = download(client, &list_url, MAX_META, &mut |_, _| {}).await?;
        if !md5_matches(&list_bytes, &package.md5) {
            return Err(invalid("官方包列表 MD5 不匹配"));
        }
        let list: FileList = serde_json::from_slice(&list_bytes)
            .map_err(|e| invalid(format!("官方包列表无效：{e}")))?;
        if list.file_list.len() > 32 {
            return Err(invalid("官方归档数量异常"));
        }
        for archive in list.file_list {
            let size = archive
                .size
                .parse::<usize>()
                .map_err(|_| invalid("官方归档长度无效"))?;
            if size == 0 || size > MAX_ZIP {
                return Err(invalid("官方归档超过 256 MiB 限制"));
            }
            budget = budget
                .checked_sub(size)
                .ok_or_else(|| invalid("查找完整官方入口超过 512 MiB 总下载限制"))?;
            let url = official_url(&list.base_url, &archive.url)?;
            let bytes = download(client, &url, size, progress).await?;
            if bytes.len() != size || !md5_matches(&bytes, &archive.md5) {
                return Err(invalid("官方 ZIP 长度或 MD5 不匹配"));
            }
            let size = expected.size;
            let hash = expected.md5.clone();
            if let Some(dll) =
                crate::blocking::run(move || extract_original(bytes, size, &hash)).await??
            {
                let saved = dll.clone();
                let receipt = serde_json::json!({"version": expected.version, "manifest": MANIFEST,
                    "source": url, "member": MEMBER, "md5": expected.md5, "bytes": expected.size,
                    "archive_md5": archive.md5, "file_list_md5": package.md5});
                crate::blocking::run(move || -> Result<()> {
                    atomic_write(&cache, &saved)?;
                    atomic_write(
                        &cache.with_extension("json"),
                        &serde_json::to_vec_pretty(&receipt).map_err(|e| invalid(e.to_string()))?,
                    )
                })
                .await??;
                return Ok(dll);
            }
        }
    }
    Err(invalid(
        "官方 ZIP 中未找到与当前全文件清单一致的完整入口；不尝试应用未知差分",
    ))
}

fn extract_original(bytes: Vec<u8>, size: usize, hash: &str) -> Result<Option<Vec<u8>>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    if archive.len() > 4096 {
        return Err(invalid("官方 ZIP 成员数量异常"));
    }
    let mut found = None;
    let mut seen = false;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().replace('\\', "/");
        // 只读取精确白名单成员，不将归档路径交给文件系统。
        if name != MEMBER {
            continue;
        }
        if seen {
            return Err(invalid("官方 ZIP 入口成员重复"));
        }
        seen = true;
        if file.is_dir() || file.size() != size as u64 {
            continue;
        }
        let mut dll = Vec::new();
        (&mut file)
            .take((MAX_DLL + 1) as u64)
            .read_to_end(&mut dll)?; // zip reader 同时校验 CRC。
        if dll.len() != size || !md5_matches(&dll, hash) {
            continue;
        }
        pe::validate(&dll)?;
        if is_shim(&dll) {
            return Err(invalid("官方入口意外识别为 shim"));
        }
        found = Some(dll);
    }
    Ok(found)
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    max: usize,
    progress: &mut impl FnMut(u64, u64),
) -> Result<Vec<u8>> {
    let mut response = client.get(url).send().await?.error_for_status()?;
    if response.content_length().is_some_and(|n| n > max as u64) {
        return Err(invalid("下载内容超过大小限制"));
    }
    let total = response.content_length().unwrap_or(0);
    let mut bytes = Vec::new();
    #[cfg(not(target_arch = "wasm32"))]
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > max {
            return Err(invalid("下载内容超过大小限制"));
        }
        bytes.extend_from_slice(&chunk);
        progress(bytes.len() as u64, total);
    }
    #[cfg(target_arch = "wasm32")]
    {
        bytes.extend_from_slice(&response.bytes().await?);
        if bytes.len() > max {
            return Err(invalid("下载内容超过大小限制"));
        }
        progress(bytes.len() as u64, total);
    }
    Ok(bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| invalid("登录入口路径缺少父目录"))?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".sdo-entry-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    // create_new 失败时不清理：那个临时文件不是本次调用创建的。
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests;
