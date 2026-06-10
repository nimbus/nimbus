use crate::spec::{SandboxProcessSpec, SandboxRootSpec, SandboxRootfsSpec};

pub(crate) fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

pub(crate) fn resolve_root_spec(
    root: &SandboxRootSpec,
    defaults: &SandboxRootfsSpec,
) -> SandboxRootSpec {
    match root {
        SandboxRootSpec::Rootfs(rootfs) if !rootfs.is_unspecified() => {
            SandboxRootSpec::Rootfs(rootfs.clone())
        }
        SandboxRootSpec::Rootfs(rootfs) => {
            let mut resolved = defaults.clone();
            resolved.readonly = resolved.readonly || rootfs.readonly;
            SandboxRootSpec::Rootfs(resolved)
        }
        SandboxRootSpec::OciImage(_) => SandboxRootSpec::Rootfs(defaults.clone()),
    }
}

pub(crate) fn resolve_process_spec(
    spec: &SandboxProcessSpec,
    defaults: &SandboxProcessSpec,
) -> SandboxProcessSpec {
    let mut resolved = defaults.clone();

    if !spec.args.is_empty() {
        resolved.args = spec.args.clone();
    }

    resolved.env = if spec.env.is_empty() || spec.uses_default_env() {
        defaults.env.clone()
    } else {
        merge_env_overrides(&defaults.env, &spec.env)
    };

    if !spec.uses_default_cwd() {
        resolved.cwd = spec.cwd.clone();
    }

    resolved.terminal = spec.terminal || defaults.terminal;
    resolved
}

pub(crate) fn merge_env_overrides(base: &[String], overrides: &[String]) -> Vec<String> {
    let mut merged = base.to_vec();
    for override_entry in overrides {
        let Some(override_key) = env_key(override_entry) else {
            merged.push(override_entry.clone());
            continue;
        };

        if let Some(index) = merged
            .iter()
            .position(|entry| env_key(entry).is_some_and(|key| key == override_key))
        {
            merged[index] = override_entry.clone();
        } else {
            merged.push(override_entry.clone());
        }
    }
    merged
}

pub(crate) fn env_key(entry: &str) -> Option<&str> {
    let (key, _) = entry.split_once('=')?;
    (!key.is_empty()).then_some(key)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn slugify_normalizes_operator_facing_names() {
        assert_eq!(slugify("Postgres Primary"), "postgres-primary");
        assert_eq!(slugify("db__1"), "db-1");
        assert_eq!(slugify("api@edge"), "api-edge");
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn resolve_root_spec_uses_image_defaults_and_preserves_explicit_rootfs() {
        let defaults = SandboxRootfsSpec::new("/image/rootfs").read_only(true);
        let image_root = resolve_root_spec(
            &SandboxRootSpec::oci_image_reference("registry.example/app:latest"),
            &defaults,
        );
        assert_eq!(image_root, SandboxRootSpec::Rootfs(defaults.clone()));

        let explicit = SandboxRootSpec::rootfs("/explicit/rootfs");
        assert_eq!(resolve_root_spec(&explicit, &defaults), explicit);

        let readonly_unspecified =
            SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("").read_only(true));
        assert_eq!(
            resolve_root_spec(
                &readonly_unspecified,
                &SandboxRootfsSpec::new("/image/rootfs")
            ),
            SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/image/rootfs").read_only(true))
        );
    }

    #[test]
    fn resolve_process_spec_merges_overrides_without_losing_defaults() {
        let defaults = SandboxProcessSpec::new(["/bin/server"])
            .with_env(["PATH=/bin", "MODE=prod", "KEEP=yes"])
            .with_cwd("/srv")
            .with_terminal(true);
        let requested = SandboxProcessSpec::new(["/custom"])
            .with_env(["MODE=dev", "EXTRA=1", "INVALID"])
            .with_cwd("/app");

        let resolved = resolve_process_spec(&requested, &defaults);

        assert_eq!(resolved.args, ["/custom"]);
        assert_eq!(
            resolved.env,
            ["PATH=/bin", "MODE=dev", "KEEP=yes", "EXTRA=1", "INVALID"]
        );
        assert_eq!(resolved.cwd, Path::new("/app"));
        assert!(resolved.terminal);
    }

    #[test]
    fn resolve_process_spec_keeps_default_env_when_request_does_not_override() {
        let defaults = SandboxProcessSpec::new(["/bin/server"]).with_env(["PATH=/image"]);
        let requested = SandboxProcessSpec::new(Vec::<String>::new());

        let resolved = resolve_process_spec(&requested, &defaults);

        assert_eq!(resolved.args, ["/bin/server"]);
        assert_eq!(resolved.env, ["PATH=/image"]);
    }
}
