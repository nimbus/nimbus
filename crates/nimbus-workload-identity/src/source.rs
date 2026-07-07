use nimbus_crypto::IdentityPublicKey;
use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};

/// Canonical machine identity record.
///
/// This is the key-derived canonical id SI2 introduces, not the workload
/// enforcement string used by the node/workload ladder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineIdentityRecord {
    id: String,
    public_key: IdentityPublicKey,
    source: IdentitySourceKind,
}

impl MachineIdentityRecord {
    pub fn local_dev(public_key: &IdentityPublicKey) -> Self {
        Self {
            id: machine_fingerprint(public_key),
            public_key: *public_key,
            source: IdentitySourceKind::LocalDev,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn public_key(&self) -> IdentityPublicKey {
        self.public_key
    }

    pub fn source(&self) -> &IdentitySourceKind {
        &self.source
    }
}

impl Serialize for MachineIdentityRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut record = serializer.serialize_struct("MachineIdentityRecord", 3)?;
        record.serialize_field("id", &self.id)?;
        record.serialize_field("public_key", &self.public_key.to_hex())?;
        record.serialize_field("source", &self.source)?;
        record.end()
    }
}

/// Canonical node identity record.
///
/// This is the key-derived canonical id SI2 introduces, not the workload
/// enforcement string used by the node/workload ladder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeIdentityRecord {
    id: String,
    public_key: IdentityPublicKey,
    source: IdentitySourceKind,
}

impl NodeIdentityRecord {
    pub fn local_dev(public_key: &IdentityPublicKey) -> Self {
        Self {
            id: public_key.fingerprint(),
            public_key: *public_key,
            source: IdentitySourceKind::LocalDev,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn public_key(&self) -> IdentityPublicKey {
        self.public_key
    }

    pub fn source(&self) -> &IdentitySourceKind {
        &self.source
    }
}

impl Serialize for NodeIdentityRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut record = serializer.serialize_struct("NodeIdentityRecord", 3)?;
        record.serialize_field("id", &self.id)?;
        record.serialize_field("public_key", &self.public_key.to_hex())?;
        record.serialize_field("source", &self.source)?;
        record.end()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentitySourceKind {
    /// Explicitly non-production.
    LocalDev,
    /// Reserved for HS1 membership-bound identity. The variant carries a
    /// [`MembershipAttestation`] with no public constructor, so this source
    /// cannot be forged by naming the variant — HS1 must introduce the
    /// membership proof before any value of this kind can exist.
    ClusterMembership(MembershipAttestation),
}

/// Proof that a node identity is bound to committed cluster membership.
///
/// SI2 deliberately provides no way to construct this — production trust
/// (`TrustMode::Production`) is structurally unreachable until HS1 delivers
/// membership-bound identity and adds the constructor alongside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MembershipAttestation {
    _reserved: (),
}

fn machine_fingerprint(public_key: &IdentityPublicKey) -> String {
    let node_fingerprint = public_key.fingerprint();
    format!("mk_{}", &node_fingerprint["nk_".len()..])
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn id_derivation_is_deterministic_and_uses_expected_prefixes() {
        let public_key = IdentityPublicKey::from_ed25519_bytes([0x42; 32]);

        let node_a = NodeIdentityRecord::local_dev(&public_key);
        let node_b = NodeIdentityRecord::local_dev(&public_key);
        let machine_a = MachineIdentityRecord::local_dev(&public_key);
        let machine_b = MachineIdentityRecord::local_dev(&public_key);

        assert_eq!(node_a.id(), node_b.id());
        assert_eq!(machine_a.id(), machine_b.id());
        assert!(node_a.id().starts_with("nk_"));
        assert!(machine_a.id().starts_with("mk_"));
        assert_eq!(&node_a.id()["nk_".len()..], &machine_a.id()["mk_".len()..]);
        assert_eq!(node_a.source(), &IdentitySourceKind::LocalDev);
        assert_eq!(machine_a.source(), &IdentitySourceKind::LocalDev);
    }

    #[test]
    fn records_serialize_public_identity_without_private_material() {
        let public_key = IdentityPublicKey::from_ed25519_bytes([0x7a; 32]);
        let node = NodeIdentityRecord::local_dev(&public_key);
        let machine = MachineIdentityRecord::local_dev(&public_key);

        assert_record_json(
            serde_json::to_value(&node).expect("node should serialize"),
            node.id(),
            "nk_",
            &public_key.to_hex(),
        );
        assert_record_json(
            serde_json::to_value(&machine).expect("machine should serialize"),
            machine.id(),
            "mk_",
            &public_key.to_hex(),
        );
    }

    fn assert_record_json(value: Value, id: &str, prefix: &str, public_key_hex: &str) {
        assert_eq!(value["id"], id);
        assert!(
            value["id"]
                .as_str()
                .expect("id should be a string")
                .starts_with(prefix)
        );
        assert_eq!(value["public_key"], public_key_hex);
        assert_eq!(value["source"], "local_dev");

        let serialized = serde_json::to_string(&value).expect("record should serialize");
        for forbidden in [
            "private_key",
            "secret",
            "seed",
            "pkcs8",
            "credential",
            "token",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "record leaked private material marker `{forbidden}`: {serialized}"
            );
        }
    }
}
