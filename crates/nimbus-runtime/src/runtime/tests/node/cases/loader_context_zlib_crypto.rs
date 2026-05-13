const NODE22_LOADER_CONTEXT_ZLIB_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-zlib-const.js"),
    shared_official_batch_case!("test/parallel/test-zlib-convenience-methods.js"),
    shared_official_batch_case!("test/parallel/test-zlib-create-raw.js"),
    shared_official_batch_case!("test/parallel/test-zlib-deflate-constructors.js"),
    shared_official_batch_case!("test/parallel/test-zlib-deflate-raw-inherits.js"),
    shared_official_batch_case!("test/parallel/test-zlib-empty-buffer.js"),
    shared_official_batch_case!("test/parallel/test-zlib-from-string.js"),
    shared_official_batch_case!("test/parallel/test-zlib-invalid-input.js"),
    shared_official_batch_case!("test/parallel/test-zlib-no-stream.js"),
    shared_official_batch_case!("test/parallel/test-zlib-not-string-or-buffer.js"),
    shared_official_batch_case!("test/parallel/test-zlib-object-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-zero-byte.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-after-error.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-after-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-in-ondata.js"),
    shared_official_batch_case!("test/parallel/test-zlib-destroy-pipe.js"),
    shared_official_batch_case!("test/parallel/test-zlib-destroy.js"),
    shared_official_batch_case!("test/parallel/test-zlib-failed-init.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-flush.js",
        COMMON_PERSON_JPG_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-flags.js"),
    shared_official_batch_case!("test/parallel/test-zlib-reset-before-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-close.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary-fail.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-concatenated-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-from-gzip-with-trailing-garbage.js"),
    shared_official_batch_case!("test/parallel/test-zlib-premature-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-truncated.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unzip-one-byte-chunks.js"),
    shared_official_batch_case!("test/parallel/test-zlib-zero-windowBits.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-zlib-brotli-16GB.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-zlib-brotli-16GB.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-zlib-brotli-16GB.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-zlib-brotli-kmaxlength-rangeerror.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-flush.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-from-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-brotli-from-string.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-crc32.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain-longblock.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-write-sync-interleaved.js"),
    shared_official_batch_case!("test/parallel/test-zlib-invalid-arg-value-brotli-compress.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-invalid-input-memory.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-maxOutputLength.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-params.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-random-byte-pipes.js"),
    shared_official_batch_case!("test/parallel/test-zlib-kmaxlength-rangeerror.js"),
    shared_official_batch_case!("test/parallel/test-zlib-sync-no-event.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unused-weak.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-flush.js"),
];

const NODE22_LOADER_CONTEXT_ZLIB_STREAM_LIFECYCLE_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-zlib-close-after-error.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-after-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-close-in-ondata.js"),
    shared_official_batch_case!("test/parallel/test-zlib-destroy-pipe.js"),
    shared_official_batch_case!("test/parallel/test-zlib-destroy.js"),
    shared_official_batch_case!("test/parallel/test-zlib-failed-init.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-flush.js",
        COMMON_PERSON_JPG_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-flags.js"),
    shared_official_batch_case!("test/parallel/test-zlib-reset-before-write.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-close.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary-fail.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-concatenated-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-from-gzip-with-trailing-garbage.js"),
    shared_official_batch_case!("test/parallel/test-zlib-premature-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-truncated.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unzip-one-byte-chunks.js"),
    shared_official_batch_case!("test/parallel/test-zlib-zero-windowBits.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-zlib-brotli-16GB.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-zlib-brotli-16GB.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-zlib-brotli-16GB.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-flush.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-from-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-brotli-from-string.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-crc32.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain-longblock.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-write-sync-interleaved.js"),
    shared_official_batch_case!("test/parallel/test-zlib-invalid-arg-value-brotli-compress.js"),
    shared_official_batch_case!("test/parallel/test-zlib-maxOutputLength.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-params.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-random-byte-pipes.js"),
    shared_official_batch_case!("test/parallel/test-zlib-sync-no-event.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unused-weak.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-flush.js"),
];

