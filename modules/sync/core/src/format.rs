//! Erika's sync file, version 2: one binary file for an export, and the
//! same container for each object of a WebDAV repository.
//!
//! ```text
//! offset  size  what
//! 0       8     magic 89 45 52 4B 0D 0A 1A 0A  ("\x89ERK\r\n\x1a\n")
//! 8       2     format version, little endian (2)
//! 10      1     kind: 1 export, 2 snapshot, 3 patch, 4 manifest, 5 base
//! 11      1     flags: bit 0, the body is deflated
//! 12      4     body length as stored, little endian
//! 16      n     body
//! 16+n    32    SHA-256 of everything before it
//! ```
//!
//! The body is a run of sections, each a tag byte, a length (LEB128) and
//! its bytes: 1 head (a map: when, which device, which app, the
//! repository and snapshot), 2 key (how the passphrase's key is drawn),
//! 3 state (categories of keyed values), 4 operations (a patch's changes),
//! 5 index (a manifest's snapshot and patches). Unknown optional sections can
//! be skipped within this format. Only the current version is accepted: there
//! is no conversion of older backups or repositories.

use crate::seal::KeyParams;
use crate::value::{Malformed, Reader, Value, put_bytes, put_text, put_value, put_varint};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MAGIC: [u8; 8] = *b"\x89ERK\r\n\x1a\n";
pub const VERSION: u16 = 2;
/// What an export is called on disk: an Erika backup.
pub const EXTENSION: &str = "ebp";

/// Everything the sync carries: category, then key, then value.
pub type State = BTreeMap<String, BTreeMap<String, Value>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A file the user exports and imports.
    Export = 1,
    /// A repository's state at a version.
    Snapshot = 2,
    /// One device's changes on a snapshot.
    Patch = 3,
    /// A repository's index.
    Manifest = 4,
    /// What this device last agreed with a repository.
    Base = 5,
}

impl Kind {
    fn of(byte: u8) -> Option<Kind> {
        Some(match byte {
            1 => Kind::Export,
            2 => Kind::Snapshot,
            3 => Kind::Patch,
            4 => Kind::Manifest,
            5 => Kind::Base,
            _ => return None,
        })
    }
}

/// A change: `value` set at `category` / `key`, or the key removed.
#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    pub category: String,
    pub key: String,
    pub value: Option<Value>,
}

/// One file of the format, read or to write.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub kind: Kind,
    pub head: BTreeMap<String, Value>,
    pub key: Option<KeyParams>,
    pub state: Option<State>,
    pub ops: Option<Vec<Op>>,
    pub index: Option<Value>,
}

impl Document {
    pub fn new(kind: Kind) -> Document {
        Document {
            kind,
            head: BTreeMap::new(),
            key: None,
            state: None,
            ops: None,
            index: None,
        }
    }

    pub fn with_head(mut self, field: &str, value: Value) -> Document {
        self.head.insert(field.into(), value);
        self
    }

