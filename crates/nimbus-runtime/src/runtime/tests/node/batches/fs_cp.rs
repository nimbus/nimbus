macro_rules! node22_node24_fs_cp_case {
    ($test_relative_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: None,
            node22_fixture_source_path: Some(concat!("node22/", $test_relative_path)),
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: NODE22_FS_CP_EXTRA_FILES,
            node24_extra_files: NODE24_FS_CP_EXTRA_FILES,
        }
    };
}

macro_rules! node24_fs_cp_case {
    ($test_relative_path:literal) => {
        NodeCompatBatchEntry {
            test_relative_path: $test_relative_path,
            node20_fixture_source_path: None,
            node22_fixture_source_path: None,
            node24_fixture_source_path: Some(concat!("node24/", $test_relative_path)),
            shared_extra_files: &[],
            node20_extra_files: &[],
            node22_extra_files: &[],
            node24_extra_files: NODE24_FS_CP_EXTRA_FILES,
        }
    };
}

const NODE22_FS_CP_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "node22/test/common/index.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fs.js",
        fixture_source_path: "node22/test/common/fs.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/README.md",
        fixture_source_path: "node22/test/fixtures/copy/kitchen-sink/README.md",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/index.js",
        fixture_source_path: "node22/test/fixtures/copy/kitchen-sink/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/index.js",
        fixture_source_path: "node22/test/fixtures/copy/kitchen-sink/a/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/b/README2.md",
        fixture_source_path: "node22/test/fixtures/copy/kitchen-sink/a/b/README2.md",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/b/index.js",
        fixture_source_path: "node22/test/fixtures/copy/kitchen-sink/a/b/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/c/README2.md",
        fixture_source_path: "node22/test/fixtures/copy/kitchen-sink/a/c/README2.md",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/c/index.js",
        fixture_source_path: "node22/test/fixtures/copy/kitchen-sink/a/c/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/c/d/README3.md",
        fixture_source_path: "node22/test/fixtures/copy/kitchen-sink/a/c/d/README3.md",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/c/d/index.js",
        fixture_source_path: "node22/test/fixtures/copy/kitchen-sink/a/c/d/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/utf/新建文件夹/index.js",
        fixture_source_path: "node22/test/fixtures/copy/utf/新建文件夹/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/utf/新建文件夹/experimental.json",
        fixture_source_path: "node22/test/fixtures/copy/utf/新建文件夹/experimental.json",
    },
];

const NODE24_FS_CP_EXTRA_FILES: &[NodeCompatExtraFixtureEntry] = &[
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/index.mjs",
        fixture_source_path: "node24/test/common/index.mjs",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/common/fs.js",
        fixture_source_path: "node24/test/common/fs.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/README.md",
        fixture_source_path: "node24/test/fixtures/copy/kitchen-sink/README.md",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/index.js",
        fixture_source_path: "node24/test/fixtures/copy/kitchen-sink/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/index.js",
        fixture_source_path: "node24/test/fixtures/copy/kitchen-sink/a/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/b/README2.md",
        fixture_source_path: "node24/test/fixtures/copy/kitchen-sink/a/b/README2.md",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/b/index.js",
        fixture_source_path: "node24/test/fixtures/copy/kitchen-sink/a/b/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/c/README2.md",
        fixture_source_path: "node24/test/fixtures/copy/kitchen-sink/a/c/README2.md",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/c/index.js",
        fixture_source_path: "node24/test/fixtures/copy/kitchen-sink/a/c/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/c/d/README3.md",
        fixture_source_path: "node24/test/fixtures/copy/kitchen-sink/a/c/d/README3.md",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/kitchen-sink/a/c/d/index.js",
        fixture_source_path: "node24/test/fixtures/copy/kitchen-sink/a/c/d/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/utf/新建文件夹/index.js",
        fixture_source_path: "node24/test/fixtures/copy/utf/新建文件夹/index.js",
    },
    NodeCompatExtraFixtureEntry {
        runtime_path: "test/fixtures/copy/utf/新建文件夹/experimental.json",
        fixture_source_path: "node24/test/fixtures/copy/utf/新建文件夹/experimental.json",
    },
];

const FS_CP_BATCH: &[NodeCompatBatchEntry] = &[
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-async-filter-function.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-copy-non-directory-symlink.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-dereference-force-false-silent-fail.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-dereference-symlink.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-dest-symlink-points-to-src-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-dir-exists-error-on-exist.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-dir-to-file.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-error-on-exist.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-file-to-dir.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-file-to-file.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-file-url.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-filter-child-folder.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-filter-function.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-identical-src-dest.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-invalid-mode-range.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-invalid-options-type.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-nested-files-folders.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-no-errors-force-false.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-no-recursive.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-overwrites-force-true.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-preserve-timestamps-readonly-file.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-preserve-timestamps.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-same-dir-twice.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-skip-validation-when-filtered.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-subdirectory-of-self.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-symlink-dest-points-to-src.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-symlink-over-file.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-symlink-points-to-dest.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-async-with-mode-flags.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-promises-async-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-promises-file-url.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-promises-invalid-mode.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-promises-mode-flags.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-promises-nested-folder-recursive.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-promises-options-validation.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-apply-filter-function.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-async-filter-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-copy-directory-to-file-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-copy-directory-without-recursive-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-copy-file-to-directory-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-copy-file-to-file-path.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-copy-symlink-not-pointing-to-folder.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-copy-symlink-over-file-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-copy-symlinks-to-existing-symlinks.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-copy-to-subdirectory-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-dereference-directory.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-dereference-file.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-dereference-twice.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-dereference.js"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-dest-name-prefix-match.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-dest-parent-name-prefix-match.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-directory-not-exist-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-error-on-exist.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-file-url.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-filename-too-long-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-incompatible-options-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-mode-flags.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-mode-invalid.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-nested-files-folders.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-no-overwrite-force-false.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-options-invalid-type-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-overwrite-force-true.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-parent-symlink-dest-points-to-src-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-preserve-timestamps-readonly.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-preserve-timestamps.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-resolve-relative-symlinks-default.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-resolve-relative-symlinks-false.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-src-dest-identical-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-src-parent-of-dest-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-symlink-dest-points-to-src-error.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-symlink-points-to-dest-error.mjs"),
    node24_fs_cp_case!("test/parallel/test-fs-cp-sync-unicode-dest.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-unicode-folder-names.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-verbatim-symlinks-invalid.mjs"),
    node22_node24_fs_cp_case!("test/parallel/test-fs-cp-sync-verbatim-symlinks-true.mjs"),
];
