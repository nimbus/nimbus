use nimbus_storage::{
    LogicalLeafKey, LogicalLeafKind, MaterializedVerificationIndex,
    VERIFICATION_INDEX_BUDGETED_BYTES_PER_LEAF, VERIFICATION_INDEX_MAX_DEPTH,
    VERIFICATION_INDEX_MAX_RESIDENT_BYTES_PER_LEAF,
};

const MILLION_LEAVES: usize = 1_000_000;

#[test]
#[ignore = "the million-leaf verification root runs in its dedicated plan gate"]
fn verification_root_million_leaf_depth_and_memory_meet_imv2_limits() {
    let index = MaterializedVerificationIndex::from_leaves((0..MILLION_LEAVES).map(|rank| {
        let identity = (rank as u64).to_be_bytes();
        let key = LogicalLeafKey::new(LogicalLeafKind::Document, &identity)
            .expect("rank identity should produce a logical leaf key");
        (key, identity)
    }))
    .expect("million-leaf verification root should build");

    eprintln!(
        "verification_root leaves={} max_depth={} resident_bytes_per_leaf={} budgeted_bytes_per_leaf={}",
        index.len(),
        index.max_depth(),
        index.resident_bytes_per_leaf(),
        VERIFICATION_INDEX_BUDGETED_BYTES_PER_LEAF,
    );

    assert_eq!(index.len(), MILLION_LEAVES);
    assert!(
        index.max_depth() <= VERIFICATION_INDEX_MAX_DEPTH,
        "deterministic treap depth {} exceeds the measured safety bound {}",
        index.max_depth(),
        VERIFICATION_INDEX_MAX_DEPTH
    );
    assert!(
        index.resident_bytes_per_leaf() <= VERIFICATION_INDEX_MAX_RESIDENT_BYTES_PER_LEAF,
        "resident bytes per leaf {} exceed the approved limit {}",
        index.resident_bytes_per_leaf(),
        VERIFICATION_INDEX_MAX_RESIDENT_BYTES_PER_LEAF
    );
}