const NODE22_LOADER_CONTEXT_ZLIB_DECOMPRESSION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-zlib-dictionary.js"),
    shared_official_batch_case!("test/parallel/test-zlib-dictionary-fail.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-from-concatenated-gzip.js",
        COMMON_ZLIB_GZIP_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-from-gzip-with-trailing-garbage.js"),
    shared_official_batch_case!("test/parallel/test-zlib-premature-end.js"),
    shared_official_batch_case!("test/parallel/test-zlib-truncated.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unzip-one-byte-chunks.js"),
    shared_official_batch_case!("test/parallel/test-zlib-zero-windowBits.js"),
];

const NODE22_LOADER_CONTEXT_ZLIB_BROTLI_AND_CONTROL_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-zlib-brotli-16GB.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-zlib-brotli-16GB.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-zlib-brotli-16GB.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-zlib-brotli-kmaxlength-rangeerror.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-flush.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli-from-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-brotli-from-string.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-brotli.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-crc32.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-drain-longblock.js"),
    shared_official_batch_case!("test/parallel/test-zlib-flush-write-sync-interleaved.js"),
    shared_official_batch_case!("test/parallel/test-zlib-invalid-arg-value-brotli-compress.js"),
    shared_official_batch_case!("test/parallel/test-zlib-maxOutputLength.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-zlib-params.js",
        COMMON_ZLIB_BROTLI_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-zlib-random-byte-pipes.js"),
    shared_official_batch_case!("test/parallel/test-zlib-kmaxlength-rangeerror.js"),
    shared_official_batch_case!("test/parallel/test-zlib-sync-no-event.js"),
    shared_official_batch_case!("test/parallel/test-zlib-unused-weak.js"),
    shared_official_batch_case!("test/parallel/test-zlib-write-after-flush.js"),
];

const NODE22_LOADER_CONTEXT_CRYPTO_HASH_RANDOM_FOUNDATION_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case!("test/parallel/test-crypto-hash-stream-pipe.js"),
    shared_official_batch_case!("test/parallel/test-crypto-from-binary.js"),
    shared_official_batch_case!("test/parallel/test-crypto-secret-keygen.js"),
    shared_official_batch_case!("test/parallel/test-crypto-encoding-validation-error.js"),
    shared_official_batch_case!("test/parallel/test-crypto-hmac.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-hash.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-getcipherinfo.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-oneshot-hash.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-random.js"),
    shared_official_batch_case!("test/parallel/test-crypto-randomfillsync-regression.js"),
    shared_official_batch_case!("test/parallel/test-crypto-randomuuid.js"),
    shared_official_batch_case!("test/parallel/test-crypto-update-encoding.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-authenticated-stream.js",
        COMMON_CRYPTO_AUTHENTICATED_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-aes-wrap.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-cipheriv-decipheriv.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-padding-aes256.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-gcm-explicit-short-tag.js"),
    shared_official_batch_case!("test/parallel/test-crypto-gcm-implicit-short-tag.js"),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-classes.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-lazy-transform-writable.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-stream.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-hkdf.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-pbkdf2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
];

const NODE22_LOADER_CONTEXT_CRYPTO_KDF_AND_STREAM_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-classes.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-lazy-transform-writable.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-stream.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-hkdf.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-pbkdf2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-scrypt.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-scrypt.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-scrypt.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-errors.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-leak.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-generate-keys.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-group-setters.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2-views.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-odd-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-shared.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-ecdh-convert-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-curves.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh-stateless.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-dh-stateless.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-dh-stateless.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE22_LOADER_CONTEXT_CRYPTO_CIPHER_AND_PADDING_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-cipheriv-decipheriv.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-padding-aes256.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case!("test/parallel/test-crypto-gcm-explicit-short-tag.js"),
    shared_official_batch_case!("test/parallel/test-crypto-gcm-implicit-short-tag.js"),
];

