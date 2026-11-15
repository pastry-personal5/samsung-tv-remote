//! Stateless MCP 2026 framing. HTTP access checks happen before this decoder.
use base64::{engine::general_purpose::STANDARD, Engine};
use hyper::http::HeaderMap;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{json, Map, Value};
use std::fmt;

pub const VERSION: &str = "2026-07-28";
pub const MAX_META_BYTES: usize = 4096;

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
}
impl Reply {
    pub fn error(status: u16, id: Option<&Value>, code: i32, message: &'static str) -> Self {
        let mut body = json!({"jsonrpc":"2.0","error":{"code":code,"message":message}});
        if let Some(id) = id {
            body["id"] = id.clone();
        }
        Self { status, body }
    }
    pub fn result(id: &Value, mut result: Value) -> Self {
        result["resultType"] = json!("complete");
        Self {
            status: 200,
            body: json!({"jsonrpc":"2.0","id":id,"result":result}),
        }
    }
}

pub struct Rpc {
    pub id: Value,
    pub method: String,
    pub params: Map<String, Value>,
}
impl Rpc {
    pub fn tool_name(&self) -> Option<&str> {
        self.params.get("name").and_then(Value::as_str)
    }
    pub fn is_read(&self) -> bool {
        matches!(self.method.as_str(), "server/discover" | "tools/list")
            || (self.method == "tools/call"
                && self.tool_name().is_some_and(super::catalog::is_read))
    }
}

pub(crate) fn unique_json(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    crate::infrastructure::unique_json::unique_json(bytes)
}

fn single<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, ()> {
    let mut values = headers.get_all(name).iter();
    let first = values.next();
    if values.next().is_some() {
        return Err(());
    }
    first.map(|v| v.to_str().map_err(|_| ())).transpose()
}
fn decoded_name(value: &str) -> Result<String, ()> {
    if let Some(encoded) = value
        .strip_prefix("=?base64?")
        .and_then(|v| v.strip_suffix("?="))
    {
        String::from_utf8(STANDARD.decode(encoded).map_err(|_| ())?).map_err(|_| ())
    } else if !value.is_empty()
        && value.trim() == value
        && value.bytes().all(|v| (0x20..=0x7e).contains(&v))
    {
        Ok(value.into())
    } else {
        Err(())
    }
}

// Recover only an unambiguous root ID for rejected unique-member documents.
// IgnoredAny consumes all other values without interpreting duplicate members
// as authority. A second root ID always makes correlation ambiguous.
struct RejectedId(Option<Value>);
impl<'de> Deserialize<'de> for RejectedId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct IdVisitor;
        impl<'de> Visitor<'de> for IdVisitor {
            type Value = RejectedId;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("request object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RejectedId, A::Error> {
                let mut id = None;
                let mut seen = false;
                let mut ambiguous = false;
                while let Some(key) = map.next_key::<String>()? {
                    if key == "id" {
                        let value: Value = map.next_value()?;
                        ambiguous |= seen;
                        seen = true;
                        if valid_id(&value) {
                            id = Some(value);
                        }
                    } else {
                        map.next_value::<de::IgnoredAny>()?;
                    }
                }
                Ok(RejectedId(if ambiguous { None } else { id }))
            }
        }
        deserializer.deserialize_map(IdVisitor)
    }
}
fn valid_id(id: &Value) -> bool {
    id.is_string() || id.as_i64().is_some() || id.as_u64().is_some()
}

