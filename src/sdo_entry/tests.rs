use super::*;

#[test]
fn shim_marker_detection_accepts_ascii_and_utf16_but_is_not_pe_validation() {
    assert!(is_shim(b"prefix ottercorp suffix"));
    assert!(is_shim(b"o\0t\0t\0e\0r\0c\0o\0r\0p\0"));
    assert!(!is_shim(b"Shanda Games sdologinentry64"));
    assert!(pe::validate(b"ottercorp").is_err());
}

fn dll() -> Vec<u8> {
    let mut b = vec![0; 2048];
    fn w16(b: &mut [u8], o: usize, x: u16) {
        b[o..o + 2].copy_from_slice(&x.to_le_bytes());
    }
    fn w32(b: &mut [u8], o: usize, x: u32) {
        b[o..o + 4].copy_from_slice(&x.to_le_bytes());
    }
    b[..2].copy_from_slice(b"MZ");
    w32(&mut b, 0x3c, 0x80);
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    w16(&mut b, 0x84, 0x8664);
    w16(&mut b, 0x86, 1);
    w16(&mut b, 0x94, 0xf0);
    w16(&mut b, 0x96, 0x2002);
    w16(&mut b, 0x98, 0x20b);
    w32(&mut b, 0x104, 16);
    w32(&mut b, 0x108, 0x1000);
    w32(&mut b, 0x10c, 0x200);
    w32(&mut b, 0x194, 0x1000); // section VA
    w32(&mut b, 0x198, 0x600);
    w32(&mut b, 0x19c, 0x200);
    w32(&mut b, 0x1ac, 0x60000020);
    w32(&mut b, 0x214, 3);
    w32(&mut b, 0x218, 3);
    w32(&mut b, 0x21c, 0x1040);
    w32(&mut b, 0x220, 0x1050);
    w32(&mut b, 0x224, 0x1060);
    let mut pos = 0x280;
    for (i, name) in ["SDOLGetModule", "SDOLInitialize", "SDOLTerminal"]
        .iter()
        .enumerate()
    {
        w32(&mut b, 0x240 + i * 4, 0x1300 + i as u32);
        w32(&mut b, 0x250 + i * 4, (pos + 0xe00) as u32);
        w16(&mut b, 0x260 + i * 2, i as u16);
        b[pos..pos + name.len()].copy_from_slice(name.as_bytes());
        pos += name.len() + 1;
    }
    b
}

#[test]
fn pe_requires_architecture_dll_and_actual_exports() {
    let b = dll();
    pe::validate(&b).unwrap();
    for offset in [0, 0x84, 0x97, 0x98, 0x108, 0x250, 0x280] {
        let mut broken = b.clone();
        broken[offset] ^= 0xff;
        assert!(pe::validate(&broken).is_err(), "offset {offset:x}");
    }
    let mut broken = b.clone();
    broken[0x214..0x218].fill(0);
    assert!(pe::validate(&broken).is_err());
    for end in [0, 10, 100, 264, 512] {
        assert!(pe::validate(&b[..end]).is_err());
    }
}

#[test]
fn exact_manifest_path_does_not_confuse_two_official_dlls() {
    let m = b"https://official|product|0.0.0.26\nLauncher3Update\\sdo\\sdologin\\sdologinentry64.dll|46080|00000000000000000000000000000000\nLauncher3Update\\Launcher3Modules\\sdologinentry64.dll|511352|ACCDEDB3A2399333A21F82F816CD739D\n";
    let e = manifest_entry(m).unwrap();
    assert_eq!(e.size, 511352);
    assert_eq!(e.version, "0.0.0.26");
    assert_eq!(e.md5, "ACCDEDB3A2399333A21F82F816CD739D");
    assert!(manifest_entry(b"header\n").is_err());
}

#[test]
fn only_https_official_urls_are_accepted() {
    let base = "https://ff.autopatch.sdo.com/v3launcher/build/diff";
    assert_eq!(
        official_url(base, "/package.zip").unwrap(),
        format!("{base}/package.zip")
    );
    for suffix in [
        "../x",
        "%2e%2e/x",
        "http://evil/x",
        "x?token=x",
        "x\\y",
        "x#f",
    ] {
        assert!(official_url(base, suffix).is_err(), "{suffix}");
    }
    for base in [
        "http://ff.autopatch.sdo.com/v3launcher/build",
        "https://evil/v3launcher/build",
        "https://ff.autopatch.sdo.com:443/v3launcher/build",
        "https://ff.autopatch.sdo.com/else",
    ] {
        // URL 标准会归一化显式默认端口；非默认端口仍须拒绝。
        if base.contains(":443") {
            continue;
        }
        assert!(official_url(base, "x").is_err());
    }
    assert!(official_url("https://ff.autopatch.sdo.com:444/v3launcher/build", "x").is_err());
}

fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn zip_reads_only_exact_whitelisted_member_and_checks_manifest_digest() {
    let dll = dll();
    let hash = format!("{:x}", md5::compute(&dll));
    let zip = archive(&[("../../escape.dll", b"evil"), (MEMBER, &dll)]);
    assert_eq!(
        extract_original(zip.clone(), dll.len(), &hash).unwrap(),
        Some(dll.clone())
    );
    assert!(
        extract_original(zip, dll.len(), "00000000000000000000000000000000")
            .unwrap()
            .is_none()
    );
    let zip = archive(&[("Pkg/Launcher3Update/sdo/sdologin/sdologinentry64.dll", &dll)]);
    assert!(extract_original(zip, dll.len(), &hash).unwrap().is_none());
}

