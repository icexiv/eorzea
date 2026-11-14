# Release publishing

Develop on `dev` and merge through a PR into `main`. PRs targeting `main` build and package App and CLI but never publish, upload to TOS, access deployment credentials or delete old artifacts. A push to `main` publishes a new rolling `dev-build`; a `v*` tag publishes a versioned release. Tags containing a hyphen are prereleases and do not replace the stable or rolling Dev manifest. Manual builds can publish Dev only from `main`.

## Independent packages

- Windows x86_64: App ZIP and CLI ZIP, each containing its fixed executable filename.
- Linux x86_64: separate App and CLI tar.gz files with executable permissions.
- macOS Apple Silicon: App DMG containing `Eorzea.app` and an Applications shortcut; CLI tar.gz. Experimental, unsigned and not notarized; failure omits macOS from that build's manifest.

Every Dev build uses a unique filename including commit, run number and attempt. Previously published manifests never silently download bytes from another build under the same URL. The initial imported Dev attachments may still be raw binaries; the manifest records their real filenames and hashes.

## Two download manifests

`downloads/github.json` and `downloads/tos.json` share schema 1 and `channels.stable` / `channels.dev`. A missing channel is `null`. Each channel records version, tag, source commit, build number, publication time and GUI assets by platform. Assets contain filename, URL, SHA-256, byte size and experimental status. CLI packages remain separately available in Releases and Shadow, not in the website's GUI selector.

The publishing job serializes updates across channels, checks GitHub attachment sizes/digests against local artifacts and commits the GitHub-only manifest to `main` first. Next it publishes packages through the pinned Shadow CLI using `--attachment`, validates refs, generates a TOS-only manifest, atomically deploys it to `https://icexiv.top/downloads/eorzea.json` and commits that manifest and refs. Failure never replaces the server manifest with an incomplete one. Older run numbers cannot overwrite newer channel state.

Shadow bodies are ignored under `published/<channel>/`; refs under `.shadow/refs/published/` are tracked. Only the selected channel's refs are replaced. After the server update and Git commit succeed, `shadow free --keep-at origin/main --delete` releases superseded objects with Shadow's default 30-day grace period. GitHub Dev assets are also retained for at least 30 days. The GitHub manifest is already updated if the independent TOS/server phase fails; rerun the failed job to retry the mirror.

The website's GitHub deployment reads the GitHub JSON from the repository; the domestic deployment reads same-origin server JSON. There is no visitor-driven GitHub API traffic.

## Credentials and server permissions

Repository Actions secrets: `TOS_ACCESS_KEY`, `TOS_SECRET_KEY`, optional `TOS_SECURITY_TOKEN`, `DEPLOY_SSH_KEY`, `DEPLOY_KNOWN_HOSTS`. `GITHUB_TOKEN` requires contents write for release assets, refs and bot commits; branch protection must permit these state commits. No keys are embedded in packages, refs or manifests.

The SSH key is independent of Market's and the homepage's. Its forced command permits only scoped rsync and `publish-manifest eorzea.json.<run-id>.tmp` in `/srv/icexiv.top/www/downloads`. The server validates the schema and TOS object paths before atomic replacement. Website deployments exclude that directory. Server setup is documented in the homepage repository's `ops/`.

## Checks

Run `python3 -m unittest discover -s scripts/tests -v` for packaging and manifest logic. Workflows are validated with actionlint. Linux, Windows and macOS native builds run in the release matrix; macOS remains optional until real-machine validation and signing are available.
