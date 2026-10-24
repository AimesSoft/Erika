//! The values a sync file holds, and how they are written: a small
//! self-describing binary encoding of its own, so a file stays readable
//! when later versions add fields, and any JSON fits in it.
//!
//! Every value starts with a tag byte:
//!
//! | tag | value | then |
//! | --- | --- | --- |
//! | 0 | null | – |
//! | 1, 2 | false, true | – |
//! | 3 | integer | zigzag LEB128 |
//! | 4 | float | 8 bytes, little endian |
//! | 5 | text | length (LEB128), UTF-8 |
//! | 6 | bytes | length, bytes |
//! | 7 | list | count, values |
//! | 8 | map | count, (text key, value) sorted by key |
//! | 9 | sealed | length, bytes encrypted with the sync passphrase |

use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
    /// A secret, encrypted with the key of the sync passphrase.
    Sealed(Vec<u8>),
}

/// Floats compare by their bits, so a value read back equals the one
/// written and an unchanged record never shows as a change.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
            (Value::Text(a), Value::Text(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) | (Value::Sealed(a), Value::Sealed(b)) => a == b,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    pub fn text(text: impl Into<String>) -> Value {
        Value::Text(text.into())
    }

    /// A map of `fields`.
    pub fn map<const N: usize>(fields: [(&str, Value); N]) -> Value {
        Value::Map(
            fields
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    pub fn get(&self, field: &str) -> Option<&Value> {
        match self {
            Value::Map(map) => map.get(field),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(value) => Some(*value),
            Value::Float(value) if value.fract() == 0.0 => Some(*value as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(value) => Some(*value),
            Value::Int(value) => Some(*value as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(bytes) | Value::Sealed(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Text field `field`, or empty.
    pub fn str_of(&self, field: &str) -> String {
        self.get(field)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    pub fn int_of(&self, field: &str) -> Option<i64> {
        self.get(field).and_then(Value::as_i64)
    }

    pub fn float_of(&self, field: &str) -> f64 {
        self.get(field).and_then(Value::as_f64).unwrap_or(0.0)
    }

    /// Any JSON, as a value.
    pub fn from_json(json: &serde_json::Value) -> Value {
        match json {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(value) => Value::Bool(*value),
            serde_json::Value::Number(number) => match number.as_i64() {
                Some(value) => Value::Int(value),
                None => Value::Float(number.as_f64().unwrap_or(0.0)),
            },
            serde_json::Value::String(text) => Value::Text(text.clone()),
            serde_json::Value::Array(items) => {
                Value::List(items.iter().map(Value::from_json).collect())
            }
            serde_json::Value::Object(map) => Value::Map(
                map.iter()
                    .map(|(key, value)| (key.clone(), Value::from_json(value)))
                    .collect(),
            ),
        }
    }

    /// The value as JSON; bytes become arrays of numbers.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Value::Null => serde_json::Value::Null,
            Value::Bool(value) => (*value).into(),
            Value::Int(value) => (*value).into(),
            Value::Float(value) => serde_json::Number::from_f64(*value)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Value::Text(text) => text.clone().into(),
            Value::Bytes(bytes) | Value::Sealed(bytes) => bytes.clone().into(),
            Value::List(items) => items.iter().map(Value::to_json).collect(),
            Value::Map(map) => serde_json::Value::Object(
                map.iter()
                    .map(|(key, value)| (key.clone(), value.to_json()))
                    .collect(),
            ),
        }
    }
}

/// Why bytes could not be read.
#[derive(Clone, Debug, PartialEq)]
pub struct Malformed(pub &'static str);

/// The deepest lists and maps nest; deeper is taken as damage.
const DEPTH: usize = 64;

pub fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

pub fn put_text(out: &mut Vec<u8>, text: &str) {
    put_bytes(out, text.as_bytes());
}

pub fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    put_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

pub fn put_value(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => out.push(0),
        Value::Bool(false) => out.push(1),
        Value::Bool(true) => out.push(2),
        Value::Int(value) => {
            out.push(3);
            put_varint(out, ((value << 1) ^ (value >> 63)) as u64);
        }
        Value::Float(value) => {
            out.push(4);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Value::Text(text) => {
            out.push(5);
            put_text(out, text);
        }
        Value::Bytes(bytes) => {
            out.push(6);
            put_bytes(out, bytes);
        }
        Value::List(items) => {
            out.push(7);
            put_varint(out, items.len() as u64);
            for item in items {
                put_value(out, item);
            }
        }
        Value::Map(map) => {
            out.push(8);
            put_varint(out, map.len() as u64);
            for (key, value) in map {
                put_text(out, key);
                put_value(out, value);
            }
        }
        Value::Sealed(bytes) => {
            out.push(9);
            put_bytes(out, bytes);
        }
    }
}

/// Reads what [`put_value`] and friends wrote, never past its end.
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, at: 0 }
    }

    pub fn done(&self) -> bool {
        self.at >= self.bytes.len()
    }

    pub fn byte(&mut self) -> Result<u8, Malformed> {
        let byte = *self.bytes.get(self.at).ok_or(Malformed("ends early"))?;
        self.at += 1;
        Ok(byte)
    }

    pub fn take(&mut self, len: usize) -> Result<&'a [u8], Malformed> {
        let end = self
            .at
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(Malformed("ends early"))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    pub fn varint(&mut self) -> Result<u64, Malformed> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(Malformed("number too long"))
    }

    /// A count of things to read, each at least a byte long.
    pub fn count(&mut self) -> Result<usize, Malformed> {
        let count = self.varint()?;
        if count > (self.bytes.len() - self.at) as u64 {
            return Err(Malformed("count past the end"));
        }
        Ok(count as usize)
    }

    pub fn bytes(&mut self) -> Result<&'a [u8], Malformed> {
        let len = self.count()?;
        self.take(len)
    }

    pub fn text(&mut self) -> Result<String, Malformed> {
        String::from_utf8(self.bytes()?.to_vec()).map_err(|_| Malformed("text is not UTF-8"))
    }

    pub fn value(&mut self) -> Result<Value, Malformed> {
        self.value_at(0)
    }

    fn value_at(&mut self, depth: usize) -> Result<Value, Malformed> {
        if depth > DEPTH {
            return Err(Malformed("nested too deep"));
        }
        Ok(match self.byte()? {
            0 => Value::Null,
            1 => Value::Bool(false),
            2 => Value::Bool(true),
            3 => {
                let raw = self.varint()?;
                Value::Int((raw >> 1) as i64 ^ -((raw & 1) as i64))
            }
            4 => Value::Float(f64::from_le_bytes(
                self.take(8)?.try_into().expect("eight bytes"),
            )),
            5 => Value::Text(self.text()?),
            6 => Value::Bytes(self.bytes()?.to_vec()),
            7 => {
                let count = self.count()?;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(self.value_at(depth + 1)?);
                }
                Value::List(items)
            }
            8 => {
                let count = self.count()?;
                let mut map = BTreeMap::new();
                for _ in 0..count {
                    let key = self.text()?;
                    map.insert(key, self.value_at(depth + 1)?);
                }
                Value::Map(map)
            }
            9 => Value::Sealed(self.bytes()?.to_vec()),
            _ => return Err(Malformed("unknown value tag")),
        })
    }
}

/// `value` written alone.
pub fn encode(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    put_value(&mut out, value);
    out
}

/// A value [`encode`] wrote.
pub fn decode(bytes: &[u8]) -> Result<Value, Malformed> {
    let mut reader = Reader::new(bytes);
    let value = reader.value()?;
    if !reader.done() {
        return Err(Malformed("bytes after the value"));
    }
    Ok(value)
}
