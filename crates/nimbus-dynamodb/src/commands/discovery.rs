//! DynamoDB discovery operations: DescribeEndpoints and DescribeLimits.
//!
//! Neither touches tenant data. `DescribeEndpoints` echoes the request's `Host`
//! so SDK endpoint discovery routes back to this listener; `DescribeLimits`
//! returns stubbed throughput limits (Nimbus does not meter provisioned
//! capacity — see the divergences doc) shaped exactly like the AWS response.

use extenddb_core::types::DescribeLimitsOutput;
use serde::Serialize;

/// AWS's documented default per-account/per-table on-demand throughput soft
/// limits (us-east-1). Nimbus does not enforce provisioned capacity, so these
/// are reported as a faithful stub rather than a metered value.
const ACCOUNT_MAX_RCU: i64 = 80_000;
const ACCOUNT_MAX_WCU: i64 = 80_000;
const TABLE_MAX_RCU: i64 = 40_000;
const TABLE_MAX_WCU: i64 = 40_000;

/// How long a client may cache a discovered endpoint, in minutes (AWS uses
/// 1440 = 24h for the regional DynamoDB endpoint).
const ENDPOINT_CACHE_PERIOD_MINUTES: i64 = 1440;

/// `DescribeEndpoints` response body (`{ "Endpoints": [ { "Address", ... } ] }`).
/// No matching type exists in `extenddb-core`, so it is defined locally.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DescribeEndpointsOutput {
    #[serde(rename = "Endpoints")]
    pub endpoints: Vec<Endpoint>,
}

/// A single endpoint entry in a `DescribeEndpoints` response.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Endpoint {
    #[serde(rename = "Address")]
    pub address: String,
    #[serde(rename = "CachePeriodInMinutes")]
    pub cache_period_in_minutes: i64,
}

/// DescribeEndpoints — report the endpoint the client should use. We echo the
/// request's `Host` (so a discovered endpoint points back at this listener);
/// the caller passes a sensible fallback when no `Host` header is present.
#[must_use]
pub fn describe_endpoints(host: &str) -> DescribeEndpointsOutput {
    DescribeEndpointsOutput {
        endpoints: vec![Endpoint {
            address: host.to_owned(),
            cache_period_in_minutes: ENDPOINT_CACHE_PERIOD_MINUTES,
        }],
    }
}

/// DescribeLimits — return stubbed account/table throughput limits.
#[must_use]
pub fn describe_limits() -> DescribeLimitsOutput {
    DescribeLimitsOutput {
        account_max_read_capacity_units: ACCOUNT_MAX_RCU,
        account_max_write_capacity_units: ACCOUNT_MAX_WCU,
        table_max_read_capacity_units: TABLE_MAX_RCU,
        table_max_write_capacity_units: TABLE_MAX_WCU,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn describe_endpoints_echoes_host_with_cache_period() {
        let output = describe_endpoints("localhost:8000");
        let value = serde_json::to_value(&output).expect("serialize");
        assert_eq!(
            value,
            json!({
                "Endpoints": [
                    { "Address": "localhost:8000", "CachePeriodInMinutes": 1440 }
                ]
            })
        );
    }

    #[test]
    fn describe_limits_reports_documented_default_shape() {
        let value = serde_json::to_value(describe_limits()).expect("serialize");
        assert_eq!(
            value,
            json!({
                "AccountMaxReadCapacityUnits": 80_000,
                "AccountMaxWriteCapacityUnits": 80_000,
                "TableMaxReadCapacityUnits": 40_000,
                "TableMaxWriteCapacityUnits": 40_000,
            })
        );
    }
}
