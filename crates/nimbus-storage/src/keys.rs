use nimbus_core::{
    CollectionName, CollectionPath, DocumentId, DocumentLocator, DocumentPath, ResourcePathBinding,
    TableName, TriggerInvocationKey,
};

/// Builds the primary document key.
pub fn document_key(table: &TableName, id: &DocumentId) -> Vec<u8> {
    let mut key = table_prefix(table);
    key.extend_from_slice(id.as_str().as_bytes());
    key
}

/// Returns the table key prefix.
pub fn table_prefix(table: &TableName) -> Vec<u8> {
    let mut prefix = table.as_str().as_bytes().to_vec();
    prefix.push(0);
    prefix
}

/// Builds a self-delimiting key for an internal document locator.
pub fn resource_locator_key(locator: &DocumentLocator) -> Vec<u8> {
    let mut key = Vec::new();
    push_length_prefixed(&mut key, locator.table.as_str().as_bytes());
    push_length_prefixed(&mut key, locator.id.as_str().as_bytes());
    key
}

/// Builds a self-delimiting key for the full external document path.
pub fn document_path_key(path: &DocumentPath) -> Vec<u8> {
    let mut key = collection_path_key(path.collection_path());
    push_length_prefixed(&mut key, path.document_id().as_str().as_bytes());
    key
}

/// Builds a collection-group prefix used for group scans over path bindings.
pub fn collection_group_prefix(collection_group: &CollectionName) -> Vec<u8> {
    let mut prefix = Vec::new();
    push_length_prefixed(&mut prefix, collection_group.as_str().as_bytes());
    prefix
}

/// Builds a collection-group index key that remains prefix-scannable without
/// delimiter-based parsing.
pub fn collection_group_binding_key(binding: &ResourcePathBinding) -> Vec<u8> {
    let mut key = collection_group_prefix(binding.collection_group());
    key.extend_from_slice(&document_path_key(&binding.document_path));
    key
}

/// Builds a self-delimiting key for one durable trigger invocation.
pub fn trigger_invocation_key(key: &TriggerInvocationKey) -> Vec<u8> {
    let mut encoded = Vec::new();
    push_length_prefixed(&mut encoded, key.registration_id.as_bytes());
    push_length_prefixed(&mut encoded, key.event_id.as_bytes());
    encoded
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
    // validated ASCII table names used by Nimbus this is not expected in
    // practice, but keeping the fallback preserves correctness if key
    // encoding changes later.
    None
}

fn collection_path_key(path: &CollectionPath) -> Vec<u8> {
    let mut key = Vec::new();
    push_length_prefixed(&mut key, path.root_collection().as_str().as_bytes());
    let descendants_len = u16::try_from(path.descendants().len())
        .expect("collection path descendants should fit in u16");
    key.extend_from_slice(&descendants_len.to_be_bytes());
    for segment in path.descendants() {
        push_length_prefixed(&mut key, segment.document_id.as_str().as_bytes());
        push_length_prefixed(&mut key, segment.collection.as_str().as_bytes());
    }
    key
}

fn push_length_prefixed(buffer: &mut Vec<u8>, bytes: &[u8]) {
    let length = u16::try_from(bytes.len()).expect("path segment should fit in u16");
    buffer.extend_from_slice(&length.to_be_bytes());
    buffer.extend_from_slice(bytes);
}
