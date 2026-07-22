use std::path::{Path, PathBuf};

const BANNED_AMBIENT_MINTS: [(&str, &str); 6] = [
    // Direct system-time reads bypass the injected WallClock.
    ("SystemTime::now(", "bypasses the injected WallClock"),
    // Direct ULID generation bypasses the injected IdSource.
    ("Ulid::new(", "bypasses the injected IdSource"),
    // Timestamp's convenience constructor reads ambient wall-clock time.
    ("Timestamp::now(", "reads ambient wall-clock time"),
    // DocumentId's constructor mints an ambient ULID.
    ("DocumentId::new(", "mints an ambient document ULID"),
    // JobId aliases DocumentId, so its constructor also mints ambiently.
    ("JobId::new(", "mints an ambient job ULID"),
    // Document's convenience constructor internally mints both ID and time.
    ("Document::new(", "mints ambient document system fields"),
];

#[test]
fn mutation_committer_source_tree_has_no_ambient_time_or_id_mints() {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .expect("Cargo should expose the nimbus-engine manifest directory at runtime");
    let mut rust_files = Vec::new();
    for relative_dir in ["src/engine/mutations", "src/engine/execution_units"] {
        collect_rust_files(&manifest_dir.join(relative_dir), &mut rust_files);
    }

    let mut violations = Vec::new();
    for path in rust_files {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for (pattern, rationale) in BANNED_AMBIENT_MINTS {
            if source.contains(pattern) {
                let display_path = path.strip_prefix(&manifest_dir).unwrap_or(&path);
                violations.push(format!(
                    "{} contains `{pattern}` ({rationale})",
                    display_path.display()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "committer source must use injected time and id seams:\n{}",
        violations.join("\n")
    );
}

fn collect_rust_files(directory: &Path, rust_files: &mut Vec<PathBuf>) {
    let mut entries = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap_or_else(|error| panic!("failed to enumerate {}: {error}", directory.display()));
    entries.sort_by_key(std::fs::DirEntry::path);

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("failed to inspect {}: {error}", path.display()));
        if file_type.is_dir() {
            collect_rust_files(&path, rust_files);
        } else if file_type.is_file() && path.extension().is_some_and(|extension| extension == "rs")
        {
            rust_files.push(path);
        }
    }
}
