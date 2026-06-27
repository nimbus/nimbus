use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    Restricted,
    Standard,
    Privileged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLanguage {
    JavaScript,
    WasmComponent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePreset {
    Application,
    Tooling,
    Oracle,
    Operator,
    Code,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct RuntimeGrants {
    pub read: Vec<String>,
    pub write: Vec<String>,
    pub net_connect: Vec<String>,
    pub net_listen: Vec<String>,
    pub env_read: Vec<String>,
    pub env_write: Vec<String>,
    pub secret: Vec<String>,
    pub identity: Vec<String>,
    pub service: Vec<String>,
    pub run: Vec<String>,
    pub sys: Vec<String>,
    pub ffi: Vec<String>,
    pub worker: Vec<String>,
    pub tool: Vec<String>,
}

impl RuntimeGrants {
    pub fn restricted() -> Self {
        Self::default()
    }

    pub(crate) fn sorted_service_grants(&self) -> Vec<String> {
        let mut grants = self.service.clone();
        grants.sort();
        grants.dedup();
        grants
    }

    pub(crate) fn has_service_grants(&self) -> bool {
        !self.service.is_empty()
    }

    pub(crate) fn permits_same_process_realm_reuse(&self) -> bool {
        self.service.is_empty()
            && self.net_connect.is_empty()
            && self.net_listen.is_empty()
            && self.run.is_empty()
            && self.ffi.is_empty()
            && self.worker.is_empty()
            && self.tool.is_empty()
            && !self.sys.iter().any(|grant| grant == "inspector")
    }

    fn application_base() -> Self {
        Self {
            read: vec!["$generated_root".to_string()],
            write: vec!["$generated_root".to_string()],
            sys: vec![
                "hostname".to_string(),
                "gid".to_string(),
                "osRelease".to_string(),
                "statfs".to_string(),
                "uid".to_string(),
            ],
            ..Self::default()
        }
    }

    pub fn application_web_standard() -> Self {
        let mut grants = Self::application_base();
        grants
            .env_read
            .push("NODE_TLS_REJECT_UNAUTHORIZED".to_string());
        grants
    }

    pub fn application_node() -> Self {
        Self::application_node_production_in_process()
    }

    pub fn application_node_production_in_process() -> Self {
        Self::application_base()
    }

    pub fn application_node_local_development() -> Self {
        let mut grants = Self::application_web_standard();
        grants.net_connect = vec!["127.0.0.1".to_string(), "localhost".to_string()];
        grants.net_listen = vec![
            "127.0.0.1".to_string(),
            "localhost".to_string(),
            "0.0.0.0".to_string(),
            "[::1]".to_string(),
            "[::]".to_string(),
        ];
        grants.sys.push("inspector".to_string());
        grants.worker = vec!["thread".to_string()];
        grants
    }

    pub fn application_node_service_microvm() -> Self {
        Self::application_node_local_development()
    }

    pub fn tooling() -> Self {
        Self {
            read: vec![
                "$app_root".to_string(),
                "$generated_root".to_string(),
                "$temp_root".to_string(),
                "$cache_root".to_string(),
            ],
            write: vec![
                "$generated_root".to_string(),
                "$temp_root".to_string(),
                "$cache_root".to_string(),
            ],
            net_connect: vec!["127.0.0.1".to_string(), "localhost".to_string()],
            env_read: vec![
                "ESBUILD_BINARY_PATH".to_string(),
                "ESBUILD_MAX_BUFFER".to_string(),
                "ESBUILD_WORKER_THREADS".to_string(),
                "HOME".to_string(),
                "NODE_ENV".to_string(),
                "NODE_TLS_REJECT_UNAUTHORIZED".to_string(),
                "NODE_INSPECTOR_IPC".to_string(),
                "NODE_V8_COVERAGE".to_string(),
                "PATH".to_string(),
                "PWD".to_string(),
                "TEMP".to_string(),
                "TMP".to_string(),
                "TMPDIR".to_string(),
                "TSC_NONPOLLING_WATCHER".to_string(),
                "TSC_WATCHDIRECTORY".to_string(),
                "TSC_WATCHFILE".to_string(),
                "TSC_WATCH_POLLINGCHUNKSIZE".to_string(),
                "TSC_WATCH_POLLINGCHUNKSIZE_HIGH".to_string(),
                "TSC_WATCH_POLLINGCHUNKSIZE_LOW".to_string(),
                "TSC_WATCH_POLLINGCHUNKSIZE_MEDIUM".to_string(),
                "TSC_WATCH_POLLINGINTERVAL".to_string(),
                "TSC_WATCH_POLLINGINTERVAL_HIGH".to_string(),
                "TSC_WATCH_POLLINGINTERVAL_LOW".to_string(),
                "TSC_WATCH_POLLINGINTERVAL_MEDIUM".to_string(),
                "TSC_WATCH_UNCHANGEDPOLLTHRESHOLDS".to_string(),
                "TSC_WATCH_UNCHANGEDPOLLTHRESHOLDS_HIGH".to_string(),
                "TSC_WATCH_UNCHANGEDPOLLTHRESHOLDS_LOW".to_string(),
                "TSC_WATCH_UNCHANGEDPOLLTHRESHOLDS_MEDIUM".to_string(),
                "VSCODE_INSPECTOR_OPTIONS".to_string(),
                "npm_config_cache".to_string(),
                "npm_config_user_agent".to_string(),
                "npm_execpath".to_string(),
            ],
            run: vec![
                "$discovered_tooling".to_string(),
                "$runtime_self_exec".to_string(),
                "$runtime_host_exec".to_string(),
            ],
            worker: vec!["thread".to_string()],
            sys: vec![
                "hostname".to_string(),
                "gid".to_string(),
                "statfs".to_string(),
                "uid".to_string(),
                "inspector".to_string(),
            ],
            ..Self::default()
        }
    }
}

pub(super) fn validate_mode_grant_ceiling(mode: RuntimeMode, grants: &RuntimeGrants) {
    match mode {
        RuntimeMode::Restricted => {
            assert_grant_family_empty(mode, "read", &grants.read);
            assert_grant_family_empty(mode, "write", &grants.write);
            assert_grant_family_empty(mode, "net_connect", &grants.net_connect);
            assert_grant_family_empty(mode, "net_listen", &grants.net_listen);
            assert_grant_family_empty(mode, "env_read", &grants.env_read);
            assert_grant_family_empty(mode, "env_write", &grants.env_write);
            assert_grant_family_empty(mode, "secret", &grants.secret);
            assert_grant_family_empty(mode, "identity", &grants.identity);
            assert_grant_family_empty(mode, "service", &grants.service);
            assert_grant_family_empty(mode, "run", &grants.run);
            assert_grant_family_empty(mode, "sys", &grants.sys);
            assert_grant_family_empty(mode, "ffi", &grants.ffi);
            assert_grant_family_empty(mode, "worker", &grants.worker);
            assert_grant_family_empty(mode, "tool", &grants.tool);
        }
        RuntimeMode::Standard => {
            assert_grant_family_empty(mode, "ffi", &grants.ffi);
        }
        RuntimeMode::Privileged => {}
    }
}

fn assert_grant_family_empty(mode: RuntimeMode, family: &str, grants: &[String]) {
    assert!(
        grants.is_empty(),
        "{family} grants exceed the {mode:?} runtime mode ceiling"
    );
}
