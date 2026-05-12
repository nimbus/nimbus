use super::*;

impl LicenseState {
    pub fn snapshot(&self) -> LicenseSnapshot {
        self.snapshot_with_usage(None)
    }

    pub fn snapshot_with_usage(
        &self,
        usage: Option<MonthlyActiveUsersSnapshot>,
    ) -> LicenseSnapshot {
        let mut warnings = Vec::new();
        let now = current_time_unix_ms();
        let status = match self.document.kind {
            LicenseKind::Community => LicenseStatus::Community,
            LicenseKind::Trial => {
                if let Some(expires_at) = self.document.trial_expires_at_unix_ms {
                    if expires_at <= now {
                        warnings.push("trial license has expired".to_string());
                        LicenseStatus::TrialExpired
                    } else {
                        maybe_warn_about_window(
                            &mut warnings,
                            expires_at,
                            now,
                            14,
                            "trial license expires soon",
                        );
                        LicenseStatus::TrialActive
                    }
                } else {
                    warnings.push("trial license has no expiration timestamp".to_string());
                    LicenseStatus::TrialActive
                }
            }
            LicenseKind::Enterprise => {
                if let Some(expires_at) = self.document.expires_at_unix_ms {
                    if expires_at <= now {
                        warnings.push("enterprise license has expired".to_string());
                        LicenseStatus::EnterpriseExpired
                    } else {
                        maybe_warn_about_window(
                            &mut warnings,
                            expires_at,
                            now,
                            30,
                            "enterprise license expires soon",
                        );
                        LicenseStatus::EnterpriseActive
                    }
                } else {
                    LicenseStatus::EnterpriseActive
                }
            }
        };
        let usage = usage.map(|usage| {
            let limit = self.document.monthly_active_user_limit;
            let limit_exceeded = limit.map(|limit| usage.monthly_active_users > limit);
            if let Some(limit) = limit {
                if usage.monthly_active_users > limit {
                    match self.document.kind {
                        LicenseKind::Enterprise => warnings.push(
                            "observed monthly active users exceed the licensed limit".to_string(),
                        ),
                        LicenseKind::Community | LicenseKind::Trial => warnings.push(
                            "observed monthly active users exceed the configured limit; enterprise licensing may be required depending on annual revenue".to_string(),
                        ),
                    }
                } else if limit > 0
                    && usage.monthly_active_users.saturating_mul(10)
                        >= limit.saturating_mul(9)
                {
                    warnings.push(
                        "observed monthly active users are approaching the configured limit"
                            .to_string(),
                    );
                }
            }
            LicenseUsageSnapshot {
                month: usage.month,
                month_start_unix_ms: usage.month_start_unix_ms,
                monthly_active_users: usage.monthly_active_users,
                last_recorded_at_unix_ms: usage.last_recorded_at_unix_ms,
                limit,
                limit_exceeded,
            }
        });

        LicenseSnapshot {
            source: self.source.clone(),
            kind: self.document.kind,
            status,
            issued_to: self.document.issued_to.clone(),
            issued_by: self.document.issued_by.clone(),
            issued_at_unix_ms: self.document.issued_at_unix_ms,
            expires_at_unix_ms: self.document.expires_at_unix_ms,
            trial_expires_at_unix_ms: self.document.trial_expires_at_unix_ms,
            revenue_limit_usd: self.document.revenue_limit_usd,
            monthly_active_user_limit: self.document.monthly_active_user_limit,
            entitlements: self.document.entitlements.clone(),
            usage,
            warnings,
        }
    }
}

pub(super) fn current_time_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn maybe_warn_about_window(
    warnings: &mut Vec<String>,
    expires_at_unix_ms: u64,
    now_unix_ms: u64,
    days: u64,
    message: &str,
) {
    let remaining_ms = expires_at_unix_ms.saturating_sub(now_unix_ms);
    let warning_window_ms = days * 24 * 60 * 60 * 1000;
    if remaining_ms <= warning_window_ms {
        warnings.push(message.to_string());
    }
}
