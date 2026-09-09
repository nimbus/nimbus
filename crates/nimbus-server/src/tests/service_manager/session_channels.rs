use super::*;
use nimbus_services::{SessionChannelAttachment, SessionChannelFrame, SessionChannelSource};
use tokio::sync::mpsc;

/// A source that answers each input line on stdout, `fail` on stderr, and
/// ends the channel with exit code 0 on `exit`.
struct EchoSessionChannelSource;

impl SessionChannelSource for EchoSessionChannelSource {
    fn attach(
        &self,
        _target: &nimbus_services::SessionTargetSnapshot,
        channel: &str,
    ) -> Result<SessionChannelAttachment, nimbus_core::Error> {
        let channel = channel.to_owned();
        let (frames, frames_rx) = mpsc::channel(16);
        let (input, mut input_rx) = mpsc::channel::<String>(16);
        tokio::spawn(async move {
            while let Some(line) = input_rx.recv().await {
                let line = line.trim_end().to_owned();
                let frame = match line.as_str() {
                    "exit" => SessionChannelFrame::Exit { code: 0 },
                    "fail" => SessionChannelFrame::Stderr(format!("{channel}: fail\n")),
                    _ => SessionChannelFrame::Stdout(format!("{line}\n")),
                };
                let done = matches!(frame, SessionChannelFrame::Exit { .. });
                if frames.send(frame).await.is_err() || done {
                    break;
                }
            }
        });
        Ok(SessionChannelAttachment {
            frames: frames_rx,
            input,
        })
    }
}

/// Reads NDJSON frames from the stream response and forwards each one.
async fn relay_frames(mut response: reqwest::Response, frames: mpsc::UnboundedSender<Value>) {
    let mut buffer = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        buffer.extend_from_slice(&chunk);
        while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=newline).collect();
            let text = String::from_utf8(line).expect("frame should be utf-8");
            let frame: Value = serde_json::from_str(text.trim()).expect("frame should parse");
            if frames.send(frame).is_err() {
                return;
            }
        }
    }
}

#[tokio::test]
async fn session_channel_stream_relays_frames_in_order_and_takes_input() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend::default());
    let manager = Arc::new(
        Arc::try_unwrap(service_manager(backend.clone()))
            .unwrap_or_else(|_| panic!("manager should be unshared"))
            .with_session_channel_source(Arc::new(EchoSessionChannelSource)),
    );
    let server = ServerFixture::start(
        managed_router_config(engine.clone(), manager, backend.clone())
            .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let create = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/sandboxes"))
        .bearer_auth("tenant-a-sandbox")
        .json(&sandbox_create_body("tenanta", "shell"))
        .send()
        .await
        .expect("sandbox create should send");
    assert_eq!(create.status(), StatusCode::CREATED);
    let sandbox_id = create.json::<Value>().await.expect("create should parse")["metadata"]["id"]
        .as_str()
        .expect("sandbox id should be a string")
        .to_owned();

    let open = server
        .client()
        .post(server.http_url("/api/sessions"))
        .bearer_auth("tenant-a-sandbox-session")
        .json(&json!({
            "tenantId": "tenanta",
            "target": { "sandbox": { "id": sandbox_id } },
            "channels": ["stdio"],
            "requestedTtlMs": 60000,
        }))
        .send()
        .await
        .expect("session open should send");
    assert_eq!(open.status(), StatusCode::CREATED);
    let session_id = open.json::<Value>().await.expect("open should parse")["metadata"]["id"]
        .as_str()
        .expect("session id should be a string")
        .to_owned();

    // A channel the session was not opened with is refused before any
    // attachment happens.
    let wrong_channel = server
        .client()
        .get(server.http_url(&format!(
            "/api/sessions/{session_id}/channels/files/stream?tenantId=tenanta"
        )))
        .bearer_auth("tenant-a-sandbox-session")
        .send()
        .await
        .expect("wrong-channel stream should send");
    assert_eq!(wrong_channel.status(), StatusCode::BAD_REQUEST);

    // Input before an attachment has nowhere to go.
    let early_input = server
        .client()
        .post(server.http_url(&format!(
            "/api/sessions/{session_id}/channels/stdio/input?tenantId=tenanta"
        )))
        .bearer_auth("tenant-a-sandbox-session")
        .json(&json!({ "data": "echo early\n" }))
        .send()
        .await
        .expect("early input should send");
    assert_eq!(early_input.status(), StatusCode::CONFLICT);

    let stream = server
        .client()
        .get(server.http_url(&format!(
            "/api/sessions/{session_id}/channels/stdio/stream?tenantId=tenanta"
        )))
        .bearer_auth("tenant-a-sandbox-session")
        .send()
        .await
        .expect("stream should send");
    assert_eq!(stream.status(), StatusCode::OK);
    assert_eq!(
        stream
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/x-ndjson")
    );
    let (frames, mut received) = mpsc::unbounded_channel();
    let relay = tokio::spawn(relay_frames(stream, frames));

    let opened = tokio::time::timeout(std::time::Duration::from_secs(5), received.recv())
        .await
        .expect("opened frame should arrive in time")
        .expect("stream should yield an opened frame");
    assert_eq!(
        opened,
        json!({ "kind": "opened", "channel": "stdio", "targetGeneration": 1 })
    );

    for data in ["echo hi\n", "fail\n", "exit\n"] {
        let input = server
            .client()
            .post(server.http_url(&format!(
                "/api/sessions/{session_id}/channels/stdio/input?tenantId=tenanta"
            )))
            .bearer_auth("tenant-a-sandbox-session")
            .json(&json!({ "data": data }))
            .send()
            .await
            .expect("input should send");
        assert_eq!(input.status(), StatusCode::ACCEPTED, "input {data:?}");
    }

    let mut rest = Vec::new();
    while let Ok(Some(frame)) =
        tokio::time::timeout(std::time::Duration::from_secs(5), received.recv()).await
    {
        rest.push(frame);
    }
    relay.await.expect("relay task should finish");
    assert_eq!(
        rest,
        vec![
            json!({ "kind": "stdout", "data": "echo hi\n" }),
            json!({ "kind": "stderr", "data": "stdio: fail\n" }),
            json!({ "kind": "exit", "code": 0 }),
            json!({ "kind": "closed", "reason": "source_closed" }),
        ]
    );

    // The attachment is gone with the stream: input is refused again and the
    // session itself is still open.
    let late_input = server
        .client()
        .post(server.http_url(&format!(
            "/api/sessions/{session_id}/channels/stdio/input?tenantId=tenanta"
        )))
        .bearer_auth("tenant-a-sandbox-session")
        .json(&json!({ "data": "echo late\n" }))
        .send()
        .await
        .expect("late input should send");
    assert_eq!(late_input.status(), StatusCode::CONFLICT);
    let session = server
        .client()
        .get(server.http_url(&format!("/api/sessions/{session_id}?tenantId=tenanta")))
        .bearer_auth("tenant-a-sandbox-session")
        .send()
        .await
        .expect("session get should send")
        .json::<Value>()
        .await
        .expect("session should parse");
    assert_eq!(session["status"]["lifecycleState"], json!("open"));

    // Tenant B cannot reach tenant A's channel.
    let cross_tenant = server
        .client()
        .get(server.http_url(&format!(
            "/api/sessions/{session_id}/channels/stdio/stream?tenantId=tenantb"
        )))
        .bearer_auth("tenant-b-sandbox")
        .send()
        .await
        .expect("cross-tenant stream should send");
    assert!(
        matches!(
            cross_tenant.status(),
            StatusCode::FORBIDDEN | StatusCode::NOT_FOUND | StatusCode::UNAUTHORIZED
        ),
        "cross-tenant stream answered {}",
        cross_tenant.status()
    );
}

