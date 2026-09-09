use super::support::*;

fn ordered_schema(table: &TableName) -> (TableSchema, nimbus_core::IndexId) {
    let by_rank = nimbus_core::IndexId::new();
    let table_schema = TableSchema {
        table: table.clone(),
        fields: vec![
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: true,
            },
            FieldSchema {
                name: "label".to_string(),
                field_type: FieldType::String,
                required: true,
            },
        ],
        indexes: vec![
            nimbus_core::IndexDefinition {
                id: by_rank.clone(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_rank".to_string(),
                fields: vec!["rank".to_string()],
            },
            nimbus_core::IndexDefinition {
                id: nimbus_core::IndexId::new(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_label".to_string(),
                fields: vec!["label".to_string()],
            },
        ],
        access_policy: None,
    };
    (table_schema, by_rank)
}

fn ranked(table: &TableName, rank: u64, label: &str) -> Document {
    Document::new(
        table.clone(),
        serde_json::Map::from_iter([
            ("rank".to_string(), serde_json::json!(rank)),
            ("label".to_string(), serde_json::json!(label)),
        ]),
    )
}

fn ids(documents: &[Document]) -> Vec<String> {
    documents
        .iter()
        .map(|document| document.id.to_string())
        .collect()
}

/// The keyspace stores the same order-preserving tuple encoding as the
/// embedded redb index, so a MySQL range scan returns numbers in numeric
/// order and strings in lexicographic order, equal to redb on the same
/// documents. A text or hash key would put 10 before 2 in both cases.
#[tokio::test(flavor = "multi_thread")]
async fn mysql_index_range_scans_order_numbers_numerically() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("index-order").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let redb = TenantStore::create_in_memory().expect("redb store should open");
        let table = TableName::new("ranked").expect("table name should build");
        let (table_schema, _) = ordered_schema(&table);
        let documents = [
            ranked(&table, 2, "2"),
            ranked(&table, 10, "10"),
            ranked(&table, 9, "9"),
        ];
        opened
            .store
            .replace_table_schema(&table_schema)
            .expect("mysql schema should apply");
        redb.replace_table_schema(&table_schema)
            .expect("redb schema should apply");
        for document in &documents {
            opened
                .store
                .insert(document)
                .expect("mysql insert should succeed");
            redb.insert_with_indexes(document, &table_schema.indexes)
                .expect("redb insert should succeed");
        }

        let one = serde_json::json!(1);
        let mysql_numbers = opened
            .store
            .index_scan_range_cancellable(
                &table,
                "by_rank",
                std::ops::Bound::Included(&one),
                std::ops::Bound::Unbounded,
                &mut || Ok(()),
            )
            .expect("mysql number range should read");
        let redb_numbers = redb
            .index_scan_range_cancellable(
                &table,
                "by_rank",
                std::ops::Bound::Included(&one),
                std::ops::Bound::Unbounded,
                &mut || Ok(()),
            )
            .expect("redb number range should read");
        let numeric_order = vec![
            documents[0].id.to_string(),
            documents[2].id.to_string(),
            documents[1].id.to_string(),
        ];
        assert_eq!(
            ids(&mysql_numbers),
            numeric_order,
            "2, 9, 10 in numeric order"
        );
        assert_eq!(
            ids(&redb_numbers),
            numeric_order,
            "redb agrees on the numeric order"
        );

        let one_text = serde_json::json!("1");
        let mysql_strings = opened
            .store
            .index_scan_range_cancellable(
                &table,
                "by_label",
                std::ops::Bound::Included(&one_text),
                std::ops::Bound::Unbounded,
                &mut || Ok(()),
            )
            .expect("mysql string range should read");
        let redb_strings = redb
            .index_scan_range_cancellable(
                &table,
                "by_label",
                std::ops::Bound::Included(&one_text),
                std::ops::Bound::Unbounded,
                &mut || Ok(()),
            )
            .expect("redb string range should read");
        let lexicographic_order = vec![
            documents[1].id.to_string(),
            documents[0].id.to_string(),
            documents[2].id.to_string(),
        ];
        assert_eq!(
            ids(&mysql_strings),
            lexicographic_order,
            "\"10\", \"2\", \"9\" in lexicographic order"
        );
        assert_eq!(
            ids(&redb_strings),
            lexicographic_order,
            "redb agrees on the lexicographic order"
        );
    })
    .await;
}

/// The candidate select is a range over `idx_index_entries_tuple` joined to
/// `documents` by primary key. No step of the plan scans a whole table, so
/// an index read stays bounded by the matching keyspace rows however large
/// the table grows.
#[tokio::test(flavor = "multi_thread")]
async fn mysql_index_reads_use_the_keyspace_not_a_table_scan() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("index-plan").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");
        let table = TableName::new("ranked").expect("table name should build");
        let (table_schema, by_rank) = ordered_schema(&table);
        opened
            .store
            .replace_table_schema(&table_schema)
            .expect("schema should apply");
        for rank in 0..128u64 {
            opened
                .store
                .insert(&ranked(&table, rank, &rank.to_string()))
                .expect("document should insert");
        }
        let table_id = opened
            .store
            .table_id(&table)
            .expect("table id should load")
            .expect("table should have an id");

        let low = serde_json::json!(40);
        let high = serde_json::json!(48);
        let (start_key, end_key) = match crate::sql::index_keyspace::IndexTupleScanBounds::for_scan(
            &[],
            std::ops::Bound::Included(&low),
            std::ops::Bound::Excluded(&high),
        )
        .expect("bounds should plan")
        {
            crate::sql::index_keyspace::IndexTupleScanBounds::Bounds { start_key, end_key } => {
                (start_key, end_key)
            }
            crate::sql::index_keyspace::IndexTupleScanBounds::Empty => {
                panic!("a number range should plan a byte range")
            }
        };
        let (sql, params) = crate::mysql::index_entries::index_candidate_select(
            opened.store.database_name(),
            &table_id,
            &by_rank,
            start_key,
            end_key,
        );

        let plan = explain_rows(&config.connection_string, &sql, params).await;
        let keyspace = plan
            .iter()
            .find(|row| row.table == "e")
            .expect("the plan should read index_entries");
        assert_eq!(keyspace.access_type, "range", "plan: {plan:?}");
        assert_eq!(
            keyspace.key.as_deref(),
            Some("idx_index_entries_tuple"),
            "plan: {plan:?}"
        );
        let documents = plan
            .iter()
            .find(|row| row.table == "d")
            .expect("the plan should join documents");
        assert_eq!(documents.access_type, "eq_ref", "plan: {plan:?}");
        assert_eq!(documents.key.as_deref(), Some("PRIMARY"), "plan: {plan:?}");
        assert!(
            plan.iter()
                .all(|row| row.access_type != "ALL" && row.access_type != "index"),
            "no step of the plan scans a whole table: {plan:?}"
        );

        let read = opened
            .store
            .index_scan_range_cancellable(
                &table,
                "by_rank",
                std::ops::Bound::Included(&low),
                std::ops::Bound::Excluded(&high),
                &mut || Ok(()),
            )
            .expect("range read should succeed");
        assert_eq!(
            read.iter()
                .map(|document| document.fields["rank"].as_u64().expect("rank is a number"))
                .collect::<Vec<_>>(),
            (40..48).collect::<Vec<_>>()
        );
    })
    .await;
}
