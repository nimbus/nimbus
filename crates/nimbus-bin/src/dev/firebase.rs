use std::io;
use std::path::Path;

use crate::cli_ux;
use crate::provision;

use super::firebase_scan::{self, CoveredSet, FirebaseScan};

/// Scan-gated Firebase wiring: statically scan the app's imports against
/// the drop-in `firebase` package's covered set, then either wire the app
/// (provision the embedded closure, rewire `package.json` to the
/// provisioned `file:` spec, force a Node reinstall) or refuse before any
/// mutation with every blocking finding listed.
pub(super) fn wire_firestore_client_app(app_dir: &Path) -> io::Result<()> {
    let covered = firebase_scan::embedded_covered_set()?;
    let scan = firebase_scan::scan_app(app_dir, covered)?;
    if !scan.passes() {
        for line in firestore_wiring_refusal_lines(&scan, covered) {
            cli_ux::write_stderr_line(&line)?;
        }
        return Err(io::Error::other(
            "refusing to wire this app: the import scan found Firebase usage \
             the drop-in `firebase` package does not cover",
        ));
    }
    let selection = provision::Selection::parse("firebase")
        .expect("firebase must be a known provision selection");
    provision::ensure(app_dir, &selection)?;
    Ok(())
}

pub(super) fn firestore_wiring_refusal_lines(
    scan: &FirebaseScan,
    covered: &CoveredSet,
) -> Vec<String> {
    let mut lines = vec![
        String::new(),
        "This app uses Firebase modules the drop-in `firebase` package does not cover:".to_string(),
        String::new(),
    ];
    for finding in scan.blocking_findings() {
        lines.push(format!("  {}", finding.describe()));
    }
    lines.push(String::new());
    lines.push(format!(
        "Covered imports: {}",
        covered.covered_specifiers().collect::<Vec<_>>().join(", ")
    ));
    lines.push("No files were changed.".to_string());
    lines.push(
        "Compatibility reference: https://nimbusdocs.com/reference/firebase/compatibility/"
            .to_string(),
    );
    lines
}
