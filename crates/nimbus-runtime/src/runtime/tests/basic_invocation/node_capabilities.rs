use super::support::*;
use super::*;

#[tokio::test]
async fn application_node22_reads_local_files_hides_non_allowlisted_env_and_denies_escape_writes() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
	import { mkdirSync } from "node:fs";
	import { readFile, stat, writeFile } from "node:fs/promises";

	globalThis.__nimbusInvoke = async function () {
	  const config = await readFile("./config.txt", "utf8");
	  mkdirSync("./sync-created", { recursive: true });
	  await writeFile("./sync-created/file.txt", "sync-data");
	  const syncRoundTrip = await readFile("./sync-created/file.txt", "utf8");
	  const nodeEnv = process.env.NODE_ENV ?? null;
	  let writeDenied = null;
	  let metadataDenied = null;
	  try {
	    await writeFile("../escape.txt", "should-fail");
	  } catch (error) {
	    writeDenied = error?.message ?? String(error);
	  }
	  try {
	    await stat("/");
	  } catch (error) {
	    metadataDenied = error?.message ?? String(error);
	  }
	  return {
	    cwd: process.cwd(),
	    config,
	    syncRoundTrip,
	    nodeEnv,
	    writeDenied,
	    metadataDenied,
	  };
	};

export {};
"#,
    );
    std::fs::write(
        bundle_path
            .parent()
            .expect("bundle parent should resolve")
            .join("config.txt"),
        "hello from bundle",
    )
    .expect("config should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    let expected_cwd = tempdir
        .path()
        .join("app/.nimbus/convex")
        .canonicalize()
        .expect("expected cwd should canonicalize");
    assert_eq!(
        result["cwd"],
        serde_json::json!(expected_cwd.display().to_string())
    );
    assert_eq!(result["config"], serde_json::json!("hello from bundle"));
    assert_eq!(result["syncRoundTrip"], serde_json::json!("sync-data"));
    assert_eq!(result["nodeEnv"], serde_json::json!(null));
    let write_denied = result["writeDenied"]
        .as_str()
        .expect("write denial should be a string");
    // node:fs surfaces a sandbox write denial as the Node-correct `EACCES`
    // errno (deno_node maps Deno's `NotCapable` permission error to EACCES so
    // fs consumers see libuv-style codes). Older Deno-native phrasing is kept
    // as an accepted alternative in case a denial is reported by the Nimbus
    // capability layer instead.
    assert!(
        write_denied.contains("EACCES")
            || write_denied.contains("runtime write capability denied")
            || write_denied.contains("Requires write access"),
        "unexpected write denial: {write_denied}"
    );
    // The hard security property: the escape write must never materialize a
    // file outside the write root, regardless of how the denial is phrased.
    assert!(
        !tempdir.path().join("app/.nimbus/escape.txt").exists(),
        "escape write must not create a file outside the write root"
    );
    let metadata_denied = result["metadataDenied"]
        .as_str()
        .expect("metadata denial should be a string");
    assert!(
        metadata_denied.contains("EACCES")
            || metadata_denied.contains("runtime read capability denied")
            || metadata_denied.contains("Requires read access"),
        "unexpected metadata denial: {metadata_denied}"
    );
}

