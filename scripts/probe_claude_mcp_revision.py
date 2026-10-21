#!/usr/bin/env python3
"""Observe a Claude Code CLI MCP health check with isolated, fake credentials.

This does not test the product server, model tool use, or OAuth. It never
reads or writes the user's normal Claude configuration or TV credentials.
"""

import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading


class Probe(BaseHTTPRequestHandler):
    observations = []
    modern = True

    def log_message(self, _format, *_args):
        pass

    def do_GET(self):
        self.observations.append({"http": "GET", "path": self.path})
        self.send_error(405)

    def do_POST(self):
        length = min(int(self.headers.get("Content-Length", "0")), 65536)
        try:
            request = json.loads(self.rfile.read(length))
        except (ValueError, UnicodeDecodeError):
            self.send_error(400)
            return
        method = request.get("method")
        self.observations.append({
            "http": "POST",
            "method": method,
            "version_header": self.headers.get("MCP-Protocol-Version"),
            "version_meta": request.get("params", {}).get("_meta", {}).get(
                "io.modelcontextprotocol/protocolVersion"
            ),
            "fake_auth_present": self.headers.get("Authorization") == "Bearer m6-probe",
        })
        if method == "notifications/initialized":
            self.send_response(202)
            self.end_headers()
            return
        if method == "initialize":
            result = {
                "protocolVersion": "2025-11-25",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "m6-probe", "version": "0.0.0"},
            }
        elif method == "server/discover" and self.modern:
            result = {
                "resultType": "complete",
                "supportedVersions": ["2026-07-28"],
                "capabilities": {"tools": {}},
                "_meta": {"io.modelcontextprotocol/serverInfo": {
                    "name": "m6-probe", "version": "0.0.0"
                }},
            }
        elif method == "tools/list":
            result = {"tools": []}
            if self.modern:
                result["resultType"] = "complete"
        else:
            self.send_error(404)
            return
        payload = json.dumps({"jsonrpc": "2.0", "id": request.get("id"), "result": result}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def main():
    claude = shutil.which("claude")
    if not claude:
        raise SystemExit("Claude Code CLI is unavailable")
    Probe.observations = []
    Probe.modern = "--legacy" not in sys.argv[1:]
    with tempfile.TemporaryDirectory(prefix="m6-claude-probe-") as temporary:
        directory = Path(temporary)
        helper = directory / "fake-header.sh"
        helper.write_text("#!/bin/sh\nprintf '{\"Authorization\":\"Bearer m6-probe\"}'\n")
        helper.chmod(0o700)
        server = ThreadingHTTPServer(("127.0.0.1", 0), Probe)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        config = {
            "type": "http",
            "url": f"http://127.0.0.1:{server.server_port}/mcp",
            "headersHelper": str(helper),
        }
        environment = {
            "HOME": temporary,
            "CLAUDE_CONFIG_DIR": temporary,
            "PATH": os.environ.get("PATH", ""),
            "DISABLE_AUTOUPDATER": "1",
        }
        try:
            added = subprocess.run(
                [claude, "mcp", "add-json", "--scope", "user", "m6-probe", json.dumps(config)],
                cwd=temporary, env=environment, text=True, capture_output=True, timeout=15,
            )
            checked = subprocess.run(
                [claude, "mcp", "get", "m6-probe"],
                cwd=temporary, env=environment, text=True, capture_output=True, timeout=15,
            ) if added.returncode == 0 else None
            print(f"add-json exit: {added.returncode}")
            print(f"mcp get exit: {checked.returncode if checked else 'not run'}")
            print(f"probe mode: {'modern' if Probe.modern else 'legacy fallback'}")
            print("observed requests:")
            print(json.dumps(Probe.observations, indent=2))
        finally:
            server.shutdown()
            server.server_close()


if __name__ == "__main__":
    main()
