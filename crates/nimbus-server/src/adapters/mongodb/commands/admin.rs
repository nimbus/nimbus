use super::super::connection::ConnectionState;
use super::super::error::MongoError;

pub fn whatsmyuri(conn: &ConnectionState) -> Result<bson::Document, MongoError> {
    Ok(bson::doc! {
        "you": conn.remote_addr.to_string(),
        "ok": 1.0,
    })
}

pub fn get_parameter(body: &bson::Document) -> Result<bson::Document, MongoError> {
    let mut doc = bson::Document::new();

    if body.get_bool("showDetails").unwrap_or(false) {
        for key in body.keys() {
            match key.as_str() {
                "getParameter" | "$db" | "showDetails" | "allParameters" => {}
                param => {
                    doc.insert(
                        param,
                        bson::doc! {
                            "value": bson::Bson::Null,
                            "settableAtRuntime": false,
                            "settableAtStartup": false,
                        },
                    );
                }
            }
        }
    } else {
        for key in body.keys() {
            match key.as_str() {
                "getParameter" | "$db" => {}
                param => {
                    doc.insert(param, bson::Bson::Null);
                }
            }
        }
    }

    doc.insert("ok", 1.0);
    Ok(doc)
}

pub fn server_status() -> Result<bson::Document, MongoError> {
    Ok(bson::doc! {
        "host": "localhost",
        "version": "7.0.0",
        "process": "nimbus",
        "pid": std::process::id() as i64,
        "uptime": 1.0,
        "uptimeMillis": 1000_i64,
        "uptimeEstimate": 1_i64,
        "localTime": bson::DateTime::now(),
        "connections": {
            "current": 1,
            "available": 1000,
            "totalCreated": 1_i64,
        },
        "ok": 1.0,
    })
}

pub fn connection_status(conn: &ConnectionState) -> Result<bson::Document, MongoError> {
    let auth_info = if conn.authenticated {
        bson::doc! {
            "authenticatedUsers": [{ "user": "admin", "db": "admin" }],
            "authenticatedUserRoles": [],
        }
    } else {
        bson::doc! {
            "authenticatedUsers": bson::Bson::Array(vec![]),
            "authenticatedUserRoles": bson::Bson::Array(vec![]),
        }
    };

    Ok(bson::doc! {
        "authInfo": auth_info,
        "ok": 1.0,
    })
}

pub fn get_cmd_line_opts() -> Result<bson::Document, MongoError> {
    Ok(bson::doc! {
        "argv": bson::Bson::Array(vec!["nimbus".into()]),
        "parsed": {},
        "ok": 1.0,
    })
}

pub fn get_free_monitoring_status() -> Result<bson::Document, MongoError> {
    Ok(bson::doc! {
        "state": "disabled",
        "ok": 1.0,
    })
}

pub fn get_log(body: &bson::Document) -> Result<bson::Document, MongoError> {
    let filter = body.get_str("getLog").unwrap_or("global");
    match filter {
        "*" => Ok(bson::doc! {
            "names": ["global", "startupWarnings"],
            "ok": 1.0,
        }),
        _ => Ok(bson::doc! {
            "log": bson::Bson::Array(vec![]),
            "totalLinesWritten": 0,
            "ok": 1.0,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> ConnectionState {
        ConnectionState::new(([192, 168, 1, 100], 54321).into())
    }

    #[test]
    fn whatsmyuri_returns_client_address() {
        let conn = test_conn();
        let doc = whatsmyuri(&conn).unwrap();
        assert_eq!(doc.get_str("you").unwrap(), "192.168.1.100:54321");
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn get_parameter_returns_null_for_unknown() {
        let body = bson::doc! { "getParameter": 1, "featureCompatibilityVersion": 1 };
        let doc = get_parameter(&body).unwrap();
        assert!(
            doc.get("featureCompatibilityVersion")
                .unwrap()
                .as_null()
                .is_some()
        );
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn get_parameter_with_show_details() {
        let body = bson::doc! {
            "getParameter": 1,
            "showDetails": true,
            "authenticationMechanisms": 1,
        };
        let doc = get_parameter(&body).unwrap();
        let detail = doc.get_document("authenticationMechanisms").unwrap();
        assert!(detail.get("value").is_some());
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn server_status_returns_version() {
        let doc = server_status().unwrap();
        assert_eq!(doc.get_str("version").unwrap(), "7.0.0");
        assert_eq!(doc.get_str("process").unwrap(), "nimbus");
        assert!(doc.get_i64("pid").is_ok());
        assert!(doc.get_document("connections").is_ok());
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn connection_status_unauthenticated() {
        let conn = test_conn();
        let doc = connection_status(&conn).unwrap();
        let auth = doc.get_document("authInfo").unwrap();
        let users = auth.get_array("authenticatedUsers").unwrap();
        assert!(users.is_empty());
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn connection_status_authenticated() {
        let mut conn = test_conn();
        conn.authenticated = true;
        let doc = connection_status(&conn).unwrap();
        let auth = doc.get_document("authInfo").unwrap();
        let users = auth.get_array("authenticatedUsers").unwrap();
        assert_eq!(users.len(), 1);
    }

    #[test]
    fn get_cmd_line_opts_returns_ok() {
        let doc = get_cmd_line_opts().unwrap();
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
        assert!(doc.get_array("argv").is_ok());
    }

    #[test]
    fn get_free_monitoring_status_returns_disabled() {
        let doc = get_free_monitoring_status().unwrap();
        assert_eq!(doc.get_str("state").unwrap(), "disabled");
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn get_log_star_returns_names() {
        let body = bson::doc! { "getLog": "*" };
        let doc = get_log(&body).unwrap();
        assert!(doc.get_array("names").is_ok());
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn get_log_global_returns_empty() {
        let body = bson::doc! { "getLog": "global" };
        let doc = get_log(&body).unwrap();
        let log = doc.get_array("log").unwrap();
        assert!(log.is_empty());
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
    }
}