#[tokio::test]
async fn application_node22_startup_snapshot_refreshes_policy_cwd_for_relative_fs_writes() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (first_tempdir, first_bundle_path) = write_app_style_bundle(
        r#"
globalThis.__nimbusInvoke = function () {
  return { cwd: process.cwd() };
};

export {};
"#,
    );
    let (second_tempdir, second_bundle_path) = write_app_style_bundle(
        r#"
import { existsSync, mkdirSync, writeFileSync } from "node:fs";

globalThis.__nimbusInvoke = function () {
  mkdirSync("./node_modules/.prisma/client", { recursive: true });
  writeFileSync("./node_modules/.prisma/client/query_engine.node", "not a prisma engine");
  return {
    cwd: process.cwd(),
    wrote: existsSync("./node_modules/.prisma/client/query_engine.node"),
  };
};

export {};
"#,
    );

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
    );
    let request = |function_name: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&first_bundle_path),
            &request("snapshot:first"),
        )
        .await
        .expect("first bundle should execute");
    let expected_first_cwd = first_tempdir
        .path()
        .join("app/.nimbus/convex")
        .canonicalize()
        .expect("first cwd should canonicalize");
    assert_eq!(
        first["cwd"],
        serde_json::json!(expected_first_cwd.display().to_string())
    );

    let second = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&second_bundle_path),
            &request("snapshot:second"),
        )
        .await
        .expect("second bundle should execute");
    let expected_second_cwd = second_tempdir
        .path()
        .join("app/.nimbus/convex")
        .canonicalize()
        .expect("second cwd should canonicalize");
    assert_eq!(
        second["cwd"],
        serde_json::json!(expected_second_cwd.display().to_string())
    );
    assert_eq!(second["wrote"], serde_json::json!(true));
    assert!(
        second_bundle_path
            .parent()
            .expect("bundle parent should resolve")
            .join("node_modules/.prisma/client/query_engine.node")
            .exists(),
        "relative write should stay inside the bundle-generated root"
    );
}

#[tokio::test]
async fn application_node22_production_hides_tls_reject_unauthorized_env_lookup() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let _tls_env = ScopedProcessEnvVar::set("NODE_TLS_REJECT_UNAUTHORIZED", "0");
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
globalThis.__nimbusInvoke = async function () {
  return {
    tlsRejectUnauthorized: process.env.NODE_TLS_REJECT_UNAUTHORIZED ?? null,
  };
};

export {};
"#,
    );

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    assert_eq!(result["tlsRejectUnauthorized"], serde_json::json!(null));
}

#[tokio::test]
async fn application_node22_local_development_allows_tls_reject_unauthorized_env_lookup() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let _tls_env = ScopedProcessEnvVar::set("NODE_TLS_REJECT_UNAUTHORIZED", "0");
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
globalThis.__nimbusInvoke = async function () {
  return {
    tlsRejectUnauthorized: process.env.NODE_TLS_REJECT_UNAUTHORIZED ?? null,
  };
};

export {};
"#,
    );

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22_local_development()),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    assert_eq!(result["tlsRejectUnauthorized"], serde_json::json!("0"));
}

#[tokio::test]
async fn application_node22_shared_worker_env_is_runtime_scoped_and_grant_gated() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { Worker, SHARE_ENV } from "node:worker_threads";

const NAME = "NIMBUS_C1_3_SHARED_ENV";

function sharedEnvOps() {
  return globalThis.__nimbusHiddenDenoGlobals.core.ops;
}

function capture(action) {
  try {
    return { ok: true, value: action() ?? null };
  } catch (error) {
    return {
      ok: false,
      code: error?.code ?? null,
      message: error?.message ?? String(error),
    };
  }
}

function runSharedWorker(value) {
  const sharedEnv = globalThis.__nimbusInstallSharedWorkerEnvProxy();
  sharedEnv[NAME] = value;
  const workerSource = `
const { parentPort } = require("node:worker_threads");
const NAME = ${JSON.stringify(NAME)};
parentPort.postMessage({ before: process.env[NAME] ?? null });
process.env[NAME] = "worker-mutated-value";
parentPort.postMessage({ after: process.env[NAME] ?? null });
`;
  const worker = new Worker(workerSource, { eval: true, env: SHARE_ENV });
  const messages = [];
  return new Promise((resolve, reject) => {
    worker.once("error", reject);
    worker.on("message", (message) => {
      messages.push(message);
      if (messages.length === 2) {
        worker.terminate();
        resolve({
          messages,
          finalValue: sharedEnv[NAME] ?? null,
        });
      }
    });
  });
}

