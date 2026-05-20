use std::path::Path;

/// Returns true when `dir` is a project boundary — the directory that hosts
/// a `.git` entry. The walk-ups in [`crate::dev`], [`crate::deploy`], and
/// [`crate::compose::discovery`] stop the moment they cross or land on such
/// a directory so they cannot escape the current repository.
///
/// We use [`Path::exists`] rather than [`Path::is_dir`] so a `.git` *file*
/// (as produced by `git worktree add` and submodules) still counts as a
/// boundary. Matches the convention used by git itself, `gh`, `pre-commit`,
/// `ripgrep`, and `denoland/deno/libs/config/glob/gitignore.rs:100`.
pub(crate) fn at_git_boundary(dir: &Path) -> bool {
    dir.join(".git").exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn at_git_boundary_detects_dot_git_directory() {
        let temp = tempdir().expect("tempdir should create");
        let root = temp.path();
        assert!(
            !at_git_boundary(root),
            "bare tempdir should not be a boundary"
        );
        fs::create_dir_all(root.join(".git")).expect(".git dir should create");
        assert!(at_git_boundary(root), ".git directory must be a boundary");
    }

    #[test]
    fn at_git_boundary_detects_dot_git_file_for_worktrees_and_submodules() {
        // git worktrees and submodules use a `.git` *file* containing a
        // gitdir: pointer. `Path::exists` matches both the directory and
        // file forms; `Path::is_dir` would miss this case and the walker
        // would escape into the main repository.
        let temp = tempdir().expect("tempdir should create");
        let root = temp.path();
        fs::write(root.join(".git"), "gitdir: /elsewhere\n").expect(".git file should write");
        assert!(
            at_git_boundary(root),
            ".git file (worktree pointer) must be a boundary"
        );
    }

    #[test]
    fn at_git_boundary_returns_false_for_unrelated_directory() {
        let temp = tempdir().expect("tempdir should create");
        assert!(!at_git_boundary(temp.path()));
    }
}
