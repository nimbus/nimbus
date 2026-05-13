use super::support::*;
use super::*;

#[tokio::test]
async fn application_node22_reads_local_files_hides_non_allowlisted_env_and_denies_escape_writes() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (tempdir, bundle_path) = write_app_style_bundle(
        r#"
	import { readFile, stat, writeFile } from "node:fs/promises";

	globalThis.__nimbusInvoke = async function () {
	  const config = await readFile("./config.txt", "utf8");
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
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
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
    assert_eq!(result["nodeEnv"], serde_json::json!(null));
    let write_denied = result["writeDenied"]
        .as_str()
        .expect("write denial should be a string");
    assert!(
        write_denied.contains("runtime write capability denied")
            || write_denied.contains("Requires write access"),
        "unexpected write denial: {write_denied}"
    );
    let metadata_denied = result["metadataDenied"]
        .as_str()
        .expect("metadata denial should be a string");
    assert!(
        metadata_denied.contains("runtime read capability denied")
            || metadata_denied.contains("Requires read access"),
        "unexpected metadata denial: {metadata_denied}"
    );
}

#[tokio::test]
async fn application_node22_allows_tls_reject_unauthorized_env_lookup() {
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
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
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
        Arc::new(RuntimePolicy::new(RuntimeLimits::tooling_node22())),
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
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22())),
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

    let mut limits = RuntimeLimits::application_node22();
    limits.grants.worker.clear();
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
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
        Arc::new(RuntimePolicy::new(RuntimeLimits::tooling_node22())),
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
