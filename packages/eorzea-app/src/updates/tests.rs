use super::*;

fn manifest(source: Source) -> Manifest {
    let mut manifest: Manifest =
        serde_json::from_str(include_str!("../../../../downloads/github.json")).unwrap();
    manifest.source = source.key().into();
    let release = manifest.channels.dev.as_mut().unwrap();
    release.version = "dev-3cda98e-7-1".into();
    release.commit = "3cda98ed3e0d9778d1d3edd14e0cf3c911ab4faa".into();
    release.build = 7;
    manifest
}
fn dev_info() -> BuildInfo {
    BuildInfo {
        version: "dev-3cda98e-7-1",
        commit: Some("3cda98ed3e0d9778d1d3edd14e0cf3c911ab4faa"),
        channel: Some(Channel::Dev),
        build: Some(7),
        attempt: Some(1),
        distribution: crate::build_info::Distribution::Portable,
    }
}
#[test]
fn actual_manifests_are_accepted_and_absent_stable_is_not_latest() {
    let github: Manifest =
        serde_json::from_str(include_str!("../../../../downloads/github.json")).unwrap();
    github.validate(Source::Github).unwrap();
    let mirror: Manifest =
        serde_json::from_str(include_str!("../../../../downloads/tos.json")).unwrap();
    mirror.validate(Source::Mirror).unwrap();
    let mut unpublished = manifest(Source::Github);
    unpublished.channels.stable = None;
    let result = unpublished.result(
        &dev_info(),
        Channel::Stable,
        Some("linux-x86_64"),
        Source::Github,
        false,
    );
    assert!(result.release.is_none());
    assert!(result.message().contains("尚未发布"));
}
#[test]
fn dev_order_uses_run_and_attempt_not_hash_or_lexicographic_version() {
    let release = manifest(Source::Github).channels.dev.unwrap();
    let mut info = dev_info();
    assert_eq!(compare(&info, Channel::Dev, &release), Comparison::Current);
    info.build = Some(6);
    assert_eq!(compare(&info, Channel::Dev, &release), Comparison::Newer);
    info.build = Some(10);
    assert_eq!(compare(&info, Channel::Dev, &release), Comparison::Older);
    info.build = Some(7);
    info.attempt = Some(2);
    assert_eq!(compare(&info, Channel::Dev, &release), Comparison::Older);
    info.build = None;
    info.commit = Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(
        compare(&info, Channel::Dev, &release),
        Comparison::Different
    );
    info.channel = Some(Channel::Stable);
    info.build = Some(1);
    assert_eq!(
        compare(&info, Channel::Dev, &release),
        Comparison::Different
    );
}
#[test]
fn stable_semver_and_source_comparison_are_conservative() {
    let mut release = manifest(Source::Github).channels.dev.unwrap();
    release.version = "v0.10.0".into();
    release.tag = release.version.clone();
    let mut info = dev_info();
    info.channel = Some(Channel::Stable);
    info.version = "v0.9.0";
    assert_eq!(compare(&info, Channel::Stable, &release), Comparison::Newer);
    info.version = "0.10.0+local";
    assert_eq!(
        compare(&info, Channel::Stable, &release),
        Comparison::Current
    );
    info.version = "0.11.0";
    assert_eq!(compare(&info, Channel::Stable, &release), Comparison::Older);
    info.channel = None;
    info.commit = None;
    assert_eq!(
        compare(&info, Channel::Stable, &release),
        Comparison::Different
    );
}
#[test]
fn malformed_metadata_is_rejected_before_opening_links() {
    for change in 0..6 {
        let mut m = manifest(Source::Github);
        let release = m.channels.dev.as_mut().unwrap();
        match change {
            0 => m.schema = 2,
            1 => m.source = "tos".into(),
            2 => release.tag = "../../malicious".into(),
            3 => release.build += 1,
            4 => release.assets.values_mut().next().unwrap().url = "file:///etc/passwd".into(),
            _ => release.assets.values_mut().next().unwrap().sha256 = "bad".into(),
        }
        assert!(m.validate(Source::Github).is_err());
    }
}
#[test]
fn unsupported_platform_still_discovers_upstream_without_offering_wrong_package() {
    let result =
        manifest(Source::Github).result(&dev_info(), Channel::Dev, None, Source::Github, false);
    assert!(result.release.is_some());
    assert!(!result.platform_available);
}

