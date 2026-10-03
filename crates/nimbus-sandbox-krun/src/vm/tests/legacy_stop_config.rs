//! Legacy coarse-stop configuration behavior retained until NNC6.5g.

use super::*;

#[test]
fn configured_stop_signal_prefers_image_metadata_and_falls_back_to_term() {
    assert_eq!(
        configured_stop_signal(
            sample_image_metadata()
                .with_stop_signal("SIGQUIT")
                .stop_signal
                .as_deref()
        ),
        "SIGQUIT"
    );
    assert_eq!(
        configured_stop_signal(
            sample_image_metadata()
                .with_stop_signal("  ")
                .stop_signal
                .as_deref()
        ),
        "TERM"
    );
    assert_eq!(configured_stop_signal(None), "TERM");
}

#[test]
fn configured_stop_timeout_prefers_sandbox_lifecycle_and_falls_back_to_backend_default() {
    let backend_default = KrunSandboxBackendConfig {
        stop_timeout: Duration::from_secs(5),
        ..KrunSandboxBackendConfig::default()
    };
    assert_eq!(
        configured_stop_timeout(
            &sample_spec().with_stop_timeout(Duration::from_secs(30)),
            backend_default.stop_timeout,
        ),
        Duration::from_secs(30)
    );
    assert_eq!(
        configured_stop_timeout(&sample_spec(), backend_default.stop_timeout),
        Duration::from_secs(5)
    );
}
