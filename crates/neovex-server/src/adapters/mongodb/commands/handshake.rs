use super::super::connection::ConnectionState;
use super::super::error::MongoError;

const NEOVEX_VERSION: &str = "7.0.0";
const MIN_WIRE_VERSION: i32 = 0;
const MAX_WIRE_VERSION: i32 = 21;
const MAX_BSON_OBJECT_SIZE: i32 = 16_777_216;
const MAX_MESSAGE_SIZE_BYTES: i32 = 48_000_000;
const MAX_WRITE_BATCH_SIZE: i32 = 100_000;
const LOGICAL_SESSION_TIMEOUT_MINUTES: i32 = 30;

pub fn hello(body: &bson::Document, conn: &ConnectionState) -> Result<bson::Document, MongoError> {
    let mut doc = base_hello_doc(conn);
    doc.insert("isWritablePrimary", true);

    if let Ok(true) = body.get_bool("helloOk") {
        doc.insert("helloOk", true);
    }

    handle_sasl_supported_mechs(body, &mut doc);
    Ok(doc)
}

pub fn is_master(
    body: &bson::Document,
    conn: &ConnectionState,
) -> Result<bson::Document, MongoError> {
    let mut doc = base_hello_doc(conn);
    doc.insert("ismaster", true);

    if let Ok(true) = body.get_bool("helloOk") {
        doc.insert("helloOk", true);
    }

    handle_sasl_supported_mechs(body, &mut doc);
    Ok(doc)
}

pub fn build_info() -> Result<bson::Document, MongoError> {
    let parts: Vec<&str> = NEOVEX_VERSION.split('.').collect();
    let major = parts
        .first()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    let minor = parts
        .get(1)
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    let patch = parts
        .get(2)
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);

    Ok(bson::doc! {
        "version": NEOVEX_VERSION,
        "gitVersion": env!("CARGO_PKG_VERSION"),
        "versionArray": [major, minor, patch, 0],
        "bits": 64,
        "debug": false,
        "maxBsonObjectSize": MAX_BSON_OBJECT_SIZE,
        "modules": bson::Bson::Array(vec![]),
        "ok": 1.0,
    })
}

fn base_hello_doc(conn: &ConnectionState) -> bson::Document {
    bson::doc! {
        "maxBsonObjectSize": MAX_BSON_OBJECT_SIZE,
        "maxMessageSizeBytes": MAX_MESSAGE_SIZE_BYTES,
        "maxWriteBatchSize": MAX_WRITE_BATCH_SIZE,
        "localTime": bson::DateTime::now(),
        "logicalSessionTimeoutMinutes": LOGICAL_SESSION_TIMEOUT_MINUTES,
        "connectionId": conn.connection_id,
        "minWireVersion": MIN_WIRE_VERSION,
        "maxWireVersion": MAX_WIRE_VERSION,
        "readOnly": false,
        "ok": 1.0,
    }
}

fn handle_sasl_supported_mechs(body: &bson::Document, response: &mut bson::Document) {
    if body.get_str("saslSupportedMechs").is_ok() {
        response.insert(
            "saslSupportedMechs",
            bson::Bson::Array(vec!["SCRAM-SHA-256".into()]),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> ConnectionState {
        ConnectionState::new(([127, 0, 0, 1], 12345).into())
    }

    #[test]
    fn hello_returns_required_fields() {
        let conn = test_conn();
        let body = bson::doc! { "hello": 1 };
        let doc = hello(&body, &conn).unwrap();

        assert!(doc.get_bool("isWritablePrimary").unwrap());
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
        assert_eq!(
            doc.get_i32("maxBsonObjectSize").unwrap(),
            MAX_BSON_OBJECT_SIZE
        );
        assert_eq!(
            doc.get_i32("maxMessageSizeBytes").unwrap(),
            MAX_MESSAGE_SIZE_BYTES
        );
        assert_eq!(
            doc.get_i32("maxWriteBatchSize").unwrap(),
            MAX_WRITE_BATCH_SIZE
        );
        assert_eq!(doc.get_i32("minWireVersion").unwrap(), MIN_WIRE_VERSION);
        assert_eq!(doc.get_i32("maxWireVersion").unwrap(), MAX_WIRE_VERSION);
        assert_eq!(
            doc.get_i32("logicalSessionTimeoutMinutes").unwrap(),
            LOGICAL_SESSION_TIMEOUT_MINUTES
        );
        assert!(doc.get_i64("connectionId").is_ok());
        assert!(doc.get_datetime("localTime").is_ok());
        assert!(!doc.get_bool("readOnly").unwrap());
    }

    #[test]
    fn hello_echoes_hello_ok() {
        let conn = test_conn();
        let body = bson::doc! { "hello": 1, "helloOk": true };
        let doc = hello(&body, &conn).unwrap();
        assert!(doc.get_bool("helloOk").unwrap());
    }

    #[test]
    fn hello_omits_hello_ok_when_not_sent() {
        let conn = test_conn();
        let body = bson::doc! { "hello": 1 };
        let doc = hello(&body, &conn).unwrap();
        assert!(doc.get_bool("helloOk").is_err());
    }

    #[test]
    fn hello_includes_sasl_mechs_when_requested() {
        let conn = test_conn();
        let body = bson::doc! { "hello": 1, "saslSupportedMechs": "admin.testuser" };
        let doc = hello(&body, &conn).unwrap();
        let mechs = doc.get_array("saslSupportedMechs").unwrap();
        assert_eq!(mechs.len(), 1);
        assert_eq!(mechs[0].as_str().unwrap(), "SCRAM-SHA-256");
    }

    #[test]
    fn is_master_returns_ismaster_field() {
        let conn = test_conn();
        let body = bson::doc! { "isMaster": 1 };
        let doc = is_master(&body, &conn).unwrap();

        assert!(doc.get_bool("ismaster").unwrap());
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
        assert_eq!(doc.get_i32("maxWireVersion").unwrap(), MAX_WIRE_VERSION);
    }

    #[test]
    fn is_master_with_hello_ok() {
        let conn = test_conn();
        let body = bson::doc! { "isMaster": 1, "helloOk": true };
        let doc = is_master(&body, &conn).unwrap();
        assert!(doc.get_bool("helloOk").unwrap());
        assert!(doc.get_bool("ismaster").unwrap());
    }

    #[test]
    fn build_info_returns_version_fields() {
        let doc = build_info().unwrap();

        assert_eq!(doc.get_str("version").unwrap(), NEOVEX_VERSION);
        assert!(doc.get_str("gitVersion").is_ok());
        let arr = doc.get_array("versionArray").unwrap();
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[0].as_i32().unwrap(), 7);
        assert_eq!(arr[1].as_i32().unwrap(), 0);
        assert_eq!(arr[2].as_i32().unwrap(), 0);
        assert_eq!(doc.get_i32("bits").unwrap(), 64);
        assert!(!doc.get_bool("debug").unwrap());
        assert_eq!(
            doc.get_i32("maxBsonObjectSize").unwrap(),
            MAX_BSON_OBJECT_SIZE
        );
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
    }
}
