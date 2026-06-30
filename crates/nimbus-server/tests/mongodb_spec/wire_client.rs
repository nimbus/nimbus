use std::sync::atomic::{AtomicI32, Ordering};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use hmac::{Hmac, Mac};
use nimbus_mongodb::wire::OP_MSG;
use sha2::Sha256;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

type HmacSha256 = Hmac<Sha256>;

static NEXT_REQUEST: AtomicI32 = AtomicI32::new(1);

pub struct WireClient {
    stream: TcpStream,
}

impl WireClient {
    pub async fn connect(addr: std::net::SocketAddr) -> Self {
        let stream = TcpStream::connect(addr).await.expect("connect to listener");
        Self { stream }
    }

    pub async fn command(&mut self, doc: &bson::Document) -> bson::Document {
        let request_id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        let body_bytes = bson::serialize_to_vec(doc).expect("serialize command");

        let flag_bits: u32 = 0;
        let payload_len = 4 + 1 + body_bytes.len();
        let message_length = (16 + payload_len) as i32;

        let mut buf = Vec::new();
        buf.extend_from_slice(&message_length.to_le_bytes());
        buf.extend_from_slice(&request_id.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&OP_MSG.to_le_bytes());
        buf.extend_from_slice(&flag_bits.to_le_bytes());
        buf.push(0);
        buf.extend_from_slice(&body_bytes);

        self.stream.write_all(&buf).await.expect("write command");

        let mut header_buf = [0u8; 16];
        self.stream
            .read_exact(&mut header_buf)
            .await
            .expect("read response header");

        let msg_len =
            i32::from_le_bytes([header_buf[0], header_buf[1], header_buf[2], header_buf[3]]);

        let body_len = (msg_len as usize) - 16;
        let mut body = vec![0u8; body_len];
        self.stream
            .read_exact(&mut body)
            .await
            .expect("read response body");

        // skip flags (4 bytes) + section kind (1 byte)
        bson::deserialize_from_slice(&body[5..]).expect("deserialize response")
    }

    pub async fn authenticate(&mut self, username: &str, password: &str) -> Result<(), String> {
        let client_nonce = "specrunnernonce";
        let client_first_bare = format!("n={username},r={client_nonce}");
        let client_first = format!("n,,{client_first_bare}");
        let step1 = self
            .command(&bson::doc! {
                "saslStart": 1,
                "mechanism": "SCRAM-SHA-256",
                "payload": bson::Binary {
                    subtype: bson::spec::BinarySubtype::Generic,
                    bytes: client_first.as_bytes().to_vec(),
                },
                "$db": "admin",
            })
            .await;
        if step1.get_f64("ok").unwrap_or(0.0) != 1.0 {
            return Err(format!(
                "saslStart failed: {}",
                step1.get_str("errmsg").unwrap_or("unknown")
            ));
        }

        let server_first_payload = step1
            .get_binary_generic("payload")
            .map_err(|error| format!("saslStart missing payload: {error}"))?;
        let server_first = std::str::from_utf8(server_first_payload.as_slice())
            .map_err(|error| format!("saslStart payload is not UTF-8: {error}"))?;
        let mut server_nonce = String::new();
        let mut salt_b64 = String::new();
        let mut iterations = 0_u32;
        for part in server_first.split(',') {
            if let Some(value) = part.strip_prefix("r=") {
                server_nonce = value.to_string();
            } else if let Some(value) = part.strip_prefix("s=") {
                salt_b64 = value.to_string();
            } else if let Some(value) = part.strip_prefix("i=") {
                iterations = value
                    .parse()
                    .map_err(|error| format!("invalid SCRAM iteration count: {error}"))?;
            }
        }
        if server_nonce.is_empty() || salt_b64.is_empty() || iterations == 0 {
            return Err(format!(
                "malformed SCRAM server-first-message: {server_first}"
            ));
        }

        let salt = BASE64
            .decode(salt_b64)
            .map_err(|error| format!("invalid SCRAM salt: {error}"))?;
        let salted_password = derive_salted_password(password, &salt, iterations);
        let client_key = compute_hmac(&salted_password, b"Client Key");
        let stored_key = sha256_hash(&client_key);
        let channel_binding = BASE64.encode(b"n,,");
        let client_final_without_proof = format!("c={channel_binding},r={server_nonce}");
        let auth_message =
            format!("{client_first_bare},{server_first},{client_final_without_proof}");
        let client_signature = compute_hmac(&stored_key, auth_message.as_bytes());
        let mut proof = client_key;
        for (i, byte) in client_signature.iter().enumerate() {
            proof[i] ^= byte;
        }
        let client_final = format!("{client_final_without_proof},p={}", BASE64.encode(proof));

        let step2 = self
            .command(&bson::doc! {
                "saslContinue": 1,
                "conversationId": step1.get_i32("conversationId").unwrap_or_default(),
                "payload": bson::Binary {
                    subtype: bson::spec::BinarySubtype::Generic,
                    bytes: client_final.as_bytes().to_vec(),
                },
                "$db": "admin",
            })
            .await;
        if step2.get_f64("ok").unwrap_or(0.0) != 1.0 || !step2.get_bool("done").unwrap_or(false) {
            return Err(format!(
                "saslContinue failed: {}",
                step2.get_str("errmsg").unwrap_or("unknown")
            ));
        }
        Ok(())
    }

