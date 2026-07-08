use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::CloudflareConfig;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CloudflareBindingRegistry {
    kv_namespaces: Vec<KvNamespaceBinding>,
    durable_objects: Vec<DurableObjectBinding>,
    d1_databases: Vec<D1DatabaseBinding>,
    r2_buckets: Vec<R2BucketBinding>,
}

impl CloudflareBindingRegistry {
    pub fn new(
        kv_namespaces: Vec<KvNamespaceBinding>,
        durable_objects: Vec<DurableObjectBinding>,
        d1_databases: Vec<D1DatabaseBinding>,
        r2_buckets: Vec<R2BucketBinding>,
    ) -> Self {
        Self {
            kv_namespaces,
            durable_objects,
            d1_databases,
            r2_buckets,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.kv_namespaces.is_empty()
            && self.durable_objects.is_empty()
            && self.d1_databases.is_empty()
            && self.r2_buckets.is_empty()
    }

    pub fn kv_namespaces(&self) -> &[KvNamespaceBinding] {
        &self.kv_namespaces
    }

    pub fn durable_objects(&self) -> &[DurableObjectBinding] {
        &self.durable_objects
    }

    pub fn d1_databases(&self) -> &[D1DatabaseBinding] {
        &self.d1_databases
    }

    pub fn r2_buckets(&self) -> &[R2BucketBinding] {
        &self.r2_buckets
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct KvNamespaceBinding {
    pub binding: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub preview_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct DurableObjectBinding {
    pub name: String,
    pub class_name: String,
    #[serde(default)]
    pub script_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct D1DatabaseBinding {
    pub binding: String,
    #[serde(default)]
    pub database_name: Option<String>,
    #[serde(default)]
    pub database_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct R2BucketBinding {
    pub binding: String,
    #[serde(default)]
    pub bucket_name: Option<String>,
    #[serde(default)]
    pub preview_bucket_name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum WranglerConfigError {
    #[error("failed to read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse {} as JSON wrangler config: {source}", path.display())]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("failed to parse {} as TOML wrangler config: {source}", path.display())]
    Toml {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("unsupported wrangler config file {}", path.display())]
    UnsupportedFile { path: PathBuf },
}

impl CloudflareConfig {
    pub fn from_app_dir(app_dir: impl AsRef<Path>) -> Result<Self, WranglerConfigError> {
        let app_dir = app_dir.as_ref();
        for file_name in ["wrangler.jsonc", "wrangler.json", "wrangler.toml"] {
            let path = app_dir.join(file_name);
            if path.is_file() {
                return Self::from_wrangler_file(path);
            }
        }
        Ok(Self::default())
    }

    pub fn from_wrangler_file(path: impl AsRef<Path>) -> Result<Self, WranglerConfigError> {
        let path = path.as_ref();
        let source = fs::read_to_string(path).map_err(|source| WranglerConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("json") | Some("jsonc") => {
                parse_jsonc_wrangler(&source, path).map(Self::from_document)
            }
            Some("toml") => parse_toml_wrangler(&source, path).map(Self::from_document),
            _ => Err(WranglerConfigError::UnsupportedFile {
                path: path.to_path_buf(),
            }),
        }
    }

    fn from_document(document: WranglerConfigDocument) -> Self {
        Self::new(CloudflareBindingRegistry::new(
            document.kv_namespaces,
            document.durable_objects.bindings,
            document.d1_databases,
            document.r2_buckets,
        ))
    }
}

fn parse_jsonc_wrangler(
    source: &str,
    path: &Path,
) -> Result<WranglerConfigDocument, WranglerConfigError> {
    let without_comments = strip_jsonc_comments(source);
    let without_trailing_commas = strip_jsonc_trailing_commas(&without_comments);
    serde_json::from_str(&without_trailing_commas).map_err(|source| WranglerConfigError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn parse_toml_wrangler(
    source: &str,
    path: &Path,
) -> Result<WranglerConfigDocument, WranglerConfigError> {
    toml::from_str(source).map_err(|source| WranglerConfigError::Toml {
        path: path.to_path_buf(),
        source,
    })
}

#[derive(Debug, Default, Deserialize)]
struct WranglerConfigDocument {
    #[serde(default)]
    kv_namespaces: Vec<KvNamespaceBinding>,
    #[serde(default)]
    durable_objects: DurableObjectsSection,
    #[serde(default)]
    d1_databases: Vec<D1DatabaseBinding>,
    #[serde(default)]
    r2_buckets: Vec<R2BucketBinding>,
}

#[derive(Debug, Default, Deserialize)]
struct DurableObjectsSection {
    #[serde(default)]
    bindings: Vec<DurableObjectBinding>,
}

fn strip_jsonc_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while let Some(ch) = chars.next() {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
                output.push(ch);
            } else {
                output.push(' ');
            }
            continue;
        }
        if in_block_comment {
            if ch == '*' && chars.peek() == Some(&'/') {
                let _ = chars.next();
                in_block_comment = false;
                output.push(' ');
                output.push(' ');
            } else if ch == '\n' {
                output.push(ch);
            } else {
                output.push(' ');
            }
            continue;
        }
        if in_string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                output.push(ch);
            }
            '/' if chars.peek() == Some(&'/') => {
                let _ = chars.next();
                in_line_comment = true;
                output.push(' ');
                output.push(' ');
            }
            '/' if chars.peek() == Some(&'*') => {
                let _ = chars.next();
                in_block_comment = true;
                output.push(' ');
                output.push(' ');
            }
            _ => output.push(ch),
        }
    }

    output
}

fn strip_jsonc_trailing_commas(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut output = String::with_capacity(source.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        if in_string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if ch == '"' {
            in_string = true;
            output.push(ch);
            index += 1;
            continue;
        }
        if ch == ',' {
            let mut next = index + 1;
            while next < chars.len() && chars[next].is_whitespace() {
                next += 1;
            }
            if next < chars.len() && matches!(chars[next], '}' | ']') {
                index += 1;
                continue;
            }
        }
        output.push(ch);
        index += 1;
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_representative_wrangler_jsonc_bindings() {
        let source = r#"
        {
          // KV namespaces become TenantKvStore-backed bindings in CFA3.
          "kv_namespaces": [
            { "binding": "CACHE", "id": "kv-prod", "preview_id": "kv-preview", },
          ],
          "durable_objects": {
            "bindings": [
              { "name": "COUNTERS", "class_name": "Counter", "script_name": "worker", },
            ],
          },
          "d1_databases": [
            { "binding": "DB", "database_name": "app-db", "database_id": "d1-id", },
          ],
          "r2_buckets": [
            { "binding": "BUCKET", "bucket_name": "prod-bucket", "preview_bucket_name": "preview-bucket", },
          ],
        }
        "#;

        let config =
            parse_jsonc_wrangler(source, Path::new("wrangler.jsonc")).expect("jsonc parses");
        let bindings = CloudflareConfig::from_document(config).bindings().clone();

        assert_eq!(bindings.kv_namespaces()[0].binding, "CACHE");
        assert_eq!(bindings.kv_namespaces()[0].id.as_deref(), Some("kv-prod"));
        assert_eq!(bindings.durable_objects()[0].name, "COUNTERS");
        assert_eq!(bindings.durable_objects()[0].class_name, "Counter");
        assert_eq!(bindings.d1_databases()[0].binding, "DB");
        assert_eq!(bindings.r2_buckets()[0].binding, "BUCKET");
    }

    #[test]
    fn parses_representative_wrangler_toml_bindings() {
        let source = r#"
        [[kv_namespaces]]
        binding = "CACHE"
        id = "kv-prod"
        preview_id = "kv-preview"

        [durable_objects]
        bindings = [
          { name = "COUNTERS", class_name = "Counter", script_name = "worker" },
        ]

        [[d1_databases]]
        binding = "DB"
        database_name = "app-db"
        database_id = "d1-id"

        [[r2_buckets]]
        binding = "BUCKET"
        bucket_name = "prod-bucket"
        preview_bucket_name = "preview-bucket"
        "#;

        let config = parse_toml_wrangler(source, Path::new("wrangler.toml")).expect("toml parses");
        let bindings = CloudflareConfig::from_document(config).bindings().clone();

        assert_eq!(bindings.kv_namespaces()[0].binding, "CACHE");
        assert_eq!(
            bindings.durable_objects()[0].script_name.as_deref(),
            Some("worker")
        );
        assert_eq!(
            bindings.d1_databases()[0].database_id.as_deref(),
            Some("d1-id")
        );
        assert_eq!(
            bindings.r2_buckets()[0].preview_bucket_name.as_deref(),
            Some("preview-bucket")
        );
    }
}
