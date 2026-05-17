use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use semver::Version;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::system::cache;
use crate::system::install_method::{UpgradeAction, detect_install_method};

const DEFAULT_RELEASES_URL: &str = "https://api.github.com/repos/nimbus/nimbus/releases/latest";
const TTL_24H: Duration = Duration::from_secs(24 * 60 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CheckStatus {
    Never,
    Fresh,
    Stale,
    Error,
    Disabled,
}

#[derive(Clone)]
pub(crate) struct VersionCheckConfig {
    pub cache_path: Option<PathBuf>,
    pub releases_url: String,
    pub user_agent: String,
    pub ttl: Duration,
    pub disabled: bool,
    pub host_label: String,
    pub current_exe: Option<PathBuf>,
}

impl VersionCheckConfig {
    pub fn from_env(current: &Version) -> Self {
        let disabled = std::env::var_os("NIMBUS_DISABLE_UPDATE_CHECK")
            .map(|v| v == "1")
            .unwrap_or(false);
        Self {
            cache_path: cache::update_check_cache_path(),
            releases_url: DEFAULT_RELEASES_URL.to_owned(),
            user_agent: format!("nimbus/{current}"),
            ttl: TTL_24H,
            disabled,
            host_label: get_hostname(),
            current_exe: std::env::current_exe().ok(),
        }
    }
}

pub(crate) struct VersionCheck {
    current: Version,
    upgrade: UpgradeAction,
    inner: Arc<RwLock<State>>,
    refresh_in_flight: Arc<AtomicBool>,
    config: VersionCheckConfig,
}

#[derive(Debug, Default, Clone)]
struct State {
    cached: Option<CachedLatest>,
    last_checked_at: Option<OffsetDateTime>,
    last_status: CheckStatusInternal,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum CheckStatusInternal {
    #[default]
    Never,
    Ok,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedLatest {
    version: String,
    tag: String,
    url: String,
    #[serde(rename = "publishedAt")]
    published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OnDiskCache {
    cached: Option<CachedLatest>,
    #[serde(rename = "lastCheckedAt")]
    last_checked_at: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct VersionSnapshot {
    pub current: Version,
    pub latest: Option<Version>,
    pub url: Option<String>,
    pub published_at: Option<String>,
    pub host: String,
    pub check_status: CheckStatus,
    pub upgrade: UpgradeAction,
}

impl VersionCheck {
    pub fn new(current: Version, config: VersionCheckConfig) -> Arc<Self> {
        let upgrade = match config.current_exe.as_deref() {
            Some(path) => detect_install_method(path),
            None => UpgradeAction {
                method: crate::system::install_method::InstallMethod::Unknown,
                command: None,
                needs_sudo: false,
                interactive: false,
                fallback_url: "https://github.com/nimbus/nimbus#install",
            },
        };

        let mut state = State::default();
        if !config.disabled
            && let Some(path) = config.cache_path.as_deref()
            && let Some(loaded) = load_cache(path)
        {
            state.cached = loaded.cached;
            state.last_checked_at = loaded.last_checked_at.as_deref().and_then(parse_rfc3339);
            if state.cached.is_some() {
                state.last_status = CheckStatusInternal::Ok;
            }
        }

        Arc::new(Self {
            current,
            upgrade,
            inner: Arc::new(RwLock::new(state)),
            refresh_in_flight: Arc::new(AtomicBool::new(false)),
            config,
        })
    }

    pub async fn snapshot(self: &Arc<Self>) -> VersionSnapshot {
        if self.config.disabled {
            return VersionSnapshot {
                current: self.current.clone(),
                latest: None,
                url: None,
                published_at: None,
                host: self.config.host_label.clone(),
                check_status: CheckStatus::Disabled,
                upgrade: self.upgrade.clone(),
            };
        }

        let state = self.inner.read().await.clone();
        let now = OffsetDateTime::now_utc();
        let age = state
            .last_checked_at
            .map(|ts| now - ts)
            .map(|d| d.unsigned_abs())
            .unwrap_or(Duration::MAX);
        let ttl = self.config.ttl;

        let check_status = match state.last_status {
            CheckStatusInternal::Never => CheckStatus::Never,
            CheckStatusInternal::Ok if age <= ttl => CheckStatus::Fresh,
            CheckStatusInternal::Ok => CheckStatus::Stale,
            CheckStatusInternal::Error => CheckStatus::Error,
        };

        let needs_refresh = matches!(
            check_status,
            CheckStatus::Never | CheckStatus::Stale | CheckStatus::Error
        );
        if needs_refresh {
            self.clone().try_spawn_refresh();
        }

        let (latest, url, published_at) = match state.cached.as_ref() {
            Some(c) => (
                Version::parse(&c.version).ok(),
                Some(c.url.clone()),
                c.published_at.clone(),
            ),
            None => (None, None, None),
        };

        VersionSnapshot {
            current: self.current.clone(),
            latest,
            url,
            published_at,
            host: self.config.host_label.clone(),
            check_status,
            upgrade: self.upgrade.clone(),
        }
    }

    fn try_spawn_refresh(self: Arc<Self>) {
        if self
            .refresh_in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            debug!("nimbus version check: refresh already in-flight; skipping spawn");
            return;
        }
        tokio::spawn(async move {
            let outcome = self.run_refresh().await;
            self.refresh_in_flight.store(false, Ordering::Release);
            match outcome {
                Ok(()) => {}
                Err(err) => info!("nimbus version check: refresh failed: {err}"),
            }
        });
    }

    /// Block-on style refresh used by tests; updates state in-place.
    #[cfg(test)]
    pub(crate) async fn refresh_blocking(self: &Arc<Self>) -> Result<(), String> {
        let _guard = InFlightGuard::acquire(&self.refresh_in_flight);
        self.run_refresh().await
    }

    async fn run_refresh(&self) -> Result<(), String> {
        let body = self.fetch_latest_release().await;
        let now = OffsetDateTime::now_utc();
        match body {
            Ok(release) => {
                let cached = CachedLatest {
                    version: release.version_str.clone(),
                    tag: release.tag.clone(),
                    url: release.html_url.clone(),
                    published_at: release.published_at.clone(),
                };
                {
                    let mut state = self.inner.write().await;
                    state.cached = Some(cached.clone());
                    state.last_checked_at = Some(now);
                    state.last_status = CheckStatusInternal::Ok;
                }
                if let Some(path) = self.config.cache_path.as_deref() {
                    let snapshot = OnDiskCache {
                        cached: Some(cached),
                        last_checked_at: format_rfc3339(now),
                    };
                    if let Err(err) = persist_cache(path, &snapshot) {
                        warn!("nimbus version check: persist cache failed: {err}");
                    }
                }
                Ok(())
            }
            Err(err) => {
                {
                    let mut state = self.inner.write().await;
                    state.last_checked_at = Some(now);
                    state.last_status = CheckStatusInternal::Error;
                }
                Err(err)
            }
        }
    }

    async fn fetch_latest_release(&self) -> Result<ReleaseInfo, String> {
        let client = reqwest::Client::builder()
            .user_agent(&self.config.user_agent)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| format!("build http client: {e}"))?;
        let response = client
            .get(&self.config.releases_url)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| format!("request: {e}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("github releases returned {status}"));
        }
        let release: GithubRelease = response
            .json()
            .await
            .map_err(|e| format!("decode json: {e}"))?;
        let tag = release.tag_name.clone();
        let version_str = tag.trim_start_matches('v').to_owned();
        // Validate that this is a parsable semver before we cache it.
        let _ = Version::parse(&version_str)
            .map_err(|e| format!("parse version from tag {tag}: {e}"))?;
        Ok(ReleaseInfo {
            tag,
            version_str,
            html_url: release.html_url,
            published_at: release.published_at,
        })
    }
}

struct ReleaseInfo {
    tag: String,
    version_str: String,
    html_url: String,
    published_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    published_at: Option<String>,
}

#[cfg(test)]
struct InFlightGuard<'a> {
    flag: &'a AtomicBool,
}

#[cfg(test)]
impl<'a> InFlightGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Self {
        flag.store(true, Ordering::Release);
        Self { flag }
    }
}

