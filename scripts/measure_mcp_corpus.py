#!/usr/bin/env python3
"""Reproducible coarse context baseline for the sanitized P2-M6 MCP corpus.

This is a UTF-8-byte/4 proxy, not a Claude tokenizer. M10 must replace
the estimate with model-visible counts in the installed Claude Code target.
"""

import json
import math
from pathlib import Path


CORPUS = Path(__file__).resolve().parents[1] / "docs/phase-2/mcp-corpus.json"
ACTIONS = (
    "PowerToggle", "Up", "Down", "Left", "Right", "Enter", "Back", "Home",
    "PlayPause", "Play", "Pause", "Mute", "VolumeUp", "VolumeDown",
)


def compact(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def estimate(value):
    return math.ceil(len(compact(value).encode("utf-8")) / 4)


def schema(tool):
    properties = {}
    required = []
    for field in tool["fields"]:
        optional = field.endswith("?")
        name = field.removesuffix("?").split(":", 1)[0]
        if name == "action":
            properties[name] = {"type": "string", "enum": list(ACTIONS)}
        elif name in ("selection_generation", "expected_generation", "expected_config_revision", "wait_seconds"):
            properties[name] = {"type": "integer", "minimum": 0}
            if name == "wait_seconds":
                properties[name]["maximum"] = 10
        elif name == "confirmed_fingerprint":
            properties[name] = {"type": "string", "pattern": "^[0-9a-f]{64}$"}
        elif name == "label":
            properties[name] = {"type": "string", "minLength": 1, "maxLength": 80}
        elif name in {"wired", "wifi"}:
            properties[name] = {"type": ["string", "null"], "pattern": "^([0-9a-f]{2}:){5}[0-9a-f]{2}$"}
        elif name == "active":
            properties[name] = {"enum": ["wired", "wifi", None]}
        elif name in {"candidate_id", "observation_id"}:
            properties[name] = {"type": "string", "minLength": 1, "maxLength": 64}
        elif name.endswith("_id") or name in {"server_epoch", "operation_key", "recovery_transaction"}:
            properties[name] = {"type": "string", "format": "uuid"}
        else:
            properties[name] = {"type": "string"}
        if not optional:
            required.append(name)
    input_schema = {
        "type": "object", "properties": properties, "required": required,
        "additionalProperties": False,
    }
    alternatives = {
        "probe_tv": ("candidate_id", "host"),
        "get_operation": ("operation_id", "operation_key"),
        "forget_tv": ("expected_config_revision", "recovery_transaction"),
    }
    if tool["name"] in alternatives:
        first, second = alternatives[tool["name"]]
        input_schema["oneOf"] = [{"required": [first]}, {"required": [second]}]
    return {
        "name": tool["name"],
        "description": tool["description"],
        "inputSchema": input_schema,
    }


def validate(corpus):
    tools = {tool["name"]: tool for tool in corpus["tools"]}
    assert len(tools) == len(corpus["tools"]), "duplicate tool name"
    reads = {"list_tvs", "inspect_tv", "get_operation"}
    for name, tool in tools.items():
        fields = {field.removesuffix("?").split(":", 1)[0] for field in tool["fields"]}
        if name not in reads:
            assert {"server_epoch", "operation_key"} <= fields, name
        if name in {"connect_tv", "remote_action", "wake_tv"}:
            assert {"device_id", "selection_generation"} <= fields, name
        if name == "pair_tv":
            assert {
                "attempt_id", "observation_id", "confirmed_fingerprint",
                "selection_generation",
            } <= fields, name
            assert "selection_generation" in tool["fields"], "Pairing generation is optional"
        if name == "forget_tv":
            assert {"device_id", "confirm_device_id"} <= fields, name
    for name, workflow in corpus["workflows"].items():
        assert len(workflow["tools"]) == len(workflow["results"]), name
        assert all(tool in tools for tool in workflow["tools"]), name
        assert all(result in corpus["results"] for result in workflow["results"]), name
    fingerprint = corpus["results"]["probe"]["fingerprint"]
    assert len(fingerprint) == 64 and all(char in "0123456789abcdef" for char in fingerprint)


def sample_args(tool):
    fields = [field for field in tool["fields"] if not field.endswith("?")]
    if tool["name"] == "probe_tv":
        fields.append("candidate_id")
    if tool["name"] == "get_operation":
        fields.append("operation_id")
    if tool["name"] == "forget_tv":
        fields.append("expected_config_revision")
    sample = {}
    for field in fields:
        name = field.split(":", 1)[0]
        if name.endswith("generation") or name.endswith("revision") or name == "wait_seconds":
            sample[name] = 7
        elif name == "confirmed_fingerprint":
            sample[name] = "0123456789abcdef" * 4
        elif name == "action":
            sample[name] = "VolumeUp"
        elif name == "label":
            sample[name] = "Living room"
        elif name == "candidate_id":
            sample[name] = "candidate-1"
        elif name == "observation_id":
            sample[name] = "obs-1"
        elif name in {"server_epoch", "operation_key", "recovery_transaction"} or name.endswith("_id"):
            sample[name] = "a2222222-2222-4222-8222-222222222222"
        else:
            sample[name] = "sample"
    return sample


def main():
    corpus = json.loads(CORPUS.read_text())
    validate(corpus)
    compact_tools = [schema(tool) for tool in corpus["tools"]]
    remote = next(tool for tool in compact_tools if tool["name"] == "remote_action")
    full_tools = [tool for tool in compact_tools if tool["name"] != "remote_action"]
    for action in ACTIONS:
        expanded = json.loads(compact(remote))
        expanded["name"] = f"remote_{action.lower()}"
        expanded["description"] = f"Send {action} to the selected TV; a socket write does not prove TV state."
        del expanded["inputSchema"]["properties"]["action"]
        expanded["inputSchema"]["required"].remove("action")
        full_tools.append(expanded)
    print("MCP context proxy: ceil(UTF-8 bytes / 4), compact JSON; NOT Claude tokens")
    print(f"catalog: compact {len(compact_tools)} tools / {estimate(compact_tools)} proxy tokens")
    print(f"catalog: one-key-per-tool {len(full_tools)} tools / {estimate(full_tools)} proxy tokens")
    print(f"server instruction: {estimate(corpus['server_instruction'])} proxy tokens")
    for name, workflow in corpus["workflows"].items():
        visible = [tool for tool in compact_tools if tool["name"] in workflow["tools"]]
        args = [sample_args(next(
            tool for tool in corpus["tools"] if tool["name"] == tool_name
        )) for tool_name in workflow["tools"]]
        results = [corpus["results"][key] for key in workflow["results"]]
        all_upfront = estimate(compact_tools) + estimate(corpus["server_instruction"])
        searched = estimate(visible) + estimate(corpus["server_instruction"])
        body = estimate(args) + estimate(results)
        duplicate = body + estimate(results)
        print(
            f"{name}: calls={len(workflow['tools'])} "
            f"tool-search-on={searched + duplicate} "
            f"all-upfront={all_upfront + duplicate} "
            f"args+single-results={body} structured+text-extra={estimate(results)}"
        )


if __name__ == "__main__":
    main()
