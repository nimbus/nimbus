use std::path::{Path, PathBuf};

use super::command::CommandSpec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildahBinary {
    path: PathBuf,
}

impl BuildahBinary {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn wrap_unshare(&self, command: &CommandSpec) -> CommandSpec {
        let program = command.program.to_string_lossy().into_owned();
        CommandSpec::new(self.path.clone())
            .arg("unshare")
            .arg("--")
            .arg(program)
            .args(command.args.iter().cloned())
    }

    pub fn from_image_command(&self, container_name: &str, image_reference: &str) -> CommandSpec {
        CommandSpec::new(self.path.clone())
            .arg("from")
            .arg("--name")
            .arg(container_name.to_owned())
            .arg(image_reference.to_owned())
    }

    pub fn mount_command(&self, container_name: &str) -> CommandSpec {
        CommandSpec::new(self.path.clone())
            .arg("mount")
            .arg(container_name.to_owned())
    }

    pub fn inspect_command(&self, container_name: &str) -> CommandSpec {
        CommandSpec::new(self.path.clone())
            .arg("inspect")
            .arg("--format")
            .arg("json")
            .arg(container_name.to_owned())
    }

    pub fn remove_command(&self, container_name: &str) -> CommandSpec {
        CommandSpec::new(self.path.clone())
            .arg("rm")
            .arg(container_name.to_owned())
    }

    pub fn build_command(
        &self,
        tag: &str,
        dockerfile_path: &Path,
        context_path: &Path,
    ) -> CommandSpec {
        CommandSpec::new(self.path.clone())
            .arg("bud")
            .arg("-t")
            .arg(tag.to_owned())
            .arg("-f")
            .arg(dockerfile_path.to_string_lossy().into_owned())
            .arg(context_path.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::BuildahBinary;
    use crate::backends::krun::command::CommandSpec;

    #[test]
    fn wrap_unshare_prefixes_existing_command() {
        let buildah = BuildahBinary::new("buildah");
        let wrapped = buildah.wrap_unshare(
            &CommandSpec::new("/usr/libexec/neovex/crun")
                .arg("state")
                .arg("sandbox-123"),
        );

        assert_eq!(wrapped.program, PathBuf::from("buildah"));
        assert_eq!(
            wrapped.args,
            vec![
                "unshare",
                "--",
                "/usr/libexec/neovex/crun",
                "state",
                "sandbox-123",
            ]
        );
    }

    #[test]
    fn build_command_matches_expected_shape() {
        let buildah = BuildahBinary::new("/usr/bin/buildah");
        let command = buildah.build_command(
            "neovex-test",
            Path::new("/workspace/Dockerfile"),
            Path::new("/workspace"),
        );

        assert_eq!(command.program, PathBuf::from("/usr/bin/buildah"));
        assert_eq!(
            command.args,
            vec![
                "bud",
                "-t",
                "neovex-test",
                "-f",
                "/workspace/Dockerfile",
                "/workspace",
            ]
        );
    }
}
