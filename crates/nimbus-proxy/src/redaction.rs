use url::Url;

pub fn redact_egress_decision_log_value(name: &str, value: &str) -> String {
    if name.eq_ignore_ascii_case("authorization")
        || name.eq_ignore_ascii_case("proxy-authorization")
        || name.eq_ignore_ascii_case("cookie")
        || value.to_ascii_lowercase().contains("bearer ")
    {
        return "<redacted>".to_string();
    }
    if let Ok(mut url) = Url::parse(value) {
        if !url.username().is_empty() || url.password().is_some() {
            let _ = url.set_username("<redacted>");
            let _ = url.set_password(None);
        }
        if url.query().is_some() {
            let query = url
                .query_pairs()
                .map(|(key, _)| format!("{key}=<redacted>"))
                .collect::<Vec<_>>()
                .join("&");
            url.set_query(Some(&query));
        }
        return url.to_string();
    }
    redact_query_values(value)
}

pub(crate) fn redact_query_values(value: &str) -> String {
    let Some((prefix, query)) = value.split_once('?') else {
        return value.to_owned();
    };
    let redacted = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let key = pair.split_once('=').map(|(key, _)| key).unwrap_or(pair);
            format!("{key}=<redacted>")
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{prefix}?{redacted}")
}
