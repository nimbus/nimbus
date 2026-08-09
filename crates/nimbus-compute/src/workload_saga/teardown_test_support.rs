pub(super) fn production_source(source: &str) -> &str {
    source.split("#[cfg(test)]").next().unwrap_or(source)
}

pub(super) fn assert_tokens_in_order(source: &str, tokens: &[&str]) {
    let mut cursor = 0;
    for token in tokens {
        let Some(offset) = source[cursor..].find(token) else {
            panic!("production source does not contain required token `{token}`");
        };
        cursor += offset + token.len();
    }
}
