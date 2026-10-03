use super::*;

impl BuildahCli {
    /// Resolve an image USER string to numeric "uid:gid" by reading
    /// /etc/passwd and /etc/group from inside the container's rootfs.
    /// Runs inside `buildah unshare` so the overlay mount is accessible.
    #[cfg(test)]
    pub(super) fn resolve_image_user(
        &self,
        session_name: &str,
        user: Option<&str>,
        rootfs: &Path,
    ) -> Result<Option<String>> {
        let Some(user) = user.map(str::trim).filter(|u| !u.is_empty()) else {
            return Ok(None);
        };

        // If the user is already fully numeric (uid or uid:gid), pass through.
        if is_numeric_user_spec(user) {
            return Ok(Some(user.to_owned()));
        }

        // Read /etc/passwd from inside the rootfs via buildah unshare to
        // resolve named users to numeric uid:gid.
        let passwd_content = self.read_rootfs_file(session_name, rootfs, "etc/passwd")?;
        let group_content = self.read_rootfs_file_optional(session_name, rootfs, "etc/group");

        resolve_user_from_content(user, &passwd_content, group_content.as_deref())
    }

    #[cfg(test)]
    pub(super) fn read_rootfs_file(
        &self,
        session_name: &str,
        rootfs: &Path,
        relative_path: &str,
    ) -> Result<String> {
        let file_path = rootfs.join(relative_path);
        let cat_command = CommandSpec::new("cat").arg(file_path.to_string_lossy().into_owned());
        let buildah = BuildahCli::new(self.path.clone()).with_unshare(self.use_unshare);
        let wrapped = buildah.wrap_unshare_with_mount(session_name, &cat_command);
        let output =
            wrapped
                .as_command()
                .output()
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to read {relative_path} from session {session_name}: {error}"
                    ),
                })?;
        if !output.status.success() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to read {relative_path} from session {session_name}: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    #[cfg(test)]
    pub(super) fn read_rootfs_file_optional(
        &self,
        session_name: &str,
        rootfs: &Path,
        relative_path: &str,
    ) -> Option<String> {
        self.read_rootfs_file(session_name, rootfs, relative_path)
            .ok()
    }
}

fn is_numeric_user_spec(user: &str) -> bool {
    let parts: Vec<&str> = user.split(':').collect();
    match parts.len() {
        1 => parts[0].parse::<u32>().is_ok(),
        2 => parts[0].parse::<u32>().is_ok() && parts[1].parse::<u32>().is_ok(),
        _ => false,
    }
}

fn resolve_user_from_content(
    user: &str,
    passwd_content: &str,
    group_content: Option<&str>,
) -> Result<Option<String>> {
    let (user_part, group_part) = match user.split_once(':') {
        Some((u, g)) => (u.trim(), Some(g.trim())),
        None => (user.trim(), None),
    };

    // Resolve user part
    let (uid, default_gid) = if let Ok(uid) = user_part.parse::<u32>() {
        let gid = find_passwd_gid_by_uid(passwd_content, uid);
        (uid, gid.unwrap_or(0))
    } else {
        let entry = find_passwd_entry_by_name(passwd_content, user_part).ok_or_else(|| {
            SandboxError::InvalidSpec {
                message: format!(
                    "image user {user:?} references user {user_part:?} not found in /etc/passwd"
                ),
            }
        })?;
        (entry.0, entry.1)
    };

    // Resolve group part
    let gid = match group_part {
        Some(g) if !g.is_empty() => {
            if let Ok(gid) = g.parse::<u32>() {
                gid
            } else {
                find_group_gid_by_name(group_content.unwrap_or(""), g).ok_or_else(|| {
                    SandboxError::InvalidSpec {
                        message: format!(
                            "image user {user:?} references group {g:?} not found in /etc/group"
                        ),
                    }
                })?
            }
        }
        _ => default_gid,
    };

    Ok(Some(format!("{uid}:{gid}")))
}

fn find_passwd_entry_by_name(passwd_content: &str, name: &str) -> Option<(u32, u32)> {
    for line in passwd_content.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 4
            && fields[0] == name
            && let (Ok(uid), Ok(gid)) = (fields[2].parse::<u32>(), fields[3].parse::<u32>())
        {
            return Some((uid, gid));
        }
    }
    None
}

fn find_passwd_gid_by_uid(passwd_content: &str, uid: u32) -> Option<u32> {
    for line in passwd_content.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 4
            && let Ok(entry_uid) = fields[2].parse::<u32>()
            && entry_uid == uid
        {
            return fields[3].parse::<u32>().ok();
        }
    }
    None
}

fn find_group_gid_by_name(group_content: &str, name: &str) -> Option<u32> {
    for line in group_content.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 3 && fields[0] == name {
            return fields[2].parse::<u32>().ok();
        }
    }
    None
}

pub(crate) fn resolve_image_user_from_rootfs(
    rootfs: &Path,
    user: Option<&str>,
) -> Result<Option<String>> {
    let Some(user) = user.map(str::trim).filter(|u| !u.is_empty()) else {
        return Ok(None);
    };

    if is_numeric_user_spec(user) {
        return Ok(Some(user.to_owned()));
    }

    let passwd_content = read_host_rootfs_file(rootfs, "etc/passwd")?;
    let group_content = read_host_rootfs_file(rootfs, "etc/group").ok();
    resolve_user_from_content(user, &passwd_content, group_content.as_deref())
}

fn read_host_rootfs_file(rootfs: &Path, relative_path: &str) -> Result<String> {
    std::fs::read_to_string(rootfs.join(relative_path)).map_err(|error| {
        SandboxError::OperationFailed {
            message: format!(
                "failed to read {relative_path} from extracted rootfs {}: {error}",
                rootfs.display()
            ),
        }
    })
}