globalThis.__nimbusInvoke = async function (request) {
  const ops = sharedEnvOps();
  const mode = request.args?.mode;
  if (mode === "write") {
    ops.op_nimbus_runtime_shared_env_seed({});
    ops.op_nimbus_runtime_shared_env_set(NAME, request.args.value);
    return {
      value: ops.op_nimbus_runtime_shared_env_get(NAME) ?? null,
      snapshotValue: ops.op_nimbus_runtime_shared_env_snapshot()[NAME] ?? null,
    };
  }
  if (mode === "read") {
    return {
      value: ops.op_nimbus_runtime_shared_env_get(NAME) ?? null,
      snapshotValue: ops.op_nimbus_runtime_shared_env_snapshot()[NAME] ?? null,
    };
  }
  if (mode === "deniedWrite") {
    return capture(() => {
      ops.op_nimbus_runtime_shared_env_set(NAME, "denied");
      return ops.op_nimbus_runtime_shared_env_get(NAME);
    });
  }
  if (mode === "workerShare") {
    return await runSharedWorker(request.args.value);
  }
  return { error: `unexpected mode ${mode}` };
};

export {};
"#,
    );

    let mut read_write_limits = RuntimeLimits::application_node22();
    read_write_limits
        .grants
        .env_read
        .push("NIMBUS_C1_3_SHARED_ENV".to_string());
    read_write_limits
        .grants
        .env_write
        .push("NIMBUS_C1_3_SHARED_ENV".to_string());

    let writer_runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(read_write_limits.clone()),
    );
    let written = writer_runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: serde_json::json!({
                    "mode": "write",
                    "value": "writer-runtime-value",
                }),
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("writer runtime should execute");

    assert_eq!(
        written,
        serde_json::json!({
            "value": "writer-runtime-value",
            "snapshotValue": "writer-runtime-value",
        })
    );

    let mut worker_limits = RuntimeLimits::application_node22_local_development();
    worker_limits
        .grants
        .env_read
        .push("NIMBUS_C1_3_SHARED_ENV".to_string());
    worker_limits
        .grants
        .env_write
        .push("NIMBUS_C1_3_SHARED_ENV".to_string());
    let worker_runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(worker_limits),
    );
    let worker_shared = worker_runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: serde_json::json!({
                    "mode": "workerShare",
                    "value": "parent-worker-value",
                }),
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("worker shared-env runtime should execute");

    assert_eq!(
        worker_shared,
        serde_json::json!({
            "messages": [
                { "before": "parent-worker-value" },
                { "after": "worker-mutated-value" },
            ],
            "finalValue": "worker-mutated-value",
        })
    );

    let reader_runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(read_write_limits),
    );
    let read = reader_runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: serde_json::json!({ "mode": "read" }),
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("reader runtime should execute");

    assert_eq!(
        read,
        serde_json::json!({
            "value": null,
            "snapshotValue": null,
        }),
        "a second runtime must not see the first runtime's shared env"
    );

    let mut read_only_limits = RuntimeLimits::application_node22();
    read_only_limits
        .grants
        .env_read
        .push("NIMBUS_C1_3_SHARED_ENV".to_string());
    let read_only_runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(read_only_limits),
    );
    let denied = read_only_runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: serde_json::json!({ "mode": "deniedWrite" }),
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("read-only runtime should execute");

    assert_eq!(denied["ok"], serde_json::json!(false));
    let message = denied["message"]
        .as_str()
        .expect("env write denial should include a message");
    assert!(
        message.contains("runtime env write capability denied"),
        "unexpected shared env write denial: {message}"
    );
}

#[tokio::test]
async fn tooling_node22_allows_allowlisted_env_and_tmp_writes() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { readFile, writeFile } from "node:fs/promises";

globalThis.__nimbusInvoke = async function () {
  await writeFile(".nimbus/tmp/tooling.txt", "tooling-data");
  const roundTrip = await readFile(".nimbus/tmp/tooling.txt", "utf8");
  return {
    cwd: process.cwd(),
    pathValue: process.env.PATH ?? null,
    roundTrip,
  };
};

