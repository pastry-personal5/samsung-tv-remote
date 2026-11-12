#!/usr/bin/env python3
"""Synthetic TLS Samsung endpoint for explicit native acceptance, never a TV.

No outgoing connections, discovery, Wake sender, token logging or app access.
Production rejects loopback; LAN permission evidence needs a separate LAN host.
"""
import argparse
import base64
import hashlib
import ipaddress
import json
from pathlib import Path
import socket
import ssl
import struct
import threading
import time
from urllib.parse import parse_qs, urlsplit

TOKEN = "m9-public-synthetic-fixture-token"
MAX_FRAME = 16 * 1024
MAX_HEADER = 8192
WS_PATH = "/api/v2/channels/samsung.remote.control"
GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
KEYS = {"KEY_POWER", "KEY_UP", "KEY_DOWN", "KEY_LEFT", "KEY_RIGHT", "KEY_ENTER",
        "KEY_RETURN", "KEY_HOME", "KEY_PLAY", "KEY_PAUSE", "KEY_MUTE", "KEY_VOLUP", "KEY_VOLDOWN"}


def read_exact(stream, count):
    result = bytearray()
    while len(result) < count:
        chunk = stream.recv(count - len(result))
        if not chunk:
            raise EOFError()
        result.extend(chunk)
    return bytes(result)


def frame(opcode, payload):
    if len(payload) > MAX_FRAME:
        raise ValueError("frame too large")
    if len(payload) < 126:
        return bytes([0x80 | opcode, len(payload)]) + payload
    return bytes([0x80 | opcode, 126]) + struct.pack("!H", len(payload)) + payload


def read_frame(stream):
    first, second = read_exact(stream, 2)
    if first & 0xF0 != 0x80 or second & 0x80 == 0:
        raise ValueError("unfragmented masked client frame required")
    opcode = first & 15
    length = second & 127
    if length == 126:
        length = struct.unpack("!H", read_exact(stream, 2))[0]
    elif length == 127:
        length = struct.unpack("!Q", read_exact(stream, 8))[0]
    if length > MAX_FRAME or (opcode >= 8 and length > 125):
        raise ValueError("frame too large")
    mask = read_exact(stream, 4)
    payload = read_exact(stream, length)
    return opcode, bytes(value ^ mask[index % 4] for index, value in enumerate(payload))


def request(stream):
    header = bytearray()
    while not header.endswith(b"\r\n\r\n"):
        if len(header) >= MAX_HEADER:
            raise ValueError("header too large")
        header.extend(read_exact(stream, 1))
    lines = header.decode("ascii").split("\r\n")
    method, target, version = lines[0].split(" ")
    if method != "GET" or version != "HTTP/1.1":
        raise ValueError("GET required")
    headers = {}
    for line in lines[1:-2]:
        key, value = line.split(":", 1)
        key = key.lower()
        if key in headers:
            raise ValueError("duplicate header")
        headers[key] = value.strip()
    if "transfer-encoding" in headers or headers.get("content-length", "0") != "0":
        raise ValueError("request body forbidden")
    return urlsplit(target), headers


def upgrade(headers):
    if headers.get("upgrade", "").lower() != "websocket" or headers.get("sec-websocket-version") != "13":
        raise ValueError("WebSocket upgrade required")
    if "upgrade" not in [part.strip().lower() for part in headers.get("connection", "").split(",")]:
        raise ValueError("upgrade connection required")
    key = headers.get("sec-websocket-key", "")
    if len(base64.b64decode(key, validate=True)) != 16:
        raise ValueError("invalid WebSocket key")
    accept = base64.b64encode(hashlib.sha1((key + GUID).encode("ascii")).digest()).decode("ascii")
    return ("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n"
            f"Connection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").encode("ascii")