pub fn parse(headers: &HeaderMap, bytes: &[u8]) -> Result<Rpc, Reply> {
    let value = unique_json(bytes).map_err(|e| {
        let code = if e.is_data() { -32600 } else { -32700 };
        let id = e
            .is_data()
            .then(|| serde_json::from_slice::<RejectedId>(bytes).ok())
            .flatten()
            .and_then(|id| id.0);
        Reply::error(400, id.as_ref(), code, "Invalid JSON-RPC request")
    })?;
    let Some(envelope) = value.as_object() else {
        return Err(Reply::error(400, None, -32600, "Expected one request"));
    };
    let id = envelope.get("id").filter(|id| valid_id(id));
    let invalid = || Reply::error(400, id, -32600, "Invalid JSON-RPC request");
    if envelope.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || envelope
            .keys()
            .any(|k| !["jsonrpc", "id", "method", "params"].contains(&k.as_str()))
    {
        return Err(invalid());
    }
    let id = id.ok_or_else(invalid)?.clone();
    let method = envelope
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?
        .to_owned();
    let params = envelope
        .get("params")
        .and_then(Value::as_object)
        .ok_or_else(|| Reply::error(400, Some(&id), -32602, "Missing request parameters"))?;
    let mismatch = || Reply::error(400, Some(&id), -32020, "MCP header mismatch");
    let meta = params
        .get("_meta")
        .and_then(Value::as_object)
        .ok_or_else(|| Reply::error(400, Some(&id), -32602, "Missing request metadata"))?;
    if serde_json::to_vec(meta).map_or(true, |v| v.len() > MAX_META_BYTES) {
        return Err(Reply::error(
            400,
            Some(&id),
            -32602,
            "Request metadata too large",
        ));
    }
    let version = meta
        .get("io.modelcontextprotocol/protocolVersion")
        .and_then(Value::as_str)
        .ok_or_else(|| Reply::error(400, Some(&id), -32602, "Missing protocol version"))?;
    if single(headers, "mcp-protocol-version").map_err(|_| mismatch())? != Some(version)
        || single(headers, "mcp-method").map_err(|_| mismatch())? != Some(method.as_str())
    {
        return Err(mismatch());
    }
    if method == "tools/call" {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| Reply::error(400, Some(&id), -32602, "Missing tool name"))?;
        let header = single(headers, "mcp-name")
            .map_err(|_| mismatch())?
            .ok_or_else(mismatch)?;
        if decoded_name(header).map_err(|_| mismatch())? != name {
            return Err(mismatch());
        }
    } else if single(headers, "mcp-name")
        .map_err(|_| mismatch())?
        .is_some()
    {
        return Err(mismatch());
    }
    if version != VERSION {
        let mut reply = Reply::error(400, Some(&id), -32022, "Unsupported protocol version");
        reply.body["error"]["data"] = json!({"supported":[VERSION],"requested":version});
        return Err(reply);
    }
    if !meta
        .get("io.modelcontextprotocol/clientCapabilities")
        .is_some_and(Value::is_object)
    {
        return Err(Reply::error(
            400,
            Some(&id),
            -32602,
            "Missing client capabilities",
        ));
    }
    if let Some(info) = meta.get("io.modelcontextprotocol/clientInfo") {
        if !info.is_object()
            || !info.get("name").is_some_and(Value::is_string)
            || !info.get("version").is_some_and(Value::is_string)
        {
            return Err(Reply::error(400, Some(&id), -32602, "Invalid client info"));
        }
    }
    if !matches!(
        method.as_str(),
        "server/discover" | "tools/list" | "tools/call"
    ) {
        return Err(Reply::error(404, Some(&id), -32601, "Unknown method"));
    }
    let allowed: &[&str] = match method.as_str() {
        "tools/call" => &["_meta", "name", "arguments"],
        "tools/list" => &["_meta", "cursor"],
        _ => &["_meta"],
    };
    if params.keys().any(|key| !allowed.contains(&key.as_str()))
        || (method == "tools/list" && params.get("cursor").is_some_and(|v| !v.is_null()))
        || (method == "tools/call" && params.get("arguments").is_some_and(|v| !v.is_object()))
    {
        return Err(Reply::error(
            400,
            Some(&id),
            -32602,
            "Invalid method parameters",
        ));
    }
    if method == "tools/call"
        && !params
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(super::catalog::contains)
    {
        return Err(Reply::error(400, Some(&id), -32602, "Unknown tool"));
    }
    Ok(Rpc {
        id,
        method,
        params: params.clone(),
    })
}

pub fn accepts_json_and_events(headers: &HeaderMap) -> bool {
    let mut json = false;
    let mut events = false;
    for value in headers.get_all("accept") {
        let Ok(value) = value.to_str() else {
            return false;
        };
        for item in value.split(',') {
            let mut parts = item.trim().split(';');
            let media = parts.next().unwrap_or("").trim();
            let mut enabled = true;
            for parameter in parts {
                if let Some(q) = parameter.trim().strip_prefix("q=") {
                    enabled &= q
                        .parse::<f64>()
                        .is_ok_and(|q| q.is_finite() && q > 0.0 && q <= 1.0);
                }
            }
            if enabled {
                json |= media.eq_ignore_ascii_case("application/json");
                events |= media.eq_ignore_ascii_case("text/event-stream");
            }
        }
    }
    json && events
}