export {};
"#,
    );
    std::fs::create_dir_all(tempdir.path().join("app/.nimbus/tmp"))
        .expect("tooling tmp dir should build");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::tooling_node22()),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    let expected_cwd = tempdir
        .path()
        .join("app")
        .canonicalize()
        .expect("expected cwd should canonicalize");
    assert_eq!(
        result["cwd"],
        serde_json::json!(expected_cwd.display().to_string())
    );
    assert_eq!(
        result["pathValue"],
        serde_json::json!(std::env::var("PATH").expect("PATH should be present in tests"))
    );
    assert_eq!(result["roundTrip"], serde_json::json!("tooling-data"));
    assert!(
        tempdir.path().join("app/.nimbus/tmp/tooling.txt").is_file(),
        "tooling write should materialize under the scoped tmp root"
    );
}

#[tokio::test]
async fn application_node22_denies_child_process_spawn_even_for_process_exec_path() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { spawnSync } from "node:child_process";

globalThis.__nimbusInvoke = function () {
  try {
    const child = spawnSync(process.execPath, ["-e", "console.log('child-ok')"], {
      encoding: "utf8",
    });
    return {
      denied: child.error?.message ?? null,
      deniedCode: child.error?.code ?? null,
      status: child.status ?? null,
      signal: child.signal ?? null,
      stdout: child.stdout ?? null,
      stderr: child.stderr ?? null,
      keys: Object.keys(child).sort(),
    };
  } catch (error) {
    return {
      denied: error?.message ?? String(error),
      deniedCode: error?.code ?? null,
      status: null,
      signal: null,
      stdout: null,
      stderr: null,
      keys: [],
    };
  }
};

export {};
"#,
    );

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    let denied = result["denied"].as_str();
    let status_is_denied = result["status"] == serde_json::json!(null);
    let stdout_is_empty = result["stdout"].is_null() || result["stdout"] == serde_json::json!("");
    let stderr_is_empty = result["stderr"].is_null() || result["stderr"] == serde_json::json!("");
    assert!(
        denied.is_some_and(|message| {
            message.contains("runtime run capability denied")
                || message.contains("Requires run access")
        }) || (status_is_denied && stdout_is_empty && stderr_is_empty),
        "unexpected child_process denial payload: {result}"
    );
    assert_eq!(result["status"], serde_json::json!(null));
}

#[tokio::test]
async fn application_node22_worker_threads_require_worker_grant() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { Worker } from "node:worker_threads";

globalThis.__nimbusInvoke = function () {
  try {
    new Worker("require('node:worker_threads').parentPort.postMessage('ok')", {
      eval: true,
    });
    return { denied: null };
  } catch (error) {
    return { denied: error?.message ?? String(error) };
  }
};

