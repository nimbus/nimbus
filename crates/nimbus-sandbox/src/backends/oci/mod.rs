pub(crate) mod buildah;
pub(crate) mod builder;
pub(crate) mod command;
pub(crate) mod conmon;
pub(crate) mod durable_directory;
pub(crate) mod egress;
pub(crate) mod hardening;
pub(crate) mod materializer;
pub(crate) mod network;
pub(crate) mod port_lease;
pub(crate) mod port_manager;
pub(crate) mod resource_quota;

/// Deserialize an explicitly present nullable field.
///
/// Serde otherwise treats a missing `Option<T>` field as `None`, which is too
/// permissive for manifest authority fields where omission and an explicit
/// post-adoption `null` have different wire meanings.
pub(crate) fn deserialize_required_option<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(deserializer)
}
