use std::io::{self, Cursor, Read};

pub const OP_COMPRESSED: i32 = 2012;
pub const OP_MSG: i32 = 2013;
pub const OP_QUERY: i32 = 2004;

const HEADER_LEN: usize = 16;
const MAX_MESSAGE_SIZE: i32 = 48 * 1024 * 1024; // 48 MiB default
const FLAG_CHECKSUM_PRESENT: u32 = 1 << 0;
const FLAG_MORE_TO_COME: u32 = 1 << 1;
const FLAG_EXHAUST_ALLOWED: u32 = 1 << 16;
const REQUIRED_FLAG_MASK: u32 = 0x0000_FFFF;
const KNOWN_REQUIRED_FLAGS: u32 = FLAG_CHECKSUM_PRESENT | FLAG_MORE_TO_COME;

#[derive(Debug, Clone)]
pub struct MsgHeader {
    pub message_length: i32,
    pub request_id: i32,
    pub response_to: i32,
    pub op_code: i32,
}

impl MsgHeader {
    pub fn new(request_id: i32, response_to: i32, op_code: i32) -> Self {
        Self {
            message_length: 0,
            request_id,
            response_to,
            op_code,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Section {
    Body(Vec<u8>),
    DocumentSequence {
        identifier: String,
        documents: Vec<Vec<u8>>,
    },
}

#[derive(Debug, Clone)]
pub struct OpMsgFlags {
    pub checksum_present: bool,
    pub more_to_come: bool,
    pub exhaust_allowed: bool,
}

impl OpMsgFlags {
    fn from_bits(bits: u32) -> Result<Self, WireError> {
        let unknown_required = bits & REQUIRED_FLAG_MASK & !KNOWN_REQUIRED_FLAGS;
        if unknown_required != 0 {
            return Err(WireError::UnsupportedFlags(unknown_required));
        }
        Ok(Self {
            checksum_present: bits & FLAG_CHECKSUM_PRESENT != 0,
            more_to_come: bits & FLAG_MORE_TO_COME != 0,
            exhaust_allowed: bits & FLAG_EXHAUST_ALLOWED != 0,
        })
    }

    fn to_bits(&self) -> u32 {
        let mut bits = 0u32;
        if self.checksum_present {
            bits |= FLAG_CHECKSUM_PRESENT;
        }
        if self.more_to_come {
            bits |= FLAG_MORE_TO_COME;
        }
        if self.exhaust_allowed {
            bits |= FLAG_EXHAUST_ALLOWED;
        }
        bits
    }
}

#[derive(Debug, Clone)]
pub struct OpMsg {
    pub header: MsgHeader,
    pub flags: OpMsgFlags,
    pub sections: Vec<Section>,
    pub checksum: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum WireError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("message length {0} exceeds maximum {MAX_MESSAGE_SIZE}")]
    MessageTooLarge(i32),
    #[error("message length {0} is smaller than header ({HEADER_LEN})")]
    MessageTooSmall(i32),
    #[error("unsupported opcode {0}")]
    UnsupportedOpcode(i32),
    #[error("legacy opcode {0} is not supported")]
    LegacyOpcode(i32),
    #[error("unsupported required flag bits: 0x{0:04x}")]
    UnsupportedFlags(u32),
    #[error("unknown section kind {0}")]
    UnknownSectionKind(u8),
    #[error("OP_MSG must contain exactly one payload type 0 section")]
    MissingBody,
    #[error("OP_MSG must contain exactly one payload type 0 section, found multiple")]
    DuplicateBody,
    #[error("malformed BSON: {0}")]
    MalformedBson(String),
    #[error("CRC-32C checksum mismatch: expected 0x{expected:08x}, got 0x{actual:08x}")]
    ChecksumMismatch { expected: u32, actual: u32 },
    #[error("connection closed")]
    ConnectionClosed,
}

const LEGACY_OP_INSERT: i32 = 2002;
const LEGACY_OP_UPDATE: i32 = 2001;
const LEGACY_OP_DELETE: i32 = 2006;
const LEGACY_OP_REPLY: i32 = 1;
const LEGACY_OP_GET_MORE: i32 = 2005;
const LEGACY_OP_KILL_CURSORS: i32 = 2007;

fn classify_opcode(op: i32) -> Result<i32, WireError> {
    match op {
        OP_MSG => Ok(OP_MSG),
        OP_QUERY => Ok(OP_QUERY),
        OP_COMPRESSED => Ok(OP_COMPRESSED),
        LEGACY_OP_INSERT
        | LEGACY_OP_UPDATE
        | LEGACY_OP_DELETE
        | LEGACY_OP_REPLY
        | LEGACY_OP_GET_MORE
        | LEGACY_OP_KILL_CURSORS => Err(WireError::LegacyOpcode(op)),
        other => Err(WireError::UnsupportedOpcode(other)),
    }
}

pub async fn read_msg(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
) -> Result<OpMsg, WireError> {
    use tokio::io::AsyncReadExt;

    let message_length = match reader.read_i32_le().await {
        Ok(len) => len,
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(WireError::ConnectionClosed);
        }
        Err(e) => return Err(WireError::Io(e)),
    };

    if message_length < HEADER_LEN as i32 {
        return Err(WireError::MessageTooSmall(message_length));
    }
    if message_length > MAX_MESSAGE_SIZE {
        return Err(WireError::MessageTooLarge(message_length));
    }

    let request_id = reader.read_i32_le().await?;
    let response_to = reader.read_i32_le().await?;
    let op_code = reader.read_i32_le().await?;

    let op_code = classify_opcode(op_code)?;

    let header = MsgHeader {
        message_length,
        request_id,
        response_to,
        op_code,
    };

    if op_code == OP_QUERY {
        return read_op_query_as_msg(reader, header).await;
    }

    if op_code == OP_COMPRESSED {
        return Err(WireError::UnsupportedOpcode(OP_COMPRESSED));
    }

    let body_len = (message_length as usize) - HEADER_LEN;
    let mut body = vec![0u8; body_len];
    reader.read_exact(&mut body).await?;

    let msg = parse_op_msg_body(&body, header)?;

    if msg.flags.checksum_present {
        if let Some(received_crc) = msg.checksum {
            let mut full_msg = Vec::with_capacity(HEADER_LEN + body_len - 4);
            full_msg.extend_from_slice(&message_length.to_le_bytes());
            full_msg.extend_from_slice(&request_id.to_le_bytes());
            full_msg.extend_from_slice(&response_to.to_le_bytes());
            full_msg.extend_from_slice(&op_code.to_le_bytes());
            full_msg.extend_from_slice(&body[..body_len - 4]);
            let computed_crc = crc32c::crc32c(&full_msg);
            if computed_crc != received_crc {
                return Err(WireError::ChecksumMismatch {
                    expected: received_crc,
                    actual: computed_crc,
                });
            }
        }
    }

    Ok(msg)
}

fn parse_op_msg_body(body: &[u8], header: MsgHeader) -> Result<OpMsg, WireError> {
    use std::io::Read;

    if body.len() < 4 {
        return Err(WireError::MessageTooSmall(header.message_length));
    }

    let mut cursor = Cursor::new(body);

    let mut flag_bytes = [0u8; 4];
    cursor.read_exact(&mut flag_bytes)?;
    let flag_bits = u32::from_le_bytes(flag_bytes);
    let flags = OpMsgFlags::from_bits(flag_bits)?;

    let checksum_size = if flags.checksum_present { 4 } else { 0 };
    let sections_end = body.len() - checksum_size;

    let mut sections = Vec::new();
    while (cursor.position() as usize) < sections_end {
        let section = read_section(&mut cursor, sections_end)?;
        sections.push(section);
    }

    let checksum = if flags.checksum_present {
        let mut cksum_bytes = [0u8; 4];
        cursor.read_exact(&mut cksum_bytes)?;
        Some(u32::from_le_bytes(cksum_bytes))
    } else {
        None
    };

    Ok(OpMsg {
        header,
        flags,
        sections,
        checksum,
    })
}

fn read_section(cursor: &mut Cursor<&[u8]>, end: usize) -> Result<Section, WireError> {
    use std::io::Read;

    let mut kind_byte = [0u8; 1];
    cursor.read_exact(&mut kind_byte)?;
    let kind = kind_byte[0];

    match kind {
        0 => {
            let doc = read_bson_document(cursor)?;
            Ok(Section::Body(doc))
        }
        1 => {
            let mut size_bytes = [0u8; 4];
            cursor.read_exact(&mut size_bytes)?;
            let section_size = i32::from_le_bytes(size_bytes) as usize;
            if section_size < 4 {
                return Err(WireError::MalformedBson(
                    "document sequence size too small".into(),
                ));
            }
            let section_end = (cursor.position() as usize - 4) + section_size;
            if section_end > end {
                return Err(WireError::MalformedBson(
                    "document sequence extends past message boundary".into(),
                ));
            }

            let identifier = read_cstring(cursor)?;
            let mut documents = Vec::new();
            while (cursor.position() as usize) < section_end {
                let doc = read_bson_document(cursor)?;
                documents.push(doc);
            }
            Ok(Section::DocumentSequence {
                identifier,
                documents,
            })
        }
        other => Err(WireError::UnknownSectionKind(other)),
    }
}

fn read_bson_document(cursor: &mut Cursor<&[u8]>) -> Result<Vec<u8>, WireError> {
    let start = cursor.position() as usize;
    let buf = cursor.get_ref();

    if start + 4 > buf.len() {
        return Err(WireError::MalformedBson("truncated BSON document".into()));
    }

    let size =
        i32::from_le_bytes([buf[start], buf[start + 1], buf[start + 2], buf[start + 3]]) as usize;

    if size < 5 {
        return Err(WireError::MalformedBson(
            "BSON document size too small".into(),
        ));
    }
    if start + size > buf.len() {
        return Err(WireError::MalformedBson(
            "BSON document extends past buffer".into(),
        ));
    }

    let doc = buf[start..start + size].to_vec();
    cursor.set_position((start + size) as u64);
    Ok(doc)
}

fn read_cstring(cursor: &mut Cursor<&[u8]>) -> Result<String, WireError> {
    let buf = cursor.get_ref();
    let start = cursor.position() as usize;
    let remaining = &buf[start..];
    let nul_pos = remaining
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| WireError::MalformedBson("unterminated cstring".into()))?;
    let s = String::from_utf8(remaining[..nul_pos].to_vec())
        .map_err(|e| WireError::MalformedBson(format!("invalid UTF-8 in cstring: {e}")))?;
    cursor.set_position((start + nul_pos + 1) as u64);
    Ok(s)
}

