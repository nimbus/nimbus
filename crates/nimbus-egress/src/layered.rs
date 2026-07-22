//! Node-global allow-ceiling composition (EE2).
//!
//! `LayeredEgressPolicy` composes an optional node-global policy with the
//! per-sandbox policy as an **allow-ceiling**: a request is allowed only if
//! BOTH allow-lists permit it (intersection). This is a combinator over two
//! compiled policies — deliberately NOT a merge of their `allow` vecs, which
//! would be a union (either list permits — the opposite of a ceiling).
//!
//! The global layer is a pure gate: allow or deny, nothing else. When both
//! layers allow, the returned authorization is the per-sandbox layer's,
//! verbatim — `matched_rule`, `requires_proxy_enforcement`, credential
//! injection, and DLP all come from the sandbox rule alone; the ceiling's
//! metadata is discarded. The global `CompiledEgressPolicy` is tenant-agnostic:
//! it carries no per-tenant secret and the composition performs no per-request
//! tenant lookup — one compiled ceiling is shared by every workload on the
//! node.
//!
//! Pure and in-memory like everything in this crate: composition adds exactly
//! one extra `authorize()` call per request (nanoseconds) and no I/O.
//!
//! A node-wide *blocklist* ("deny host X for everyone") cannot be expressed by
//! an allow-ceiling — intersection can only narrow — and needs a deny-rule
//! type instead; that is explicitly out of scope here (egress-engine plan,
//! Non-goals).

use crate::policy::{CompiledEgressPolicy, EgressAuthorization, EgressRequest};

/// A per-sandbox policy optionally narrowed by a node-global allow-ceiling.
#[derive(Debug, Clone)]
pub struct LayeredEgressPolicy {
    global: Option<CompiledEgressPolicy>,
    sandbox: CompiledEgressPolicy,
}

impl LayeredEgressPolicy {
    /// No ceiling configured: behavior is byte-identical to the sandbox
    /// policy alone.
    pub fn sandbox_only(sandbox: CompiledEgressPolicy) -> Self {
        Self {
            global: None,
            sandbox,
        }
    }

    /// Narrow the sandbox policy under a node-global allow-ceiling.
    ///
    /// Note the default-deny consequence: a configured-but-empty ceiling (a
    /// compiled policy with no rules) allows nothing, so every request is
    /// denied. "No ceiling" is expressed by [`Self::sandbox_only`], not by an
    /// empty policy.
    pub fn with_global_ceiling(
        global: CompiledEgressPolicy,
        sandbox: CompiledEgressPolicy,
    ) -> Self {
        Self {
            global: Some(global),
            sandbox,
        }
    }

    /// True when a global ceiling is configured.
    pub fn has_global_ceiling(&self) -> bool {
        self.global.is_some()
    }

    /// The per-sandbox layer (the sole source of enforcement metadata).
    pub fn sandbox(&self) -> &CompiledEgressPolicy {
        &self.sandbox
    }

    /// Authorize `request`: the ceiling (when configured) must allow it, and
    /// then the per-sandbox policy decides — its authorization is returned
    /// verbatim, so all enforcement metadata comes from the sandbox rule.
    pub fn authorize(&self, request: &EgressRequest) -> EgressAuthorization {
        if let Some(global) = &self.global {
            let ceiling = global.authorize(request);
            if !ceiling.is_allowed() {
                // Pure gate: surface the denial as the ceiling's, with no
                // matched rule and no enforcement metadata (a deny never
                // carries either).
                return EgressAuthorization::deny(format!(
                    "node egress allow-ceiling denied the request: {}",
                    ceiling.reason()
                ));
            }
        }
        self.sandbox.authorize(request)
    }

    /// Authorize an outer HTTPS CONNECT without pretending it is the inner
    /// HTTP request. Both layers enforce authority and SSRF constraints; the
    /// sandbox authorization remains the returned metadata source.
    pub fn authorize_connect(&self, request: &EgressRequest) -> EgressAuthorization {
        if let Some(global) = &self.global {
            let ceiling = global.authorize_connect(request);
            if !ceiling.is_allowed() {
                return EgressAuthorization::deny(format!(
                    "node egress allow-ceiling denied the request: {}",
                    ceiling.reason()
                ));
            }
        }
        self.sandbox.authorize_connect(request)
    }

