use super::*;

pub(super) fn shell_escape(s: &str) -> String {
    if s.is_empty() {
        return "''".to_owned();
    }
    if s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'/' || b == b'.')
    {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
pub(super) fn localhost_image_reference(image_name: &str) -> String {
    format!("localhost/{image_name}")
}

pub(super) fn display_command(
    command: &CommandSpec,
    needs_unshare: bool,
    buildah: &BuildahCli,
) -> String {
    let rendered = if needs_unshare {
        buildah.wrap_unshare(command)
    } else {
        command.clone()
    };
    let mut parts = vec![rendered.program.display().to_string()];
    parts.extend(rendered.args);
    parts.join(" ")
}