async fn read_op_query_as_msg(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    header: MsgHeader,
) -> Result<OpMsg, WireError> {
    let body_len = (header.message_length as usize) - HEADER_LEN;
    let mut body = vec![0u8; body_len];
    tokio::io::AsyncReadExt::read_exact(&mut *reader, &mut body).await?;

    let mut cursor = Cursor::new(body.as_slice());
    // OP_QUERY: flags(4) + fullCollectionName(cstring) + numberToSkip(4) + numberToReturn(4) + query(doc)
    let mut _flags = [0u8; 4];
    cursor.read_exact(&mut _flags)?;
    let _collection = read_cstring(&mut cursor)?;
    let mut _skip = [0u8; 4];
    cursor.read_exact(&mut _skip)?;
    let mut _return = [0u8; 4];
    cursor.read_exact(&mut _return)?;
    let query_doc = read_bson_document(&mut cursor)?;

    Ok(OpMsg {
        header: MsgHeader {
            message_length: header.message_length,
            request_id: header.request_id,
            response_to: header.response_to,
            op_code: OP_MSG,
        },
        flags: OpMsgFlags {
            checksum_present: false,
            more_to_come: false,
            exhaust_allowed: false,
        },
        sections: vec![Section::Body(query_doc)],
        checksum: None,
    })
}