    pub fn connect_requires_interception(&self, request: &EgressRequest) -> bool {
        self.global
            .as_ref()
            .is_some_and(|global| global.connect_requires_interception(request))
            || self.sandbox.connect_requires_interception(request)
    }

    /// Pre-resolution variant: same composition applied to the hostname-only
    /// decision point (the PEP authorizes before DNS and again on the
    /// resolved IP; the ceiling gates both).
    pub fn authorize_hostname_without_resolved_ip(
        &self,
        request: &EgressRequest,
    ) -> EgressAuthorization {
        if let Some(global) = &self.global {
            let ceiling = global.authorize_hostname_without_resolved_ip(request);
            if !ceiling.is_allowed() {
                return EgressAuthorization::deny(format!(
                    "node egress allow-ceiling denied the request: {}",
                    ceiling.reason()
                ));
            }
        }
        self.sandbox.authorize_hostname_without_resolved_ip(request)
    }

    pub fn authorize_connect_hostname_without_resolved_ip(
        &self,
        request: &EgressRequest,
    ) -> EgressAuthorization {
        if let Some(global) = &self.global {
            let ceiling = global.authorize_connect_hostname_without_resolved_ip(request);
            if !ceiling.is_allowed() {
                return EgressAuthorization::deny(format!(
                    "node egress allow-ceiling denied the request: {}",
                    ceiling.reason()
                ));
            }
        }
        self.sandbox
            .authorize_connect_hostname_without_resolved_ip(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{EgressDlpRule, EgressPolicy, EgressProtocol, EgressRequest, EgressRule};

    fn compiled(rules: impl IntoIterator<Item = EgressRule>) -> CompiledEgressPolicy {
        EgressPolicy::new(rules)
            .compile()
            .expect("test policy should compile")
    }

    fn https_rule(name: &str, host: &str) -> EgressRule {
        EgressRule::new(name, EgressProtocol::Https, host, 443)
    }

    fn request(host: &str) -> EgressRequest {
        EgressRequest::new(EgressProtocol::Https, host, 443)
    }

    #[test]
    fn no_ceiling_is_byte_identical_to_sandbox_policy() {
        let sandbox = compiled([https_rule("api", "api.example.test")]);
        let layered = LayeredEgressPolicy::sandbox_only(sandbox.clone());

        for host in ["api.example.test", "denied.example.test"] {
            let req = request(host);
            assert_eq!(
                layered.authorize(&req),
                sandbox.authorize(&req),
                "sandbox-only layering must not change the authorization for {host}"
            );
            assert_eq!(
                layered.authorize_hostname_without_resolved_ip(&req),
                sandbox.authorize_hostname_without_resolved_ip(&req),
            );
        }
    }

    #[test]
    fn both_allow_returns_sandbox_authorization_verbatim() {
        // The ceiling rule carries DLP (requires_proxy_enforcement = true);
        // the sandbox rule is plain. If ceiling metadata leaked, matched_rule
        // or requires_proxy_enforcement would betray it.
        let global = compiled([https_rule("ceiling-api", "api.example.test")
            .with_dlp_rules([EgressDlpRule::new("no-secret", "secret")])]);
        let sandbox = compiled([https_rule("sandbox-api", "api.example.test")]);
        let layered = LayeredEgressPolicy::with_global_ceiling(global, sandbox.clone());

        let req = request("api.example.test");
        let combined = layered.authorize(&req);
        assert!(combined.is_allowed());
        assert_eq!(
            combined,
            sandbox.authorize(&req),
            "an allowed request must carry the sandbox layer's authorization verbatim"
        );
        assert_eq!(combined.matched_rule(), Some("sandbox-api"));
        assert!(
            !combined.requires_proxy_enforcement(),
            "the ceiling's DLP metadata must be discarded — the sandbox rule is the \
             sole source of enforcement metadata"
        );
    }

    #[test]
    fn ceiling_denies_what_the_sandbox_would_allow_anti_union() {
        // The sandbox allows internal.example.test; the ceiling does not list
        // it. A vec-merge (union) of the two allow-lists WOULD allow it —
        // intersection must deny. This is the load-bearing anti-union test.
        let global = compiled([https_rule("ceiling-api", "api.example.test")]);
        let sandbox = compiled([
            https_rule("sandbox-api", "api.example.test"),
            https_rule("sandbox-internal", "internal.example.test"),
        ]);
        let layered = LayeredEgressPolicy::with_global_ceiling(global, sandbox);

        let denied = layered.authorize(&request("internal.example.test"));
        assert!(
            !denied.is_allowed(),
            "the ceiling must narrow the sandbox allow-list (intersection, not union)"
        );
        assert!(denied.reason().contains("allow-ceiling"));
        assert_eq!(denied.matched_rule(), None);
        assert!(!denied.requires_proxy_enforcement());

        // The intersection host still flows.
        assert!(layered.authorize(&request("api.example.test")).is_allowed());
    }

    #[test]
    fn sandbox_deny_passes_through_verbatim_when_ceiling_allows() {
        let global = compiled([
            https_rule("ceiling-api", "api.example.test"),
            https_rule("ceiling-other", "other.example.test"),
        ]);
        let sandbox = compiled([https_rule("sandbox-api", "api.example.test")]);
        let layered = LayeredEgressPolicy::with_global_ceiling(global, sandbox.clone());

        let req = request("other.example.test");
        let denied = layered.authorize(&req);
        assert!(!denied.is_allowed());
        assert_eq!(
            denied,
            sandbox.authorize(&req),
            "a ceiling-allowed request must surface the sandbox layer's own deny"
        );
    }

    #[test]
    fn configured_but_empty_ceiling_denies_everything_default_deny() {
        let sandbox = compiled([https_rule("sandbox-api", "api.example.test")]);
        let layered = LayeredEgressPolicy::with_global_ceiling(compiled([]), sandbox);

        let denied = layered.authorize(&request("api.example.test"));
        assert!(
            !denied.is_allowed(),
            "an empty ceiling is default-deny; absence of a ceiling is sandbox_only()"
        );
        assert!(denied.reason().contains("allow-ceiling"));
    }

    #[test]
    fn ceiling_gates_the_pre_resolution_decision_point_too() {
        let global = compiled([https_rule("ceiling-api", "api.example.test")]);
        let sandbox = compiled([
            https_rule("sandbox-api", "api.example.test"),
            https_rule("sandbox-internal", "internal.example.test"),
        ]);
        let layered = LayeredEgressPolicy::with_global_ceiling(global, sandbox);

        let denied =
            layered.authorize_hostname_without_resolved_ip(&request("internal.example.test"));
        assert!(!denied.is_allowed());
        assert!(denied.reason().contains("allow-ceiling"));
        assert!(
            layered
                .authorize_hostname_without_resolved_ip(&request("api.example.test"))
                .is_allowed()
        );
    }

    #[test]
    fn composition_stays_in_memory_nanosecond_scale() {
        // Performance invariant (plan EE2): the ceiling adds one in-memory
        // authorize() per request. 100k layered authorizations complete well
        // inside a generous wall bound — no I/O, no allocation cliffs. The
        // bound is deliberately loose so this cannot flake on slow CI.
        let global = compiled([https_rule("ceiling-api", "api.example.test")]);
        let sandbox = compiled([https_rule("sandbox-api", "api.example.test")]);
        let layered = LayeredEgressPolicy::with_global_ceiling(global, sandbox);
        let req = request("api.example.test");

        let start = std::time::Instant::now();
        for _ in 0..100_000 {
            assert!(layered.authorize(&req).is_allowed());
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "100k layered authorizations took {elapsed:?}; composition must stay \
             in-memory and per-request-cheap"
        );
    }
}