const NODE22_LOADER_CONTEXT_CRYPTO_DH_AND_ECDH_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-errors.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-leak.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-generate-keys.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-group-setters.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2-views.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-odd-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-shared.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-ecdh-convert-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-curves.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh-stateless.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-dh-stateless.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-dh-stateless.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE20_LOADER_CONTEXT_CRYPTO_DH_AND_ECDH_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-errors.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-leak.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-generate-keys.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-group-setters.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2-views.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-modp2.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-odd-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-padding.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-shared.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-ecdh-convert-key.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-curves.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh-stateless.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-dh-stateless.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-dh-stateless.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE22_LOADER_CONTEXT_CRYPTO_DH_SAFE_PRIME_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-constructor.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-dh.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-crypto-dh.js"),
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE22_LOADER_CONTEXT_CRYPTO_DH_CURVES_AND_STATELESS_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-curves.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh-stateless.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-dh-stateless.js"),
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-dh-stateless.js"),
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

const NODE20_LOADER_CONTEXT_CRYPTO_DH_SAFE_PRIME_BATCH: &[NodeCompatBatchEntry] =
    &[shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh-constructor.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    )];

const NODE24_LOADER_CONTEXT_CRYPTO_DH_STATELESS_SUPPORTED_WATCHPOINT_BATCH: &[NodeCompatBatchEntry] =
    &[NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-dh-stateless.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some("node24/test/parallel/test-crypto-dh-stateless.js"),
        shared_extra_files: COMMON_CRYPTO_HASH_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    }];

const NODE20_LOADER_CONTEXT_CRYPTO_DH_SUPPORTED_WATCHPOINT_BATCH: &[NodeCompatBatchEntry] =
    &[shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-dh.js",
        COMMON_CRYPTO_HASH_EXTRA_FILES
    )];

const LOADER_CONTEXT_CRYPTO_AUTHENTICATED_AND_AES_WRAP_BATCH: &[NodeCompatBatchEntry] = &[
    shared_official_batch_case_with_extra!(
        "test/parallel/test-crypto-authenticated-stream.js",
        COMMON_CRYPTO_AUTHENTICATED_EXTRA_FILES
    ),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-authenticated.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some("node22/test/parallel/test-crypto-authenticated.js"),
        node24_fixture_source_path: Some("node24/test/parallel/test-crypto-authenticated.js"),
        shared_extra_files: COMMON_CRYPTO_AUTHENTICATED_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-crypto-aes-wrap.js"),
    shared_official_batch_case!("test/parallel/test-crypto-des3-wrap.js"),
];

const NODE20_LOADER_CONTEXT_CRYPTO_AUTHENTICATED_SUPPORTED_WATCHPOINT_BATCH: &[NodeCompatBatchEntry] =
    &[NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-authenticated.js",
        node20_fixture_source_path: Some("node20/test/parallel/test-crypto-authenticated.js"),
        node22_fixture_source_path: None,
        node24_fixture_source_path: None,
        shared_extra_files: COMMON_CRYPTO_AUTHENTICATED_EXTRA_FILES,
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    }];

const LOADER_CONTEXT_CRYPTO_XOF_EXTENSION_BATCH: &[NodeCompatBatchEntry] = &[
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-default-shake-lengths.js",
        node20_fixture_source_path: Some(
            "node20/test/parallel/test-crypto-default-shake-lengths.js",
        ),
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some(
            "node24/test/parallel/test-crypto-default-shake-lengths.js",
        ),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-default-shake-lengths-oneshot.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some(
            "node24/test/parallel/test-crypto-default-shake-lengths-oneshot.js",
        ),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
    shared_official_batch_case!("test/parallel/test-worker-type-check.js"),
    shared_official_batch_case!("test/parallel/test-worker-message-port.js"),
    NodeCompatBatchEntry {
        test_relative_path: "test/parallel/test-crypto-oneshot-hash-xof.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: None,
        node24_fixture_source_path: Some("node24/test/parallel/test-crypto-oneshot-hash-xof.js"),
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    },
];