export {};
"#,
    );

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute far enough to prove worker denial");

    let denied = result["denied"]
        .as_str()
        .expect("worker creation should be denied by grants");
    assert!(
        denied.contains("runtime worker grant denied for `thread`"),
        "unexpected worker denial: {denied}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn application_node22_confines_symlink_stat_and_readlink_targets() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { stat, readlink } from "node:fs/promises";
import { statSync, readlinkSync } from "node:fs";

async function capture(action) {
  try {
    return { ok: true, value: await action() };
  } catch (error) {
    return {
      ok: false,
      code: error?.code ?? null,
      message: error?.message ?? String(error),
    };
  }
}

function captureSync(action) {
  try {
    return { ok: true, value: action() };
  } catch (error) {
    return {
      ok: false,
      code: error?.code ?? null,
      message: error?.message ?? String(error),
    };
  }
}

globalThis.__nimbusInvoke = async function () {
  return {
    insideStat: await capture(async () => (await stat("./inside-link.txt")).isFile()),
    insideReadlink: await capture(async () => await readlink("./inside-link.txt")),
    escapeStat: await capture(async () => (await stat("./escape-link.txt")).isFile()),
    escapeStatSync: captureSync(() => statSync("./escape-link.txt").isFile()),
    escapeReadlink: await capture(async () => await readlink("./escape-link.txt")),
    escapeReadlinkSync: captureSync(() => readlinkSync("./escape-link.txt")),
  };
};

export {};
"#,
    );
    let bundle_dir = bundle_path
        .parent()
        .expect("bundle parent should resolve")
        .to_path_buf();
    let inside_target = bundle_dir.join("inside-target.txt");
    let outside_target = tempdir.path().join("outside-secret.txt");
    std::fs::write(&inside_target, "inside").expect("inside target should write");
    std::fs::write(&outside_target, "outside").expect("outside target should write");
    std::os::unix::fs::symlink(&inside_target, bundle_dir.join("inside-link.txt"))
        .expect("inside symlink should write");
    std::os::unix::fs::symlink(&outside_target, bundle_dir.join("escape-link.txt"))
        .expect("escape symlink should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::application_node22()),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result["insideStat"]["ok"],
        serde_json::json!(true),
        "inside symlink stat should resolve inside target: {result}"
    );
    assert_eq!(
        result["insideStat"]["value"],
        serde_json::json!(true),
        "inside symlink stat should report a file: {result}"
    );
    assert_eq!(
        result["insideReadlink"]["ok"],
        serde_json::json!(true),
        "inside symlink readlink should resolve inside target: {result}"
    );
    assert_eq!(
        result["insideReadlink"]["value"],
        serde_json::json!(inside_target.display().to_string())
    );

    for key in [
        "escapeStat",
        "escapeStatSync",
        "escapeReadlink",
        "escapeReadlinkSync",
    ] {
        assert_eq!(
            result[key]["ok"],
            serde_json::json!(false),
            "escaping symlink operation {key} should be denied: {result}"
        );
        let message = result[key]["message"]
            .as_str()
            .expect("symlink target denial should include a message");
        assert!(
            message.contains("runtime read capability denied")
                || message.contains("Requires read access"),
            "unexpected {key} denial: {message}"
        );
        assert!(
            message.contains("outside-secret"),
            "denial should identify the escaped target for {key}: {message}"
        );
    }
}

#[tokio::test]
async fn tooling_node22_write_file_requires_preexisting_parent_directory() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
import { writeFile } from "node:fs/promises";

globalThis.__nimbusInvoke = async function () {
  try {
    await writeFile(".nimbus/tmp/missing/tooling.txt", "tooling-data");
    return { ok: true };
  } catch (error) {
    return {
      ok: false,
      code: error?.code ?? null,
      syscall: error?.syscall ?? null,
      message: error?.message ?? String(error),
    };
  }
};

export {};
"#,
    );
    std::fs::create_dir_all(tempdir.path().join("app/.nimbus/tmp"))
        .expect("tooling tmp dir should build");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        runtime_test_policy_with_real_fs(RuntimeLimits::tooling_node22()),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle should execute");

    assert_eq!(
        result["ok"],
        serde_json::json!(false),
        "unexpected missing-parent write result: {result}"
    );
    assert_eq!(
        result["code"],
        serde_json::json!("ENOENT"),
        "unexpected missing-parent write result: {result}"
    );
    assert_eq!(
        result["syscall"],
        serde_json::json!("open"),
        "unexpected missing-parent write result: {result}"
    );
    let message = result["message"]
        .as_str()
        .expect("missing parent write failure should include a message");
    assert!(
        message.contains("no such file or directory"),
        "unexpected write failure: {message}"
    );
    assert!(
        !tempdir
            .path()
            .join("app/.nimbus/tmp/missing/tooling.txt")
            .exists(),
        "writeFile should not materialize missing parent directories"
    );
}
