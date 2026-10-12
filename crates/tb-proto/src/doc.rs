use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    /// `{"OR": <record>, ...}`: answers a request.
    Reply,
    /// `{"UP": <record>, ...}`: pushed for a subscription, carrying only the changed keys.
    Update,
}

/// A record document: `{"OR"|"UP": <record>, "KVP": {<hex key>: <value>, ...}}`.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub kind: DocKind,
    pub record: String,
    pub kvp: Map<String, Value>,
}

impl Document {
    pub fn from_json(v: &Value) -> Option<Self> {
        let obj = v.as_object()?;
        let (kind, record) = if let Some(r) = obj.get("OR") {
            (DocKind::Reply, r)
        } else {
            (DocKind::Update, obj.get("UP")?)
        };
        Some(Self {
            kind,
            record: record.as_str()?.to_string(),
            kvp: obj.get("KVP")?.as_object()?.clone(),
        })
    }
}