#[cfg(test)]
impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}

fn load_cache(path: &std::path::Path) -> Option<OnDiskCache> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn persist_cache(path: &std::path::Path, cache: &OnDiskCache) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(cache).map_err(|e| format!("serialize: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

fn parse_rfc3339(s: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(s, &Rfc3339).ok()
}

fn format_rfc3339(ts: OffsetDateTime) -> Option<String> {
    ts.format(&Rfc3339).ok()
}

#[cfg(unix)]
fn get_hostname() -> String {
    let mut buf = [0u8; 256];
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if rc == 0 {
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        return String::from_utf8_lossy(&buf[..len]).into_owned();
    }
    "localhost".to_owned()
}

#[cfg(windows)]
fn get_hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn config_with(
        cache_path: Option<PathBuf>,
        releases_url: String,
        disabled: bool,
    ) -> VersionCheckConfig {
        VersionCheckConfig {
            cache_path,
            releases_url,
            user_agent: "nimbus/test".to_owned(),
            ttl: Duration::from_secs(60),
            disabled,
            host_label: "test-host".to_owned(),
            current_exe: Some(PathBuf::from("/opt/homebrew/bin/nimbus")),
        }
    }

    #[tokio::test]
    async fn first_snapshot_is_never_when_no_cache() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("update-check.json");
        let check = VersionCheck::new(
            Version::parse("0.1.31").unwrap(),
            config_with(Some(cache_path), "http://127.0.0.1:1/".to_owned(), false),
        );
        let snap = check.snapshot().await;
        assert!(matches!(snap.check_status, CheckStatus::Never));
        assert!(snap.latest.is_none());
        assert_eq!(snap.current, Version::parse("0.1.31").unwrap());
        assert_eq!(snap.upgrade.method.as_str(), "brew");
    }

    #[tokio::test]
    async fn disabled_short_circuits_everything() {
        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("update-check.json");
        // pre-populate disk to prove disabled path doesn't read it
        std::fs::create_dir_all(dir.path()).unwrap();
        let prior = OnDiskCache {
            cached: Some(CachedLatest {
                version: "9.9.9".into(),
                tag: "v9.9.9".into(),
                url: "https://example.com".into(),
                published_at: None,
            }),
            last_checked_at: Some("2026-05-14T00:00:00Z".to_owned()),
        };
        std::fs::write(&cache_path, serde_json::to_vec(&prior).unwrap()).unwrap();

        let check = VersionCheck::new(
            Version::parse("0.1.31").unwrap(),
            config_with(
                Some(cache_path.clone()),
                "http://127.0.0.1:1/".to_owned(),
                true,
            ),
        );
        let snap = check.snapshot().await;
        assert!(matches!(snap.check_status, CheckStatus::Disabled));
        assert!(snap.latest.is_none());
    }

    #[tokio::test]
    async fn fresh_after_successful_refresh() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = serde_json::json!({
            "tag_name": "v0.1.41",
            "html_url": "https://github.com/nimbus/nimbus/releases/tag/v0.1.41",
            "published_at": "2026-05-14T18:22:00Z",
        });
        Mock::given(method("GET"))
            .and(path("/repos/nimbus/nimbus/releases/latest"))
            .and(header("User-Agent", "nimbus/test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("update-check.json");
        let url = format!("{}/repos/nimbus/nimbus/releases/latest", server.uri());

        let check = VersionCheck::new(
            Version::parse("0.1.31").unwrap(),
            config_with(Some(cache_path.clone()), url, false),
        );

        check.refresh_blocking().await.expect("refresh");
        let snap = check.snapshot().await;

        assert!(matches!(snap.check_status, CheckStatus::Fresh));
        assert_eq!(snap.latest, Some(Version::parse("0.1.41").unwrap()));
        assert!(snap.latest.as_ref().unwrap() > &snap.current);
        assert_eq!(
            snap.url.as_deref(),
            Some("https://github.com/nimbus/nimbus/releases/tag/v0.1.41")
        );
        assert!(cache_path.exists(), "cache file should be persisted");
    }

    #[tokio::test]
    async fn stale_after_ttl_expires() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/nimbus/nimbus/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v0.1.41",
                "html_url": "https://example.com/v0.1.41",
                "published_at": null,
            })))
            .mount(&server)
            .await;

        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("update-check.json");
        let url = format!("{}/repos/nimbus/nimbus/releases/latest", server.uri());

        let mut cfg = config_with(Some(cache_path.clone()), url, false);
        cfg.ttl = Duration::from_millis(50);
        let check = VersionCheck::new(Version::parse("0.1.31").unwrap(), cfg);
        check.refresh_blocking().await.expect("refresh");

        // Wait past TTL.
        tokio::time::sleep(Duration::from_millis(120)).await;
        let snap = check.snapshot().await;
        assert!(matches!(snap.check_status, CheckStatus::Stale));
        // Stale still surfaces the cached value, not null.
        assert_eq!(snap.latest, Some(Version::parse("0.1.41").unwrap()));
    }

    #[tokio::test]
    async fn error_path_preserves_last_good_cache() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/nimbus/nimbus/releases/latest"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let dir = tempdir().unwrap();
        let cache_path = dir.path().join("update-check.json");
        // Pre-populate the disk cache with a known-good value.
        std::fs::create_dir_all(dir.path()).unwrap();
        let prior = OnDiskCache {
            cached: Some(CachedLatest {
                version: "0.1.40".into(),
                tag: "v0.1.40".into(),
                url: "https://example.com/v0.1.40".into(),
                published_at: None,
            }),
            last_checked_at: format_rfc3339(OffsetDateTime::now_utc()),
        };
        std::fs::write(&cache_path, serde_json::to_vec(&prior).unwrap()).unwrap();

        let url = format!("{}/repos/nimbus/nimbus/releases/latest", server.uri());
        let mut cfg = config_with(Some(cache_path.clone()), url, false);
        cfg.ttl = Duration::from_millis(10);
        let check = VersionCheck::new(Version::parse("0.1.31").unwrap(), cfg);

        // First snapshot returns Fresh from disk.
        let snap = check.snapshot().await;
        assert!(matches!(snap.check_status, CheckStatus::Fresh));

        tokio::time::sleep(Duration::from_millis(40)).await;
        // Now a refresh attempt should fail (503), preserving the cached value.
        let _ = check.refresh_blocking().await; // returns Err but we don't unwrap
        let snap = check.snapshot().await;
        assert!(matches!(snap.check_status, CheckStatus::Error));
        assert_eq!(snap.latest, Some(Version::parse("0.1.40").unwrap()));
    }

    #[tokio::test]
    async fn semver_comparison_handles_v_prefix() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/nimbus/nimbus/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v0.1.31",
                "html_url": "https://example.com/v0.1.31",
                "published_at": null,
            })))
            .mount(&server)
            .await;

        let dir = tempdir().unwrap();
        let url = format!("{}/repos/nimbus/nimbus/releases/latest", server.uri());
        let check = VersionCheck::new(
            Version::parse("0.1.31").unwrap(),
            config_with(Some(dir.path().join("c.json")), url, false),
        );
        check.refresh_blocking().await.expect("refresh");
        let snap = check.snapshot().await;
        assert_eq!(snap.latest, Some(Version::parse("0.1.31").unwrap()));
        assert!(
            snap.latest.as_ref().unwrap() <= &snap.current,
            "equal versions are not available"
        );
    }
}
