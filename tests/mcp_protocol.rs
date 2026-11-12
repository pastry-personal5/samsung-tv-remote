//! Revision-specific framing fixtures independent of TV/service policy.
use base64::{engine::general_purpose::STANDARD, Engine};
use hyper::http::HeaderMap;
use samsung_tv_remote::server::mcp::{
    catalog,
    protocol::{self, Reply},
};
use serde_json::{json, Value};

fn request(method: &str) -> (HeaderMap, Value) {
    let mut headers = HeaderMap::new();
    headers.insert("mcp-protocol-version", protocol::VERSION.parse().unwrap());
    headers.insert("mcp-method", method.parse().unwrap());
    headers.insert(
        "accept",
        "application/json, text/event-stream".parse().unwrap(),
    );
    let mut body = json!({"jsonrpc":"2.0","id":"fixture","method":method,"params":{"_meta":{
        "io.modelcontextprotocol/protocolVersion":protocol::VERSION,
        "io.modelcontextprotocol/clientCapabilities":{}}}});
    if method == "tools/call" {
        headers.insert("mcp-name", "list_tvs".parse().unwrap());
        body["params"]["name"] = json!("list_tvs");
        body["params"]["arguments"] = json!({});
    }
    (headers, body)
}
fn parse(headers: &HeaderMap, body: &Value) -> Result<protocol::Rpc, Reply> {
    protocol::parse(headers, &serde_json::to_vec(body).unwrap())
}
fn error(headers: &HeaderMap, body: &Value, status: u16, code: i32) {
    let reply = parse(headers, body).err().expect("rejected request");
    assert_eq!(reply.status, status);
    assert_eq!(reply.body["error"]["code"], code);
}

#[test]
fn direct_first_call_has_no_connection_or_discovery_prerequisite() {
    let (mut headers, mut body) = request("tools/call");
    body["params"]["_meta"]["org.example/extension"] = json!({"unknown":true});
    let rpc = parse(&headers, &body).unwrap();
    assert!(rpc.is_read());
    assert_eq!(rpc.tool_name(), Some("list_tvs"));
    let encoded = format!("=?base64?{}?=", STANDARD.encode("list_tvs"));
    headers.insert("mcp-name", encoded.parse().unwrap());
    assert!(parse(&headers, &body).is_ok());
    // Removed session headers are ignored; they never establish identity/state.
    headers.insert("mcp-session-id", "obsolete".parse().unwrap());
    headers.insert("last-event-id", "obsolete".parse().unwrap());
    assert!(parse(&headers, &body).is_ok());
}

#[test]
fn mirrored_headers_missing_duplicated_malformed_or_mismatched_are_rejected() {
    for header in ["mcp-protocol-version", "mcp-method", "mcp-name"] {
        let (mut headers, body) = request("tools/call");
        headers.remove(header);
        error(&headers, &body, 400, -32020);
        let (mut headers, body) = request("tools/call");
        headers.append(header, "wrong".parse().unwrap());
        error(&headers, &body, 400, -32020);
        let (mut headers, body) = request("tools/call");
        headers.insert(header, "wrong".parse().unwrap());
        error(&headers, &body, 400, -32020);
    }
    let (mut headers, body) = request("tools/call");
    headers.insert("mcp-name", "=?base64?not-base64?=".parse().unwrap());
    error(&headers, &body, 400, -32020);
}

