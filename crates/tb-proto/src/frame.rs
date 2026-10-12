use serde_json::Value;

/// RACE command carrying the key/value protocol.
pub const CMD_KVP: u16 = 0x9902;

const HEADER_LEN: usize = 12;
const OP_GET: u8 = 0x01;
const OP_SET: u8 = 0x03;
const OP_SET_ACK: u8 = 0x43;

/// Bytes 2–4 of the header: chosen by the host, echoed in the reply. The device accepts any values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub session: u8,
    pub counter: u8,
    pub seq: u8,
}

impl Token {
    pub fn bytes(self) -> [u8; 3] {
        [self.session, self.counter, self.seq]
    }
}

/// A host request, as Swarm II sends them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `b5 "index"`: list the records.
    Index,
    /// `61 00 'S' <record>`: answer the record now and push its changes later.
    Subscribe(String),
    /// `61 00 'R' <record>`: answer the record once.
    Read(String),
    /// `b7 "set_kvp" ff <json>`: write keys, e.g. `{"0x1210":"2"}`.
    SetKvp(String),
}

impl Request {
    /// The RACE payload of command `0x9902` for this request.
    pub fn encode(&self, token: Token) -> Vec<u8> {
        let op = if matches!(self, Self::SetKvp(_)) { OP_SET } else { OP_GET };
        let mut out = vec![0x48, op];
        out.extend_from_slice(&token.bytes());
        out.resize(HEADER_LEN, 0);
        match self {
            Self::Index => out.extend_from_slice(b"\xb5index"),
            Self::Subscribe(record) => {
                out.extend_from_slice(b"\x61\x00S");
                out.extend_from_slice(record.as_bytes());
            }
            Self::Read(record) => {
                out.extend_from_slice(b"\x61\x00R");
                out.extend_from_slice(record.as_bytes());
            }
            Self::SetKvp(json) => {
                out.extend_from_slice(b"\xb7set_kvp\xff");
                out.extend_from_slice(json.as_bytes());
            }
        }
        out
    }

    /// Short description for error messages.
    pub fn describe(&self) -> String {
        match self {
            Self::Index => "index".into(),
            Self::Subscribe(r) => format!("subscribe {r}"),
            Self::Read(r) => format!("read {r}"),
            Self::SetKvp(j) => format!("set {j}"),
        }
    }
}

/// Byte 0 of the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    /// `48`: host request.
    Request,
    /// `68`: device reply to a request (token echoed).
    Reply,
    /// `58`: device push for a subscription.
    Push,
    Other(u8),
}

/// A decoded `0x9902` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub kind: MessageKind,
    pub op: u8,
    pub token: [u8; 3],
    pub body: Vec<u8>,
}

impl Message {
    pub fn decode(payload: &[u8]) -> Option<Self> {
        if payload.len() < 5 {
            return None;
        }
        let kind = match payload[0] {
            0x48 => MessageKind::Request,
            0x68 => MessageKind::Reply,
            0x58 => MessageKind::Push,
            b => MessageKind::Other(b),
        };
        Some(Self {
            kind,
            op: payload[1],
            token: [payload[2], payload[3], payload[4]],
            body: payload.get(HEADER_LEN..).unwrap_or_default().to_vec(),
        })
    }

    pub fn is_set_ack(&self) -> bool {
        self.kind == MessageKind::Reply && self.op == OP_SET_ACK
    }

    /// The JSON document or array the body carries.
    ///
    /// Bodies seen: `61 <n> 62 32 <y> 21 01 ff <json>` for documents and `c1 32 21 01 ff <json>` for the index.
    pub fn json(&self) -> Option<Value> {
        let b = &self.body;
        let start = match b.first() {
            Some(0x61) if b.get(7) == Some(&0xFF) => 8,
            Some(0xC1) if b.get(4) == Some(&0xFF) => 5,
            _ => b.iter().position(|&x| x == 0xFF)? + 1,
        };
        serde_json::from_slice(b.get(start..)?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use race_hid::{RacePacket, RaceType};

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    fn race(req: &Request, token: Token) -> Vec<u8> {
        RacePacket::new(RaceType::Command, CMD_KVP, req.encode(token)).encode()
    }

    #[test]
    fn subscribe_matches_swarm_ii() {
        // HaloBattery's capture of Swarm II, packet 150: RACE header, 9902 header, "a\0SInf"
        let t = Token { session: 0xC0, counter: 0x4A, seq: 0x01 };
        let want = hex(concat!("055a14000299", "4801c04a01", "00000000000000", "610053496e66"));
        assert_eq!(race(&Request::Subscribe("Inf".into()), t), want);
    }

    #[test]
    fn index_matches_swarm_ii() {
        // our capture: Swarm II's index request
        let t = Token { session: 0xAA, counter: 0x8A, seq: 0x01 };
        let want = hex(concat!("055a14000299", "4801aa8a01", "00000000000000", "b5696e646578"));
        assert_eq!(race(&Request::Index, t), want);
    }

    #[test]
    fn set_kvp_matches_swarm_ii() {
        // our capture: Swarm II selecting the Bass Boost preset
        let t = Token { session: 0xAA, counter: 0xA5, seq: 0x1C };
        let req = Request::SetKvp(r#"{"0x1210":"2"}"#.into());
        assert_eq!(race(&req, t), hex("055a250002994803aaa51c00000000000000b77365745f6b7670ff7b22307831323130223a2232227d"));
    }

    #[test]
    fn decodes_set_ack() {
        let m = Message::decode(&hex("6843aaa51c00000000000000")).unwrap();
        assert!(m.is_set_ack());
        assert_eq!(m.token, [0xAA, 0xA5, 0x1C]);
        assert_eq!(m.json(), None);
    }

    #[test]
    fn decodes_push_document() {
        let payload = hex("58450064050000000000000061296232ef2101ff7b225550223a224254222c224b5650223a7b22333230223a222d3533227d7d");
        let m = Message::decode(&payload).unwrap();
        assert_eq!(m.kind, MessageKind::Push);
        assert_eq!(m.json().unwrap(), serde_json::json!({"UP": "BT", "KVP": {"320": "-53"}}));
    }
}