    pub fn head_str(&self, field: &str) -> String {
        self.head
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    pub fn head_int(&self, field: &str) -> Option<i64> {
        self.head.get(field).and_then(Value::as_i64)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FormatError {
    /// Not a file of this format.
    NotErika,
    /// Written by a later Erika, in a version this one cannot read.
    Newer(u16),
    /// Older schemas are not imported or upgraded.
    Incompatible(u16),
    /// Cut short or changed after it was written.
    Damaged(&'static str),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::NotErika => f.write_str("不是 Erika 的同步文件"),
            FormatError::Newer(version) => {
                write!(f, "文件来自更新版本的 Erika（格式 {version}），请先更新")
            }
            FormatError::Incompatible(version) => write!(f, "备份或同步库格式不兼容（格式 {version}），此开发版不提供旧数据迁移"),
            FormatError::Damaged(what) => write!(f, "文件已损坏：{what}"),
        }
    }
}

impl From<Malformed> for FormatError {
    fn from(error: Malformed) -> Self {
        FormatError::Damaged(error.0)
    }
}

const SECTION_HEAD: u8 = 1;
const SECTION_KEY: u8 = 2;
const SECTION_STATE: u8 = 3;
const SECTION_OPS: u8 = 4;
const SECTION_INDEX: u8 = 5;
const FLAG_DEFLATED: u8 = 1;
/// The most a body may inflate to.
const MAX_BODY: u64 = 1 << 30;

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn put_section(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    put_bytes(out, payload);
}

pub fn put_state(out: &mut Vec<u8>, state: &State) {
    put_varint(out, state.len() as u64);
    for (category, entries) in state {
        put_text(out, category);
        put_varint(out, entries.len() as u64);
        for (key, value) in entries {
            put_text(out, key);
            put_value(out, value);
        }
    }
}

fn read_state(reader: &mut Reader) -> Result<State, Malformed> {
    let mut state = State::new();
    for _ in 0..reader.count()? {
        let category = reader.text()?;
        let mut entries = BTreeMap::new();
        for _ in 0..reader.count()? {
            let key = reader.text()?;
            entries.insert(key, reader.value()?);
        }
        state.insert(category, entries);
    }
    Ok(state)
}

fn put_ops(out: &mut Vec<u8>, ops: &[Op]) {
    put_varint(out, ops.len() as u64);
    for op in ops {
        put_text(out, &op.category);
        put_text(out, &op.key);
        match &op.value {
            Some(value) => {
                out.push(0);
                put_value(out, value);
            }
            None => out.push(1),
        }
    }
}

fn read_ops(reader: &mut Reader) -> Result<Vec<Op>, Malformed> {
    let count = reader.count()?;
    let mut ops = Vec::with_capacity(count);
    for _ in 0..count {
        let category = reader.text()?;
        let key = reader.text()?;
        let value = match reader.byte()? {
            0 => Some(reader.value()?),
            1 => None,
            _ => return Err(Malformed("unknown operation")),
        };
        ops.push(Op {
            category,
            key,
            value,
        });
    }
    Ok(ops)
}

fn key_value(params: &KeyParams) -> Value {
    Value::map([
        ("kdf", Value::text("pbkdf2-sha256")),
        ("rounds", Value::Int(i64::from(params.rounds))),
        ("salt", Value::Bytes(params.salt.clone())),
        ("check", Value::Bytes(params.check.clone())),
    ])
}

fn key_params(value: &Value) -> Result<KeyParams, Malformed> {
    if value.get("kdf").and_then(Value::as_str) != Some("pbkdf2-sha256") {
        return Err(Malformed("unknown key derivation"));
    }
    let bytes = |field| {
        value
            .get(field)
            .and_then(Value::as_bytes)
            .map(<[u8]>::to_vec)
            .ok_or(Malformed("key parameters incomplete"))
    };
    Ok(KeyParams {
        salt: bytes("salt")?,
        check: bytes("check")?,
        rounds: value
            .int_of("rounds")
            .and_then(|rounds| u32::try_from(rounds).ok())
            .ok_or(Malformed("key parameters incomplete"))?,
    })
}

/// `document` as bytes.
pub fn write(document: &Document) -> Vec<u8> {
    let mut body = Vec::new();
    let mut section = Vec::new();
    put_value(&mut section, &Value::Map(document.head.clone()));
    put_section(&mut body, SECTION_HEAD, &section);
    if let Some(params) = &document.key {
        section.clear();
        put_value(&mut section, &key_value(params));
        put_section(&mut body, SECTION_KEY, &section);
    }
    if let Some(state) = &document.state {
        section.clear();
        put_state(&mut section, state);
        put_section(&mut body, SECTION_STATE, &section);
    }
    if let Some(ops) = &document.ops {
        section.clear();
        put_ops(&mut section, ops);
        put_section(&mut body, SECTION_OPS, &section);
    }
    if let Some(index) = &document.index {
        section.clear();
        put_value(&mut section, index);
        put_section(&mut body, SECTION_INDEX, &section);
    }
    let mut flags = 0;
    if body.len() > 512 {
        let deflated = deflate(&body);
        if deflated.len() < body.len() {
            body = deflated;
            flags |= FLAG_DEFLATED;
        }
    }
    let mut out = Vec::with_capacity(16 + body.len() + 32);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.push(document.kind as u8);
    out.push(flags);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    let digest = sha256(&out);
    out.extend_from_slice(&digest);
    out
}

/// The document `bytes` hold, checked against its digest.
pub fn read(bytes: &[u8]) -> Result<Document, FormatError> {
    if bytes.len() < 8 || bytes[..8] != MAGIC {
        return Err(FormatError::NotErika);
    }
    if bytes.len() < 16 + 32 {
        return Err(FormatError::Damaged("ends early"));
    }
    let version = u16::from_le_bytes([bytes[8], bytes[9]]);
    if version > VERSION {
        return Err(FormatError::Newer(version));
    }
    if version < VERSION {
        return Err(FormatError::Incompatible(version));
    }
    let kind = Kind::of(bytes[10]).ok_or(FormatError::Damaged("unknown kind"))?;
    let flags = bytes[11];
    let length = u32::from_le_bytes(bytes[12..16].try_into().expect("four bytes")) as usize;
    if bytes.len() != 16 + length + 32 {
        return Err(FormatError::Damaged("length does not match"));
    }
    let (content, digest) = bytes.split_at(16 + length);
    if sha256(content) != digest {
        return Err(FormatError::Damaged("checksum does not match"));
    }
    let stored = &content[16..];
    let inflated;
    let body = if flags & FLAG_DEFLATED != 0 {
        inflated = inflate(stored)?;
        &inflated[..]
    } else {
        stored
    };
    let mut document = Document::new(kind);
    let mut reader = Reader::new(body);
    while !reader.done() {
        let tag = reader.byte()?;
        let mut section = Reader::new(reader.bytes()?);
        match tag {
            SECTION_HEAD => match section.value()? {
                Value::Map(head) => document.head = head,
                _ => return Err(FormatError::Damaged("head is not a map")),
            },
            SECTION_KEY => document.key = Some(key_params(&section.value()?)?),
            SECTION_STATE => document.state = Some(read_state(&mut section)?),
            SECTION_OPS => document.ops = Some(read_ops(&mut section)?),
            SECTION_INDEX => document.index = Some(section.value()?),
            // A later version's section.
            _ => {}
        }
    }
    Ok(document)
}

fn deflate(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).expect("writing to memory");
    encoder.finish().expect("writing to memory")
}

fn inflate(bytes: &[u8]) -> Result<Vec<u8>, FormatError> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(bytes)
        .take(MAX_BODY + 1)
        .read_to_end(&mut out)
        .map_err(|_| FormatError::Damaged("body does not inflate"))?;
    if out.len() as u64 > MAX_BODY {
        return Err(FormatError::Damaged("body too large"));
    }
    Ok(out)
}
