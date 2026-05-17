use super::*;

use crate::protocol::VersionUpgradeAction;

pub(crate) async fn version_info(State(state): State<Arc<AppState>>) -> Json<VersionInfoResponse> {
    let snapshot = state.version_check.snapshot().await;
    let available = snapshot
        .latest
        .as_ref()
        .map(|latest| latest > &snapshot.current)
        .unwrap_or(false);
    Json(VersionInfoResponse {
        current: snapshot.current.to_string(),
        latest: snapshot.latest.as_ref().map(ToString::to_string),
        available,
        url: snapshot.url,
        published_at: snapshot.published_at,
        host: snapshot.host,
        check_status: snapshot.check_status,
        upgrade: VersionUpgradeAction {
            method: snapshot.upgrade.method.as_str(),
            command: snapshot.upgrade.command,
            needs_sudo: snapshot.upgrade.needs_sudo,
            interactive: snapshot.upgrade.interactive,
            fallback_url: snapshot.upgrade.fallback_url,
        },
    })
}
