pub mod buildah;
pub mod builder;
pub mod command;
pub mod conmon;
pub mod durable_directory;
pub mod egress;
pub mod hardening;
pub mod materializer;
pub mod network;
pub mod port_lease;
pub mod port_lifecycle;
pub mod resource_quota;

/// Deserialize an explicitly present nullable field.
///
/// Serde otherwise treats a missing `Option<T>` field as `None`, which is too
/// permissive for manifest authority fields where omission and an explicit
/// post-adoption `null` have different wire meanings.
pub fn deserialize_required_option<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(deserializer)
}