class Fixture:
    def __init__(self, context, mode="approve"):
        self.context = context
        self.mode = mode
        self.lock = threading.Lock()
        self.slots = threading.BoundedSemaphore(8)
        self.stopping = threading.Event()
        self.connections = set()
        self.workers = set()

    def emit(self, event):
        # Fixed event names only. Never log peers, URLs, headers or payloads.
        with self.lock:
            print(json.dumps({"fixture_event": event}), flush=True)

    def current_mode(self):
        with self.lock:
            return self.mode

    def set_mode(self, mode):
        if mode not in ("approve", "deny", "offline"):
            raise ValueError("unsupported mode")
        with self.lock:
            self.mode = mode
            connections = tuple(self.connections) if mode == "offline" else ()
        # Interrupt live sessions; a later approve permits only new sessions.
        for connection in connections:
            try:
                connection.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        self.emit(mode)

    def handle(self, plain):
        connection = plain
        try:
            plain.settimeout(5)
            if self.current_mode() == "offline":
                return
            connection = self.context.wrap_socket(plain, server_side=True)
            with self.lock:
                self.connections.add(connection)
            target, headers = request(connection)
            if target.path == "/api/v2/" and not target.query:
                body = json.dumps({"device": {"name": "M9 Synthetic TV", "modelName": "M9 TLS Fixture"}}).encode("ascii")
                connection.sendall((f"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {len(body)}\r\nConnection: close\r\n\r\n").encode("ascii") + body)
                self.emit("metadata")
                return
            if target.path != WS_PATH:
                raise ValueError("unsupported route")
            query = parse_qs(target.query, strict_parsing=True)
            if set(query) - {"name", "token"} or len(query.get("name", [])) != 1 or len(query.get("token", [])) > 1:
                raise ValueError("unsupported query")
            connection.sendall(upgrade(headers))
            mode = self.current_mode()
            if mode != "approve" or query.get("token", [TOKEN])[0] != TOKEN:
                connection.sendall(frame(1, b'{"event":"ms.channel.unauthorized"}'))
                self.emit("authorization_denied")
                return
            reconnect = "token" in query
            event = {"event": "ms.channel.connect", "data": {} if reconnect else {"token": TOKEN}}
            connection.sendall(frame(1, json.dumps(event).encode("ascii")))
            self.emit("reconnect_authorized" if reconnect else "pairing_authorized")
            # Read timeout closes idle fixtures rather than retaining workers forever.
            connection.settimeout(120)
            while not self.stopping.is_set() and self.current_mode() != "offline":
                opcode, payload = read_frame(connection)
                if opcode == 8:
                    connection.sendall(frame(8, payload))
                    break
                if opcode == 9:
                    connection.sendall(frame(10, payload))
                elif opcode == 1:
                    value = json.loads(payload)
                    if not isinstance(value, dict) or value.get("method") != "ms.remote.control" or not isinstance(value.get("params"), dict):
                        raise ValueError("unsupported command")
                    params = value["params"]
                    if (params.get("Cmd") != "Click" or params.get("Option") != "false"
                            or params.get("TypeOfRemote") != "SendRemoteKey"
                            or not isinstance(params.get("DataOfCmd"), str)
                            or params["DataOfCmd"] not in KEYS):
                        raise ValueError("unsupported key")
                    self.emit("synthetic_key_received")
                elif opcode != 10:
                    raise ValueError("unsupported opcode")
        except (OSError, EOFError, ValueError, UnicodeError, RecursionError):
            self.emit("connection_closed")
        finally:
            with self.lock:
                self.connections.discard(connection)
                self.workers.discard(threading.current_thread())
            connection.close()
            self.slots.release()

    def serve(self, listener):
        listener.settimeout(0.5)
        while not self.stopping.is_set():
            try:
                connection, _ = listener.accept()
            except socket.timeout:
                continue
            if self.slots.acquire(blocking=False):
                worker = threading.Thread(target=self.handle, args=(connection,), daemon=True)
                with self.lock:
                    self.workers.add(worker)
                worker.start()
            else:
                connection.close()
                self.emit("capacity_rejected")

    def join_handlers(self):
        with self.lock:
            workers = tuple(self.workers)
        deadline = time.monotonic() + 6
        for worker in workers:
            worker.join(timeout=max(0, deadline - time.monotonic()))


def bind_address(value):
    address = ipaddress.IPv4Address(value)
    local = (ipaddress.ip_network("10.0.0.0/8"), ipaddress.ip_network("172.16.0.0/12"),
             ipaddress.ip_network("192.168.0.0/16"), ipaddress.ip_network("169.254.0.0/16"))
    if not address.is_loopback and not any(address in network for network in local):
        raise argparse.ArgumentTypeError("explicit private IPv4 or loopback interface required")
    return str(address)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bind", required=True, type=bind_address)
    parser.add_argument("--cert", required=True, type=Path)
    parser.add_argument("--key", required=True, type=Path)
    parser.add_argument("--mode", choices=("approve", "deny", "offline"), default="approve")
    args = parser.parse_args()
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(args.cert, args.key)
    fixture = Fixture(context, args.mode)
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind((args.bind, 8002))
        listener.listen(8)
        worker = threading.Thread(target=fixture.serve, args=(listener,), daemon=True)
        worker.start()
        fixture.emit("ready")
        try:
            for command in iter(input, "quit"):
                if command in ("approve", "deny", "offline"):
                    fixture.set_mode(command)
                else:
                    fixture.emit("unknown_control")
        except (EOFError, KeyboardInterrupt):
            pass
        finally:
            fixture.stopping.set()
            fixture.set_mode("offline")
            worker.join(timeout=2)
            fixture.join_handlers()


if __name__ == "__main__":
    main()
