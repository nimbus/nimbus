//! CD7(i) — discovery file format is a three-party contract: the CLI
//! (here), the Electron desktop shell (`desktop/src/main/discovery.ts`),
//! and the Playwright fixture
//! (`packages/nimbus-ui/tests/e2e/fixtures/nimbus-server.ts`) all read
//! `server.json`. Any silent shape drift — a renamed field, a casing
//! change, an unexpected `null` — would break the other two consumers
//! without producing a server-side test failure.
//!
//! This integration test builds a `ServerDiscoveryRecord` fixture,
//! pretty-serialises it, byte-compares against a checked-in golden, and
//! confirms the JSON deserialises back into an equal record. Updates to
//! the golden file are intentionally noisy — they must be reviewed.

use nimbus_server::ServerDiscoveryRecord;

const GOLDEN: &str = include_str!("fixtures/server_discovery.golden.json");

fn fixture_record() -> ServerDiscoveryRecord {
    ServerDiscoveryRecord {
        pid: 12345,
        address: "127.0.0.1:8088".to_string(),
        started_at: "2026-05-19T12:34:56Z".to_string(),
        version: "0.1.31".to_string(),
        protocol_versions: vec!["nimbus.v2".to_string()],
    }
}

#[test]
fn server_discovery_record_matches_golden_pretty_json() {
    let record = fixture_record();
    let serialized =
        serde_json::to_string_pretty(&record).expect("ServerDiscoveryRecord should serialize");
    assert_eq!(
        serialized,
        GOLDEN.trim_end_matches('\n'),
        "ServerDiscoveryRecord JSON shape has drifted from the checked-in golden \
         (consumers: desktop/src/main/discovery.ts + Playwright nimbus-server fixture). \
         Update tests/fixtures/server_discovery.golden.json and the other two \
         consumers in lockstep."
    );
}

#[test]
fn server_discovery_record_round_trips_through_golden_bytes() {
    let parsed: ServerDiscoveryRecord = serde_json::from_str(GOLDEN)
        .expect("golden discovery JSON should deserialize into ServerDiscoveryRecord");
    assert_eq!(parsed, fixture_record());
}