    pub async fn insert(
        &mut self,
        db: &str,
        collection: &str,
        documents: &[bson::Document],
    ) -> bson::Document {
        let docs_bson: Vec<bson::Bson> = documents
            .iter()
            .map(|d| bson::Bson::Document(d.clone()))
            .collect();
        let cmd = bson::doc! {
            "insert": collection,
            "documents": docs_bson,
            "$db": db,
        };
        self.command(&cmd).await
    }

    pub async fn find(
        &mut self,
        db: &str,
        collection: &str,
        filter: bson::Document,
        options: bson::Document,
    ) -> Result<Vec<bson::Document>, String> {
        let mut cmd = bson::doc! {
            "find": collection,
            "filter": filter,
            "$db": db,
        };

        for (k, v) in options {
            cmd.insert(k, v);
        }

        let response = self.command(&cmd).await;

        if response.get_f64("ok").unwrap_or(0.0) != 1.0 {
            return Err(response
                .get_str("errmsg")
                .unwrap_or("unknown error")
                .to_string());
        }

        let cursor = response
            .get_document("cursor")
            .map_err(|e| format!("no cursor: {e}"))?;
        let first_batch = cursor
            .get_array("firstBatch")
            .map_err(|e| format!("no firstBatch: {e}"))?;

        let mut results: Vec<bson::Document> = first_batch
            .iter()
            .filter_map(|b| b.as_document().cloned())
            .collect();

        let cursor_id = cursor.get_i64("id").unwrap_or(0);
        if cursor_id != 0 {
            let ns = cursor.get_str("ns").unwrap_or("");
            let collection = ns.split('.').next_back().unwrap_or(collection);
            loop {
                let get_more_cmd = bson::doc! {
                    "getMore": cursor_id,
                    "collection": collection,
                    "$db": db,
                };
                let more_response = self.command(&get_more_cmd).await;
                let more_cursor = more_response
                    .get_document("cursor")
                    .map_err(|e| format!("no cursor in getMore: {e}"))?;
                let next_batch = more_cursor
                    .get_array("nextBatch")
                    .map_err(|e| format!("no nextBatch: {e}"))?;

                if next_batch.is_empty() {
                    break;
                }

                results.extend(next_batch.iter().filter_map(|b| b.as_document().cloned()));

                let next_id = more_cursor.get_i64("id").unwrap_or(0);
                if next_id == 0 {
                    break;
                }
            }
        }

        Ok(results)
    }

    pub async fn update(
        &mut self,
        db: &str,
        collection: &str,
        filter: bson::Document,
        update: bson::Document,
        multi: bool,
    ) -> bson::Document {
        let cmd = bson::doc! {
            "update": collection,
            "updates": [{
                "q": filter,
                "u": update,
                "multi": multi,
            }],
            "$db": db,
        };
        self.command(&cmd).await
    }

    pub async fn delete(
        &mut self,
        db: &str,
        collection: &str,
        filter: bson::Document,
        limit: i32,
    ) -> bson::Document {
        let cmd = bson::doc! {
            "delete": collection,
            "deletes": [{
                "q": filter,
                "limit": limit,
            }],
            "$db": db,
        };
        self.command(&cmd).await
    }

    pub async fn aggregate(
        &mut self,
        db: &str,
        collection: &str,
        pipeline: Vec<bson::Document>,
    ) -> Result<Vec<bson::Document>, String> {
        let pipeline_bson: Vec<bson::Bson> =
            pipeline.into_iter().map(bson::Bson::Document).collect();
        let cmd = bson::doc! {
            "aggregate": collection,
            "pipeline": pipeline_bson,
            "cursor": {},
            "$db": db,
        };
        let response = self.command(&cmd).await;

        if response.get_f64("ok").unwrap_or(0.0) != 1.0 {
            return Err(response
                .get_str("errmsg")
                .unwrap_or("unknown error")
                .to_string());
        }

        let cursor = response
            .get_document("cursor")
            .map_err(|e| format!("no cursor: {e}"))?;
        let first_batch = cursor
            .get_array("firstBatch")
            .map_err(|e| format!("no firstBatch: {e}"))?;

        Ok(first_batch
            .iter()
            .filter_map(|b| b.as_document().cloned())
            .collect())
    }

    pub async fn drop_collection(&mut self, db: &str, collection: &str) -> bson::Document {
        let cmd = bson::doc! {
            "drop": collection,
            "$db": db,
        };
        self.command(&cmd).await
    }

    pub async fn start_session(&mut self) -> bson::Document {
        let cmd = bson::doc! {
            "startSession": 1,
            "$db": "admin",
        };
        self.command(&cmd).await
    }
}

fn derive_salted_password(password: &str, salt: &[u8], iterations: u32) -> Vec<u8> {
    let mut salted = vec![0_u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, iterations, &mut salted);
    salted
}

fn compute_hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hash(data: &[u8]) -> Vec<u8> {
    use sha2::Digest;
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}
