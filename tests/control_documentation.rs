//! Reconcile corpus inputs with the implemented MCP catalog and native parser.
//! Historical result projections remain a proxy, not actual client evidence.
use samsung_tv_remote::server::control_http::{
    request::{parse_mutation, parse_stop, MutationRoute},
    wire::{self, Counter},
};
use serde_json::Value;

#[test]
fn every_corpus_input_conforms_to_the_shipped_tool_schema() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("python3")
        .arg(root.join("scripts/measure_mcp_corpus.py"))
        .arg("--wire-fixtures")
        .output()
        .unwrap();
    assert!(output.status.success());
    let samples: Value = serde_json::from_slice(&output.stdout).unwrap();
    let catalog = samsung_tv_remote::server::mcp::catalog::tools()
        .as_array()
        .unwrap();
    assert_eq!(samples.as_object().unwrap().len(), catalog.len());
    for tool in catalog {
        let name = tool["name"].as_str().unwrap();
        let arguments = &samples[name]["arguments"];
        assert!(arguments.is_object(), "missing corpus input: {name}");
        let validator = jsonschema::draft202012::options()
            .offline()
            .should_validate_formats(true)
            .build(&tool["inputSchema"])
            .unwrap();
        let errors: Vec<_> = validator
            .iter_errors(arguments)
            .map(|e| e.to_string())
            .collect();
        assert!(errors.is_empty(), "{name}: {errors:?}");
    }
    // Count the complete shipped definitions, including output schemas and
    // annotations. The historical M6 input-only estimate is not this catalog.
    let bytes = serde_json::to_vec(samsung_tv_remote::server::mcp::catalog::tools())
        .unwrap()
        .len();
    let instructions = samsung_tv_remote::server::mcp::catalog::INSTRUCTIONS.len();
    println!("Shipped catalog UTF-8 proxy: definitions={} bytes / {} units; instructions={} bytes / {} units. Actual client tokens unmeasured.", bytes, bytes.div_ceil(4), instructions, instructions.div_ceil(4));
}

#[test]
fn proposed_mcp_mutation_arguments_parse_as_the_corresponding_http_commands() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("python3")
        .arg(root.join("scripts/measure_mcp_corpus.py"))
        .arg("--wire-fixtures")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let samples: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut mutations = 0;
    for (name, sample) in samples.as_object().unwrap() {
        if ["list_tvs", "inspect_tv", "get_operation"].contains(&name.as_str()) {
            continue;
        }
        let mut arguments = sample["arguments"].clone();
        let device = arguments["device_id"].as_str().unwrap_or("").to_owned();
        let operation = arguments.as_object_mut().unwrap().remove("operation_id");
        let operation = operation.as_ref().and_then(Value::as_str).unwrap_or("");
        let route = match name.as_str() {
            "discover_tvs" => MutationRoute::Discover,
            "probe_tv" => MutationRoute::Probe,
            "pair_tv" => MutationRoute::Pair,
            "select_tv" => MutationRoute::Select,
            "connect_tv" => MutationRoute::Connect,
            "set_wake" => MutationRoute::SetWake(&device),
            "remote_action" => MutationRoute::Remote,
            "wake_tv" => MutationRoute::Wake,
            "cancel_wake" => MutationRoute::CancelWake(operation),
            "forget_tv" => MutationRoute::Forget(&device),
            "recover_pairing" => MutationRoute::RecoverPairing(&device),
            "recover_forget" => MutationRoute::RecoverForget(&device),
            _ => panic!("undocumented mutation tool {name}"),
        };
        let epoch = wire::parse_epoch(arguments["server_epoch"].as_str().unwrap()).unwrap();
        let bytes = serde_json::to_vec(&arguments).unwrap();
        let request = parse_mutation(route, &bytes, epoch)
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        let (_, _, native) =
            samsung_tv_remote::control_client::projection::encode(&request).unwrap();
        let decoded = parse_mutation(route, &serde_json::to_vec(&native).unwrap(), epoch).unwrap();
        assert_eq!(decoded, request);
        mutations += 1;
        let properties = sample["schema"]["properties"].as_object().unwrap();
        for field in [
            "selection_generation",
            "expected_generation",
            "expected_config_revision",
        ] {
            if let Some(schema) = properties.get(field) {
                assert_eq!(schema["type"], "string");
            }
        }
        assert!(!properties.contains_key("client_id"));
    }
    assert_eq!(mutations, 12);
}

fn check_result(value: &Value, epoch: samsung_tv_remote::application::control_service::Epoch) {
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                if matches!(
                    name.as_str(),
                    "selection_generation"
                        | "current_selection_generation"
                        | "config_revision"
                        | "revision"
                        | "expires_at_unix_ms"
                ) && !value.is_null()
                {
                    serde_json::from_value::<Counter>(value.clone()).unwrap();
                }
                if let Some(text) = value.as_str() {
                    match name.as_str() {
                        "operation_id" | "attempt_id" | "observation_id" => {
                            wire::parse_operation(text, epoch, epoch).unwrap();
                        }
                        "candidate_id" => {
                            wire::parse_candidate(text, epoch, epoch).unwrap();
                        }
                        _ => (),
                    }
                }
                check_result(value, epoch);
            }
        }
        Value::Array(values) => {
            for value in values {
                check_result(value, epoch);
            }
        }
        _ => (),
    }
}
#[test]
fn corpus_results_preserve_full_epoch_scoped_handles_lossless_counters_and_empty_first_use() {
    let corpus: Value =
        serde_json::from_str(include_str!("../docs/phase-2/mcp-corpus.json")).unwrap();
    for result in corpus["results"].as_object().unwrap().values() {
        let epoch = wire::parse_epoch(result["server_epoch"].as_str().unwrap()).unwrap();
        check_result(result, epoch);
    }
    let probe = &corpus["results"]["probe"];
    assert_eq!(probe["attempt_id"], probe["observation_id"]);
    assert_eq!(
        corpus["results"]["fresh_list"]["tvs"],
        serde_json::json!([])
    );
    assert!(corpus["results"]["fresh_list"]["selected"].is_null());
    assert_eq!(corpus["results"]["fresh_list"]["selection_generation"], "0");
}
#[test]
fn published_json_request_and_receipt_examples_use_the_actual_parser() {
    let mut requests = 0;
    let mut receipts = 0;
    let mut stops = 0;
    for document in [
        include_str!("../docs/phase-2/milestone-06-contract.md"),
        include_str!("../docs/phase-2/milestone-06-api-examples.md"),
    ] {
        for block in document.split("```json\n").skip(1) {
            let json = block.split("\n```").next().unwrap();
            let value: Value = serde_json::from_str(json).unwrap();
            let epoch = wire::parse_epoch(value["server_epoch"].as_str().unwrap()).unwrap();
            if value.get("action").is_some() {
                parse_mutation(MutationRoute::Remote, json.as_bytes(), epoch).unwrap();
                requests += 1;
            } else if value.get("state").is_some() {
                samsung_tv_remote::control_client::projection::NativeReceipt::parse(&value, epoch)
                    .unwrap();
                receipts += 1;
            } else {
                parse_stop(json.as_bytes(), epoch).unwrap();
                stops += 1;
            }
        }
    }
    assert_eq!((requests, receipts, stops), (2, 2, 1));
}