async fn server(body: Vec<u8>, status: &str) -> (String, tokio::task::JoinHandle<String>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/manifest.json", listener.local_addr().unwrap());
    let status = status.to_owned();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = vec![0; 8192];
        let len = socket.read(&mut request).await.unwrap();
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = socket.write_all(header.as_bytes()).await;
        let _ = socket.write_all(&body).await;
        String::from_utf8_lossy(&request[..len]).into_owned()
    });
    (url, handle)
}

#[tokio::test]
async fn fallback_fetches_only_manifest_and_cache_is_per_source_and_revalidated() {
    let (bad, bad_task) = server(b"invalid".to_vec(), "200 OK").await;
    let (good, good_task) = server(
        serde_json::to_vec(&manifest(Source::Github)).unwrap(),
        "200 OK",
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let mut checker = Checker {
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .no_proxy()
            .build()
            .unwrap(),
        endpoints: vec![(Source::Mirror, bad), (Source::Github, good)],
        cache: Some(dir.path().to_owned()),
    };
    let info = dev_info();
    let result = checker
        .check(&info, Channel::Dev, Some("linux-x86_64"), false, 100)
        .await
        .unwrap();
    assert_eq!(result.source, Source::Github);
    assert!(!result.cached);
    for task in [bad_task, good_task] {
        let request = task.await.unwrap();
        assert!(request.starts_with("GET /manifest.json "));
        assert!(!request.contains("Authorization:"));
    }
    let result = checker
        .check(&info, Channel::Dev, None, false, 101)
        .await
        .unwrap();
    assert!(result.cached);
    checker.endpoints.retain(|(s, _)| *s == Source::Github);
    // A manual check bypasses cache; offline is an error, never a false "latest" result.
    assert!(
        checker
            .check(&info, Channel::Dev, None, true, 102)
            .await
            .is_err()
    );
    assert!(
        checker
            .check(&info, Channel::Dev, None, false, 100 + CACHE_TTL)
            .await
            .is_err()
    );
    // A backwards clock invalidates cache rather than suppressing checks indefinitely.
    assert!(
        checker
            .check(&info, Channel::Dev, None, false, 99)
            .await
            .is_err()
    );
    let path = dir.path().join("github.json");
    std::fs::write(&path, b"corrupted").unwrap();
    assert!(
        checker
            .check(&info, Channel::Dev, None, false, 101)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn rejects_oversized_and_http_error_responses() {
    for (body, status) in [
        (vec![b'x'; MAX_MANIFEST + 1], "200 OK"),
        (b"oops".to_vec(), "503 Unavailable"),
    ] {
        let (url, task) = server(body, status).await;
        let checker = Checker {
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            endpoints: vec![(Source::Github, url)],
            cache: None,
        };
        assert!(
            checker
                .check(&dev_info(), Channel::Dev, None, true, 0)
                .await
                .is_err()
        );
        task.await.unwrap();
    }
}

#[tokio::test]
async fn unpublished_channel_never_silently_falls_back_or_switches_channel() {
    let mut mirror = manifest(Source::Mirror);
    mirror.channels.stable = None;
    let (url, task) = server(serde_json::to_vec(&mirror).unwrap(), "200 OK").await;
    let checker = Checker {
        client: reqwest::Client::builder().no_proxy().build().unwrap(),
        endpoints: vec![
            (Source::Mirror, url),
            (Source::Github, "invalid URL".into()),
        ],
        cache: None,
    };
    let result = checker
        .check(&dev_info(), Channel::Stable, None, true, 0)
        .await
        .unwrap();
    assert_eq!(result.source, Source::Mirror);
    assert!(result.release.is_none());
    assert!(result.message().contains("尚未发布"));
    task.await.unwrap();
}
