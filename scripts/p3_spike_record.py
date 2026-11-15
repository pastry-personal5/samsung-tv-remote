#!/usr/bin/env python3
"""Prepare and record the finite P3-M1 hardware matrix; never connect to a TV.

Only enumerated observations are retained. Wire captures, target identifiers,
credentials and free-form notes deliberately have no input or output channel.
"""

import argparse
import datetime
import json
from pathlib import Path
import re
import sys

CONDITIONS = ("baseline", "reconnect", "wake_reconnect")
EXPERIMENTS = (
    "installed_apps", "launch_app_a", "launch_app_b", "ime_start", "ime_end",
    "hdmi1", "hdmi2", "hdmi3", "tuner", "sources_menu",
)
ROWS = tuple(f"{condition}/{experiment}" for condition in CONDITIONS for experiment in EXPERIMENTS)
RESULTS = ("untested", "observed", "not_observed", "inconclusive", "not_applicable")
MAX_RECORD_BYTES = 16 * 1024


def validate(record):
    if not isinstance(record, dict) or set(record) != {"format", "build", "dirty", "date", "observations"}:
        raise ValueError("invalid record fields")
    if type(record["format"]) is not int or record["format"] != 1:
        raise ValueError("unsupported record format")
    if not isinstance(record["build"], str) or not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", record["build"]):
        raise ValueError("build must be a full lowercase Git object ID")
    if type(record["dirty"]) is not bool:
        raise ValueError("invalid dirty flag")
    date = record["date"]
    if not isinstance(date, str) or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", date):
        raise ValueError("invalid date")
    datetime.date.fromisoformat(date)
    observations = record["observations"]
    if not isinstance(observations, dict) or set(observations) != set(ROWS):
        raise ValueError("matrix must contain every experiment exactly once")
    if any(not isinstance(value, str) or value not in RESULTS for value in observations.values()):
        raise ValueError("invalid observation")
    return record


def new_record(build, dirty, date):
    return validate({
        "format": 1, "build": build, "dirty": dirty, "date": date,
        "observations": dict.fromkeys(ROWS, "untested"),
    })


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate record field")
        result[key] = value
    return result


def load(path):
    with path.open("rb") as source:
        payload = source.read(MAX_RECORD_BYTES + 1)
    if len(payload) > MAX_RECORD_BYTES:
        raise ValueError("record too large")
    return validate(json.loads(payload, object_pairs_hook=unique_object))


def save_new(path, record):
    # Exclusive creation prevents accidental replacement of previous evidence.
    payload = json.dumps(validate(record), indent=2) + "\n"
    with path.open("x", encoding="utf-8") as output:
        output.write(payload)


def render(record):
    validate(record)
    lines = [
        "# P3-M1 operator observation matrix", "",
        f"Date: {record['date']} · Build: `{record['build']}` · Dirty: {str(record['dirty']).lower()}",
        "", "Operator declarations only; no automated TV connection or wire evidence.",
        "`not_observed` is inconclusive about support. No row establishes unsupported.",
        "App A/B are session-local aliases; reacquire returned IDs after reconnect.",
        "", "| Condition | Experiment | Operator observation |", "| --- | --- | --- |",
    ]
    lines.extend(f"| {row.split('/')[0]} | {row.split('/')[1]} | {record['observations'][row]} |" for row in ROWS)
    lines.extend([
        "", "Protocol evidence, schema freeze, fixture review and predecessor acceptance remain separate gates.",
        "See milestone-01-spike-automation.md for manual steps and the owner-harness contract.", "",
    ])
    return "\n".join(lines)


class Parser(argparse.ArgumentParser):
    def error(self, message):
        # argparse normally echoes rejected values; a mistyped private payload
        # must not reach the terminal or diagnostics either.
        self.print_usage(sys.stderr)
        self.exit(2, "Invalid arguments; use --help for the finite choices.\n")


def main(argv=None):
    parser = Parser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare = commands.add_parser("prepare", help="create a wholly untested matrix")
    prepare.add_argument("output", type=Path)
    prepare.add_argument("--build", required=True)
    prepare.add_argument("--dirty", action="store_true")
    prepare.add_argument("--date", default=datetime.datetime.now(datetime.timezone.utc).date().isoformat())
    record = commands.add_parser("record", help="copy a record with one explicit operator observation")
    record.add_argument("input", type=Path)
    record.add_argument("output", type=Path)
    record.add_argument("--row", required=True, choices=ROWS)
    record.add_argument("--result", required=True, choices=RESULTS)
    display = commands.add_parser("render", help="validate and render a record as Markdown")
    display.add_argument("input", type=Path)
    arguments = parser.parse_args(argv)
    try:
        if arguments.command == "prepare":
            save_new(arguments.output, new_record(arguments.build, arguments.dirty, arguments.date))
        elif arguments.command == "record":
            value = load(arguments.input)
            value["observations"][arguments.row] = arguments.result
            save_new(arguments.output, value)
        else:
            print(render(load(arguments.input)), end="")
    except (OSError, ValueError, RecursionError):
        # Exception text may contain input data or local paths; never echo it.
        print("Record rejected: check schema, build ID, date and absent output path.", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
