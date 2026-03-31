use neovex_core::{DocumentId, TableName};

/// Builds the primary document key.
pub fn document_key(table: &TableName, id: &DocumentId) -> Vec<u8> {
    let mut key = table_prefix(table);
    key.extend_from_slice(&id.to_bytes());
    key
}

/// Returns the table key prefix.
pub fn table_prefix(table: &TableName) -> Vec<u8> {
    let mut prefix = table.as_str().as_bytes().to_vec();
    prefix.push(0);
    prefix
}

/// Returns the exclusive upper bound for a prefix range.
pub fn prefix_end(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut end = prefix.to_vec();
    for index in (0..end.len()).rev() {
        if end[index] != u8::MAX {
            end[index] += 1;
            end.truncate(index + 1);
            return Some(end);
        }
    }
    // This can only happen when the prefix is entirely 0xFF bytes. With the
    // validated ASCII table names used by Neovex this is not expected in
    // practice, but keeping the fallback preserves correctness if key
    // encoding changes later.
    None
}