pub async fn write_msg(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    request_id: i32,
    response_to: i32,
    body_doc: &[u8],
) -> Result<(), WireError> {
    use tokio::io::AsyncWriteExt;
    // flag_bits(4) + section_kind(1) + body_doc
    let payload_len = 4 + 1 + body_doc.len();
    let message_length = (HEADER_LEN + payload_len) as i32;

    let mut buf = Vec::with_capacity(message_length as usize);
    buf.extend_from_slice(&message_length.to_le_bytes());
    buf.extend_from_slice(&request_id.to_le_bytes());
    buf.extend_from_slice(&response_to.to_le_bytes());
    buf.extend_from_slice(&OP_MSG.to_le_bytes());
    // flag_bits = 0 (no checksum, no moreToCome)
    buf.extend_from_slice(&0u32.to_le_bytes());
    // Section kind 0
    buf.push(0);
    buf.extend_from_slice(body_doc);

    writer.write_all(&buf).await?;
    writer.flush().await?;
    Ok(())
}

pub fn validate_op_msg(msg: &OpMsg) -> Result<&[u8], WireError> {
    let mut body = None;
    for section in &msg.sections {
        if let Section::Body(doc) = section {
            if body.is_some() {
                return Err(WireError::DuplicateBody);
            }
            body = Some(doc.as_slice());
        }
    }
    body.ok_or(WireError::MissingBody)
}