#[tokio::test]
async fn invalid_existing_backup_fails_before_network_and_is_preserved() {
    let root = std::env::temp_dir().join(format!("eorzea-sdo-invalid-{}", std::process::id()));
    std::fs::create_dir_all(root.join("sdo/sdologin")).unwrap();
    let backup = root.join(ORIGINAL);
    std::fs::write(&backup, b"do not overwrite").unwrap();
    assert!(
        prepare(&root, |_, _| panic!("must not download"))
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(backup).unwrap(), b"do not overwrite");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn atomic_deployment_never_truncates_existing_file() {
    let root = std::env::temp_dir().join(format!("eorzea-sdo-atomic-{}", std::process::id()));
    let dest = root.join("entry.dll");
    atomic_write(&dest, b"original").unwrap();
    atomic_write(&dest, b"replacement").unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"replacement");
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    std::fs::remove_dir_all(root).unwrap();
}

/// 手工提供已下载样本，只在隔离临时目录验证完整准备过程，不执行 DLL/游戏。
#[tokio::test]
#[ignore = "requires EORZEA_SDO_ORIGINAL_FIXTURE and EORZEA_SDO_SHIM_FIXTURE"]
async fn real_official_samples_prepare_idempotently_without_network() {
    let original = std::fs::read(std::env::var("EORZEA_SDO_ORIGINAL_FIXTURE").unwrap()).unwrap();
    let shim = std::fs::read(std::env::var("EORZEA_SDO_SHIM_FIXTURE").unwrap()).unwrap();
    pe::validate(&original).unwrap();
    pe::validate(&shim).unwrap();
    assert!(!is_shim(&original));
    assert!(md5_matches(&shim, SHIM_MD5));
    let root = std::env::temp_dir().join(format!("eorzea-sdo-samples-{}", std::process::id()));
    atomic_write(&root.join(ORIGINAL), &original).unwrap();
    atomic_write(&root.join(ENTRY), &shim).unwrap();
    for _ in 0..2 {
        prepare(&root, |_, _| {
            panic!("complete installation must not download")
        })
        .await
        .unwrap();
        validate_installation(&root).unwrap();
        assert_eq!(std::fs::read(root.join(ORIGINAL)).unwrap(), original);
    }
    let tools = root.join("isolated-tools");
    atomic_write(&tools.join("sdologinentry64.dll"), &shim).unwrap();
    // 只有原版入口时先生成备份，再使用已校验缓存部署 shim。
    let fresh = root.join("original-entry");
    atomic_write(&fresh.join(ENTRY), &original).unwrap();
    prepare_with_tools(&fresh, &tools, |_, _| {
        panic!("cached shim must not download")
    })
    .await
    .unwrap();
    validate_installation(&fresh).unwrap();
    assert_eq!(std::fs::read(fresh.join(ORIGINAL)).unwrap(), original);
    // 有效备份已存在时，即使入口是另一份有效原版，也保留先前备份。
    let mut alternate = original.clone();
    alternate.extend_from_slice(b"backup preservation fixture");
    atomic_write(&fresh.join(ENTRY), &alternate).unwrap();
    prepare_with_tools(&fresh, &tools, |_, _| {
        panic!("cached shim must not download")
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read(fresh.join(ORIGINAL)).unwrap(), original);
    assert_eq!(std::fs::read(fresh.join(ENTRY)).unwrap(), shim);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn concurrent_prepare_is_rejected_before_downloading() {
    let root = std::env::temp_dir().join(format!("eorzea-sdo-locked-{}", std::process::id()));
    std::fs::create_dir_all(root.join(".eorzea")).unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join(".eorzea/sdo-entry.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(
        prepare(&root, |_, _| panic!("must not download"))
            .await
            .is_err()
    );
    drop(lock);
    std::fs::remove_dir_all(root).unwrap();
}

/// 官方下载链集成测试；只准备隔离目录，不执行任何下载的文件。
#[tokio::test]
#[ignore = "downloads official ZIP packages and pinned shim into an isolated directory"]
async fn official_prepare_fresh_directory_without_executing_game() {
    let root = std::env::temp_dir().join(format!("eorzea-sdo-official-{}", std::process::id()));
    let game = root.join("game-install");
    let tools = root.join("tools");
    assert!(!root.exists(), "refuse to reuse an unknown test directory");
    let mut downloaded = 0;
    prepare_with_tools(&game, &tools, |done, _| downloaded = downloaded.max(done))
        .await
        .unwrap();
    assert!(downloaded > 0);
    validate_installation(&game).unwrap();
    let original = std::fs::read(game.join(ORIGINAL)).unwrap();
    prepare_with_tools(&game, &tools, |_, _| {
        panic!("idempotent prepare must not download")
    })
    .await
    .unwrap();
    assert_eq!(std::fs::read(game.join(ORIGINAL)).unwrap(), original);
    assert_eq!(
        std::fs::read_dir(game.join("sdo/sdologin"))
            .unwrap()
            .count(),
        2
    );
    std::fs::remove_dir_all(root).unwrap();
}