#[test]
fn versions_and_metadata_are_checked_on_every_request() {
    let (mut headers, mut body) = request("server/discover");
    headers.insert("mcp-protocol-version", "1999-01-01".parse().unwrap());
    body["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("1999-01-01");
    let reply = parse(&headers, &body).err().unwrap();
    assert_eq!(reply.status, 400);
    assert_eq!(reply.body["error"]["code"], -32022);
    assert_eq!(
        reply.body["error"]["data"],
        json!({"supported":[protocol::VERSION],"requested":"1999-01-01"})
    );
    assert_eq!(reply.body["id"], "fixture");
    for field in [
        "io.modelcontextprotocol/protocolVersion",
        "io.modelcontextprotocol/clientCapabilities",
    ] {
        let (headers, mut body) = request("tools/call");
        body["params"]["_meta"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        error(&headers, &body, 400, -32602);
    }
    let (headers, mut body) = request("tools/list");
    body["params"]["_meta"]["org.example/extension"] = json!("x".repeat(protocol::MAX_META_BYTES));
    error(&headers, &body, 400, -32602);
}

#[test]
fn malformed_envelopes_and_notifications_never_become_tool_calls() {
    let (headers, body) = request("tools/call");
    for id in [Value::Null, json!(1.5), json!(true), json!([])] {
        let mut changed = body.clone();
        changed["id"] = id;
        error(&headers, &changed, 400, -32600);
    }
    let mut notification = body.clone();
    notification.as_object_mut().unwrap().remove("id");
    let reply = parse(&headers, &notification).err().unwrap();
    assert_eq!(reply.status, 400);
    assert!(reply.body.get("id").is_none());
    error(&headers, &json!([body.clone()]), 400, -32600);
    for id in [json!("request"), json!(-1), json!(u64::MAX)] {
        let mut valid = body.clone();
        valid["id"] = id.clone();
        let rpc = parse(&headers, &valid).unwrap();
        assert_eq!(rpc.id, id);
        let reply = Reply::result(&rpc.id, json!({}));
        assert_eq!(reply.body["id"], id);
        assert_eq!(reply.body["result"]["resultType"], "complete");
    }
    let bytes = serde_json::to_vec(&body).unwrap();
    let reply = protocol::parse(&headers, &bytes[..bytes.len() - 1])
        .err()
        .unwrap();
    assert_eq!(reply.body["error"]["code"], -32700);
}

#[test]
fn duplicates_are_rejected_before_generic_maps_discard_them() {
    let (headers, body) = request("tools/call");
    let text = serde_json::to_string(&body).unwrap();
    for (needle, replacement) in [
        ("\"id\":\"fixture\"", "\"id\":\"fixture\",\"id\":\"other\""),
        (
            "\"arguments\":{}",
            "\"arguments\":{\"cursor\":\"a\",\"cursor\":\"b\"}",
        ),
        (
            "\"clientCapabilities\":{}",
            "\"clientCapabilities\":{\"same\":1,\"same\":2}",
        ),
    ] {
        // The full namespaced key occurs in serialized metadata.
        let needle = if needle == "\"clientCapabilities\":{}" {
            "\"io.modelcontextprotocol/clientCapabilities\":{}"
        } else {
            needle
        };
        let replacement = if replacement.starts_with("\"clientCapabilities") {
            "\"io.modelcontextprotocol/clientCapabilities\":{\"same\":1,\"same\":2}"
        } else {
            replacement
        };
        let duplicate = text.replace(needle, replacement);
        assert_ne!(duplicate, text);
        let reply = protocol::parse(&headers, duplicate.as_bytes())
            .err()
            .unwrap();
        assert_eq!(reply.body["error"]["code"], -32600);
        if needle == "\"id\":\"fixture\"" {
            assert!(reply.body.get("id").is_none());
        } else {
            assert_eq!(reply.body["id"], "fixture");
        }
    }
}

#[test]
fn unknown_methods_tools_and_invalid_parameter_shapes_use_rpc_errors() {
    let (headers, body) = request("initialize");
    error(&headers, &body, 404, -32601);
    let (mut headers, mut body) = request("tools/call");
    headers.insert("mcp-name", "execute".parse().unwrap());
    body["params"]["name"] = json!("execute");
    error(&headers, &body, 400, -32602);
    let (headers, mut body) = request("tools/call");
    body["params"]["arguments"] = json!([]);
    error(&headers, &body, 400, -32602);
}

#[test]
fn required_accept_types_cannot_be_removed_by_zero_quality() {
    let (mut headers, _) = request("tools/list");
    assert!(protocol::accepts_json_and_events(&headers));
    for accept in [
        "application/json",
        "*/*",
        "application/json, text/event-stream;q=0",
        "application/json, text/event-stream;q=NaN",
    ] {
        headers.insert("accept", accept.parse().unwrap());
        assert!(!protocol::accepts_json_and_events(&headers));
    }
}

#[test]
fn catalog_is_sorted_stable_and_never_advertises_replay_safe_mutations() {
    let tools = catalog::tools().as_array().unwrap();
    assert_eq!(tools.len(), 15);
    let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        let read = matches!(name, "list_tvs" | "inspect_tv" | "get_operation");
        assert_eq!(tool["annotations"]["readOnlyHint"], read);
        assert_eq!(tool["annotations"]["idempotentHint"], read);
        assert_eq!(tool["inputSchema"]["type"], "object");
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        assert_eq!(tool["outputSchema"]["type"], "object");
        for schema in [&tool["inputSchema"], &tool["outputSchema"]] {
            jsonschema::draft202012::options()
                .offline()
                .should_validate_formats(true)
                .build(schema)
                .unwrap();
        }
        if !read {
            let required = tool["inputSchema"]["required"].as_array().unwrap();
            assert!(required.contains(&json!("server_epoch")));
            assert!(required.contains(&json!("operation_key")));
        }
    }
}

#[test]
fn duplicate_rejection_never_guesses_ambiguous_or_malformed_ids() {
    let (headers, _) = request("tools/list");
    for bytes in [
        r#"{"id":"same","id":"same","duplicate":1,"duplicate":2}"#,
        r#"{"id":null,"duplicate":1,"duplicate":2}"#,
        r#"{"id":"fixture","duplicate":1,"duplicate":2"#,
    ] {
        let reply = protocol::parse(&headers, bytes.as_bytes()).err().unwrap();
        assert!(reply.body.get("id").is_none(), "{}", reply.body);
    }
    let reply = protocol::parse(&headers, br#"{"id":42,"duplicate":1,"duplicate":2}"#)
        .err()
        .unwrap();
    assert_eq!(reply.body["id"], 42);
}
