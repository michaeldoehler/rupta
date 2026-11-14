//! The envelope format: a JSON header line, then items, each a JSON
//! header line and a payload. A payload is either `length` bytes (from the
//! item header) followed by an optional newline, or everything up to the
//! next newline (or the end).
//!
//! ```text
//! {"event_id":"9ec79c33ec9942ab8353589fcb2e04dc","dsn":"https://key@host/1"}
//! {"type":"attachment","length":10,"filename":"hello.txt"}
//! \xef\xbb\xbfHello\r\n
//! {"type":"event"}
//! {"message":"hello"}
//! ```

use bytes::Bytes;
use serde_json::{Map, Value};

/// Most items accepted in one envelope.
pub const MAX_ITEMS: usize = 1000;

#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    pub headers: Map<String, Value>,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub headers: Map<String, Value>,
    /// The `type` header (`event`, `attachment`, ...); empty if missing.
    pub ty: String,
    pub payload: Bytes,
}

impl Item {
    pub fn header_str(&self, key: &str) -> Option<&str> {
        self.headers.get(key).and_then(Value::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EnvelopeError {
    #[error("empty envelope")]
    Empty,
    #[error("invalid envelope header: {0}")]
    InvalidHeader(String),
    #[error("invalid item header at byte {0}: {1}")]
    InvalidItemHeader(usize, String),
    #[error("item at byte {0} claims {1} bytes but only {2} remain")]
    Truncated(usize, usize, usize),
    #[error("too many items (more than {MAX_ITEMS})")]
    TooManyItems,
}

impl Envelope {
    pub fn event_id(&self) -> Option<&str> {
        self.headers.get("event_id").and_then(Value::as_str)
    }

    pub fn dsn(&self) -> Option<&str> {
        self.headers.get("dsn").and_then(Value::as_str)
    }
}

fn line_end(data: &[u8], from: usize) -> usize {
    data[from..].iter().position(|&b| b == b'\n').map_or(data.len(), |p| from + p)
}

fn object(bytes: &[u8]) -> Result<Map<String, Value>, String> {
    let trimmed = trim_ascii(bytes);
    if trimmed.is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_slice::<Value>(trimmed) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err("not a JSON object".into()),
        Err(e) => Err(e.to_string()),
    }
}

fn trim_ascii(b: &[u8]) -> &[u8] {
    let start = b.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(b.len());
    let end = b.iter().rposition(|c| !c.is_ascii_whitespace()).map_or(start, |e| e + 1);
    &b[start..end]
}

/// Parses an envelope. Never panics; malformed input is an error.
pub fn parse(data: Bytes) -> Result<Envelope, EnvelopeError> {
    let bytes = &data[..];
    if trim_ascii(bytes).is_empty() {
        return Err(EnvelopeError::Empty);
    }
    let end = line_end(bytes, 0);
    let headers = object(&bytes[..end]).map_err(EnvelopeError::InvalidHeader)?;
    let mut pos = (end + 1).min(bytes.len());
    let mut items = Vec::new();
    while pos < bytes.len() {
        let hend = line_end(bytes, pos);
        let line = &bytes[pos..hend];
        if trim_ascii(line).is_empty() {
            // Blank lines between items (and a trailing newline) are allowed.
            pos = hend + 1;
            continue;
        }
        let item_headers = object(line).map_err(|e| EnvelopeError::InvalidItemHeader(pos, e))?;
        let header_pos = pos;
        pos = (hend + 1).min(bytes.len());
        let length = match item_headers.get("length") {
            None | Some(Value::Null) => None,
            Some(v) => match v.as_u64() {
                Some(n) => Some(n),
                None => return Err(EnvelopeError::InvalidItemHeader(header_pos, "invalid length".into())),
            },
        };
        let payload = match length {
            Some(n) => {
                let remaining = bytes.len() - pos;
                if n > remaining as u64 {
                    return Err(EnvelopeError::Truncated(header_pos, n.min(usize::MAX as u64) as usize, remaining));
                }
                let n = n as usize;
                let p = data.slice(pos..pos + n);
                pos += n;
                if pos < bytes.len() && bytes[pos] == b'\n' {
                    pos += 1;
                }
                p
            }
            None => {
                let pend = line_end(bytes, pos);
                let mut slice_end = pend;
                if slice_end > pos && bytes[slice_end - 1] == b'\r' {
                    slice_end -= 1;
                }
                let p = data.slice(pos..slice_end);
                pos = pend + 1;
                p
            }
        };
        let ty = item_headers.get("type").and_then(Value::as_str).unwrap_or("").to_string();
        items.push(Item { headers: item_headers, ty, payload });
        if items.len() > MAX_ITEMS {
            return Err(EnvelopeError::TooManyItems);
        }
    }
    Ok(Envelope { headers, items })
}

/// Serializes an envelope (used by tests and the benchmark).
pub fn serialize(headers: &Value, items: &[(Value, Vec<u8>)]) -> Vec<u8> {
    let mut out = serde_json::to_vec(headers).unwrap_or_default();
    out.push(b'\n');
    for (h, payload) in items {
        let mut h = h.clone();
        if let Value::Object(m) = &mut h {
            m.insert("length".into(), Value::from(payload.len()));
        }
        out.extend(serde_json::to_vec(&h).unwrap_or_default());
        out.push(b'\n');
        out.extend_from_slice(payload);
        out.push(b'\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_example_with_lengths_and_implicit_lengths() {
        let raw = b"{\"event_id\":\"9ec79c33ec9942ab8353589fcb2e04dc\",\"dsn\":\"https://e12d836b15bb49d7bbf99e64295d995b:@sentry.io/42\"}\n{\"type\":\"attachment\",\"length\":10,\"content_type\":\"text/plain\",\"filename\":\"hello.txt\"}\n\xef\xbb\xbfHello\r\n\n{\"type\":\"event\",\"length\":41,\"content_type\":\"application/json\",\"filename\":\"application.log\"}\n{\"message\":\"hello world\",\"level\":\"error\"}\n";
        let e = parse(Bytes::from_static(raw)).unwrap();
        assert_eq!(e.event_id(), Some("9ec79c33ec9942ab8353589fcb2e04dc"));
        assert_eq!(e.items.len(), 2);
        assert_eq!(e.items[0].ty, "attachment");
        assert_eq!(&e.items[0].payload[..], b"\xef\xbb\xbfHello\r\n");
        assert_eq!(&e.items[1].payload[..], b"{\"message\":\"hello world\",\"level\":\"error\"}");

        let raw =
            b"{}\n{\"type\":\"session\"}\n{\"started\": \"2020-02-07T14:16:00Z\"}\n{\"type\":\"client_report\"}\r\n{}";
        let e = parse(Bytes::from_static(raw)).unwrap();
        assert_eq!(e.items.len(), 2);
        assert_eq!(&e.items[0].payload[..], b"{\"started\": \"2020-02-07T14:16:00Z\"}");
        assert_eq!(&e.items[1].payload[..], b"{}");
    }

    #[test]
    fn empty_payloads_and_missing_trailing_newline() {
        let e = parse(Bytes::from_static(
            b"{}\n{\"type\":\"attachment\",\"length\":0}\n\n{\"type\":\"event\",\"length\":2}\n{}",
        ))
        .unwrap();
        assert_eq!(e.items.len(), 2);
        assert!(e.items[0].payload.is_empty());
        assert_eq!(&e.items[1].payload[..], b"{}");
        let e = parse(Bytes::from_static(b"{\"event_id\":\"x\"}")).unwrap();
        assert!(e.items.is_empty());
    }

    #[test]
    fn malformed_envelopes_are_errors() {
        assert_eq!(parse(Bytes::new()), Err(EnvelopeError::Empty));
        assert!(matches!(parse(Bytes::from_static(b"[1]\n")), Err(EnvelopeError::InvalidHeader(_))));
        assert!(matches!(parse(Bytes::from_static(b"{}\nnope\n")), Err(EnvelopeError::InvalidItemHeader(..))));
        assert!(matches!(parse(Bytes::from_static(b"{}\n{\"length\":99}\nabc")), Err(EnvelopeError::Truncated(..))));
        assert!(matches!(
            parse(Bytes::from_static(b"{}\n{\"length\":-1}\nabc")),
            Err(EnvelopeError::InvalidItemHeader(..))
        ));
    }

    #[test]
    fn random_bytes_never_panic() {
        // xorshift: deterministic pseudo-random input.
        let mut state: u64 = 0x9E3779B97F4A7C15;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let base = serialize(
            &serde_json::json!({"event_id": "0123456789abcdef0123456789abcdef"}),
            &[
                (serde_json::json!({"type": "event"}), b"{\"message\":\"x\"}".to_vec()),
                (serde_json::json!({"type": "attachment"}), vec![0, 1, 2, 10, 13]),
            ],
        );
        for i in 0..20_000 {
            let len = (next() % 300) as usize;
            let data: Vec<u8> = if i % 2 == 0 {
                // Mutations of a valid envelope.
                let mut d = base.clone();
                for _ in 0..(next() % 8) {
                    let at = (next() as usize) % d.len();
                    d[at] = (next() & 0xff) as u8;
                }
                d.truncate((next() as usize) % (d.len() + 1));
                d
            } else {
                // Random bytes drawn mostly from envelope-ish characters.
                let alphabet = b"{}[]\":,\n\r 0123456789lengthtype-";
                (0..len)
                    .map(|_| {
                        let r = next();
                        if r % 5 == 0 { (r >> 8) as u8 } else { alphabet[(r as usize >> 3) % alphabet.len()] }
                    })
                    .collect()
            };
            let _ = parse(Bytes::from(data));
        }
    }
}