#[tokio::test]
async fn session_channel_stream_closes_when_the_backend_cannot_stream() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend::default());
    let manager = service_manager(backend.clone());
    let server = ServerFixture::start(
        managed_router_config(engine.clone(), manager, backend.clone())
            .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
            .without_deploy_admin_token()
            .build(),
    )
    .await;
    let create = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/sandboxes"))
        .bearer_auth("tenant-a-sandbox")
        .json(&sandbox_create_body("tenanta", "shell"))
        .send()
        .await
        .expect("sandbox create should send");
    let sandbox_id = create.json::<Value>().await.expect("create should parse")["metadata"]["id"]
        .as_str()
        .expect("sandbox id should be a string")
        .to_owned();
    let open = server
        .client()
        .post(server.http_url("/api/sessions"))
        .bearer_auth("tenant-a-sandbox-session")
        .json(&json!({
            "tenantId": "tenanta",
            "target": { "sandbox": { "id": sandbox_id } },
            "channels": ["stdio"],
        }))
        .send()
        .await
        .expect("session open should send");
    let session_id = open.json::<Value>().await.expect("open should parse")["metadata"]["id"]
        .as_str()
        .expect("session id should be a string")
        .to_owned();

    let stream = server
        .client()
        .get(server.http_url(&format!(
            "/api/sessions/{session_id}/channels/stdio/stream?tenantId=tenanta"
        )))
        .bearer_auth("tenant-a-sandbox-session")
        .send()
        .await
        .expect("stream should send");
    assert_eq!(stream.status(), StatusCode::OK);
    let body = stream.text().await.expect("stream body should read");
    let frames: Vec<Value> = body
        .lines()
        .map(|line| serde_json::from_str(line).expect("frame should parse"))
        .collect();
    assert_eq!(
        frames,
        vec![
            json!({ "kind": "opened", "channel": "stdio", "targetGeneration": 1 }),
            json!({
                "kind": "closed",
                "reason": "backend `krun` does not stream channel `stdio` bytes yet",
            }),
        ]
    );
}