pub fn document_sequences(msg: &OpMsg) -> Vec<(&str, &[Vec<u8>])> {
    msg.sections
        .iter()
        .filter_map(|s| match s {
            Section::DocumentSequence {
                identifier,
                documents,
            } => Some((identifier.as_str(), documents.as_slice())),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    fn make_bson_doc(pairs: &[(&str, &str)]) -> Vec<u8> {
        let mut doc = Vec::new();
        let mut elements = Vec::new();
        for &(key, val) in pairs {
            // type 0x02 = UTF-8 string
            elements.push(0x02u8);
            elements.extend_from_slice(key.as_bytes());
            elements.push(0x00);
            let val_bytes = val.as_bytes();
            elements.extend_from_slice(&((val_bytes.len() as i32 + 1).to_le_bytes()));
            elements.extend_from_slice(val_bytes);
            elements.push(0x00);
        }
        let size = 4 + elements.len() + 1; // size + elements + terminator
        doc.extend_from_slice(&(size as i32).to_le_bytes());
        doc.extend_from_slice(&elements);
        doc.push(0x00);
        doc
    }

    fn make_simple_int32_doc(key: &str, val: i32) -> Vec<u8> {
        let mut elements = Vec::new();
        // type 0x10 = int32
        elements.push(0x10u8);
        elements.extend_from_slice(key.as_bytes());
        elements.push(0x00);
        elements.extend_from_slice(&val.to_le_bytes());
        let size = 4 + elements.len() + 1;
        let mut doc = Vec::new();
        doc.extend_from_slice(&(size as i32).to_le_bytes());
        doc.extend_from_slice(&elements);
        doc.push(0x00);
        doc
    }

    fn build_op_msg_bytes(flag_bits: u32, sections: &[u8]) -> Vec<u8> {
        let payload_len = 4 + sections.len();
        let message_length = (HEADER_LEN + payload_len) as i32;
        let mut buf = Vec::new();
        buf.extend_from_slice(&message_length.to_le_bytes());
        buf.extend_from_slice(&1i32.to_le_bytes()); // request_id
        buf.extend_from_slice(&0i32.to_le_bytes()); // response_to
        buf.extend_from_slice(&OP_MSG.to_le_bytes());
        buf.extend_from_slice(&flag_bits.to_le_bytes());
        buf.extend_from_slice(sections);
        buf
    }

    fn kind0_section(doc: &[u8]) -> Vec<u8> {
        let mut s = vec![0u8]; // kind 0
        s.extend_from_slice(doc);
        s
    }

    fn kind1_section(identifier: &str, docs: &[&[u8]]) -> Vec<u8> {
        let mut inner = Vec::new();
        inner.extend_from_slice(identifier.as_bytes());
        inner.push(0x00);
        for doc in docs {
            inner.extend_from_slice(doc);
        }
        let size = (4 + inner.len()) as i32;
        let mut s = vec![1u8]; // kind 1
        s.extend_from_slice(&size.to_le_bytes());
        s.extend_from_slice(&inner);
        s
    }

    #[tokio::test]
    async fn parse_simple_op_msg() {
        let body_doc = make_simple_int32_doc("ping", 1);
        let sections = kind0_section(&body_doc);
        let msg_bytes = build_op_msg_bytes(0, &sections);

        let mut reader = BufReader::new(msg_bytes.as_slice());
        let msg = read_msg(&mut reader).await.unwrap();

        assert_eq!(msg.header.op_code, OP_MSG);
        assert_eq!(msg.header.request_id, 1);
        assert!(!msg.flags.checksum_present);
        assert!(!msg.flags.more_to_come);

        let body = validate_op_msg(&msg).unwrap();
        assert_eq!(body, body_doc.as_slice());
    }

    #[tokio::test]
    async fn parse_op_msg_with_document_sequence() {
        let body_doc = make_bson_doc(&[("insert", "users")]);
        let doc1 = make_bson_doc(&[("name", "alice")]);
        let doc2 = make_bson_doc(&[("name", "bob")]);

        let mut sections = kind0_section(&body_doc);
        sections.extend_from_slice(&kind1_section("documents", &[&doc1, &doc2]));
        let msg_bytes = build_op_msg_bytes(0, &sections);

        let mut reader = BufReader::new(msg_bytes.as_slice());
        let msg = read_msg(&mut reader).await.unwrap();

        let seqs = document_sequences(&msg);
        assert_eq!(seqs.len(), 1);
        assert_eq!(seqs[0].0, "documents");
        assert_eq!(seqs[0].1.len(), 2);
    }

    #[tokio::test]
    async fn reject_legacy_opcode() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&20i32.to_le_bytes()); // message_length
        buf.extend_from_slice(&1i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&LEGACY_OP_INSERT.to_le_bytes());
        buf.extend_from_slice(&[0u8; 4]); // enough body to fill

        let mut reader = BufReader::new(buf.as_slice());
        let err = read_msg(&mut reader).await.unwrap_err();
        assert!(matches!(err, WireError::LegacyOpcode(LEGACY_OP_INSERT)));
    }

    #[tokio::test]
    async fn reject_unknown_opcode() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&20i32.to_le_bytes());
        buf.extend_from_slice(&1i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&9999i32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 4]);

        let mut reader = BufReader::new(buf.as_slice());
        let err = read_msg(&mut reader).await.unwrap_err();
        assert!(matches!(err, WireError::UnsupportedOpcode(9999)));
    }

    #[tokio::test]
    async fn reject_message_too_small() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&4i32.to_le_bytes()); // too small
        buf.extend_from_slice(&1i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&OP_MSG.to_le_bytes());

        let mut reader = BufReader::new(buf.as_slice());
        let err = read_msg(&mut reader).await.unwrap_err();
        assert!(matches!(err, WireError::MessageTooSmall(4)));
    }

    #[tokio::test]
    async fn reject_unsupported_required_flags() {
        let body_doc = make_simple_int32_doc("ping", 1);
        let sections = kind0_section(&body_doc);
        // Set bit 2 which is an unknown required flag
        let msg_bytes = build_op_msg_bytes(0x0004, &sections);

        let mut reader = BufReader::new(msg_bytes.as_slice());
        let err = read_msg(&mut reader).await.unwrap_err();
        assert!(matches!(err, WireError::UnsupportedFlags(_)));
    }

    #[tokio::test]
    async fn reject_missing_body() {
        let doc1 = make_bson_doc(&[("name", "alice")]);
        // Only a Kind 1 section, no Kind 0
        let sections = kind1_section("documents", &[&doc1]);
        let msg_bytes = build_op_msg_bytes(0, &sections);

        let mut reader = BufReader::new(msg_bytes.as_slice());
        let msg = read_msg(&mut reader).await.unwrap();
        let err = validate_op_msg(&msg).unwrap_err();
        assert!(matches!(err, WireError::MissingBody));
    }

    #[tokio::test]
    async fn reject_unknown_section_kind() {
        let body_doc = make_simple_int32_doc("ping", 1);
        let mut sections = kind0_section(&body_doc);
        // Append a section with unknown kind 5
        sections.push(5);
        sections.extend_from_slice(&body_doc);

        let msg_bytes = build_op_msg_bytes(0, &sections);

        let mut reader = BufReader::new(msg_bytes.as_slice());
        let err = read_msg(&mut reader).await.unwrap_err();
        assert!(matches!(err, WireError::UnknownSectionKind(5)));
    }

    #[tokio::test]
    async fn roundtrip_write_read() {
        let body_doc = make_simple_int32_doc("ok", 1);

        let mut buf = Vec::new();
        write_msg(&mut buf, 42, 1, &body_doc).await.unwrap();

        let mut reader = BufReader::new(buf.as_slice());
        let msg = read_msg(&mut reader).await.unwrap();

        assert_eq!(msg.header.request_id, 42);
        assert_eq!(msg.header.response_to, 1);
        let body = validate_op_msg(&msg).unwrap();
        assert_eq!(body, body_doc.as_slice());
    }

    #[tokio::test]
    async fn connection_closed_on_eof() {
        let buf: &[u8] = &[];
        let mut reader = BufReader::new(buf);
        let err = read_msg(&mut reader).await.unwrap_err();
        assert!(matches!(err, WireError::ConnectionClosed));
    }

    fn build_checksummed_msg(flag_bits: u32, sections: &[u8]) -> Vec<u8> {
        let payload_len = 4 + sections.len() + 4;
        let message_length = (HEADER_LEN + payload_len) as i32;
        let mut buf = Vec::new();
        buf.extend_from_slice(&message_length.to_le_bytes());
        buf.extend_from_slice(&1i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&OP_MSG.to_le_bytes());
        buf.extend_from_slice(&flag_bits.to_le_bytes());
        buf.extend_from_slice(sections);
        let crc = crc32c::crc32c(&buf);
        buf.extend_from_slice(&crc.to_le_bytes());
        buf
    }

    #[tokio::test]
    async fn parse_checksum_present() {
        let body_doc = make_simple_int32_doc("ping", 1);
        let sections = kind0_section(&body_doc);
        let buf = build_checksummed_msg(FLAG_CHECKSUM_PRESENT, &sections);

        let mut reader = BufReader::new(buf.as_slice());
        let msg = read_msg(&mut reader).await.unwrap();
        assert!(msg.flags.checksum_present);
        assert!(msg.checksum.is_some());
        validate_op_msg(&msg).unwrap();
    }

    #[tokio::test]
    async fn reject_invalid_checksum() {
        let body_doc = make_simple_int32_doc("ping", 1);
        let sections = kind0_section(&body_doc);
        let mut buf = build_checksummed_msg(FLAG_CHECKSUM_PRESENT, &sections);
        let len = buf.len();
        buf[len - 4..].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());

        let mut reader = BufReader::new(buf.as_slice());
        let err = read_msg(&mut reader).await.unwrap_err();
        assert!(matches!(err, WireError::ChecksumMismatch { .. }));
    }

    #[tokio::test]
    async fn reject_malformed_bson_truncated() {
        // Section kind 0 with a BSON doc that claims to be 100 bytes but only has 5
        let mut sections = vec![0u8]; // kind 0
        sections.extend_from_slice(&100i32.to_le_bytes()); // BSON size claims 100
        sections.push(0x00); // just terminator

        let msg_bytes = build_op_msg_bytes(0, &sections);
        let mut reader = BufReader::new(msg_bytes.as_slice());
        let err = read_msg(&mut reader).await.unwrap_err();
        assert!(matches!(err, WireError::MalformedBson(_)));
    }
}
