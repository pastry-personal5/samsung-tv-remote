"""No TV/LAN or Keychain: native acceptance fixture self-checks on loopback."""
import base64
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import socket
import ssl
import struct
import subprocess
import tempfile
import threading
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("m9_tv_fixture", ROOT / "scripts/m9_tv_fixture.py")
fixture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fixture)


def client_frame(opcode, payload):
    mask = b"abcd"
    length = bytes([0x80 | len(payload)]) if len(payload) < 126 else b"\xfe" + struct.pack("!H", len(payload))
    return bytes([0x80 | opcode]) + length + mask + bytes(value ^ mask[index % 4] for index, value in enumerate(payload))


def server_frame(stream):
    first, length = fixture.read_exact(stream, 2)
    if length == 126:
        length = struct.unpack("!H", fixture.read_exact(stream, 2))[0]
    return first & 15, fixture.read_exact(stream, length)


class BytesStream(io.BytesIO):
    def recv(self, count):
        return self.read(count)


class FrameTests(unittest.TestCase):
    def test_fragmented_unmasked_and_oversized_frames_are_rejected(self):
        for data in (b"\x01\x80", b"\x81\x00", b"\x81\xff" + struct.pack("!Q", fixture.MAX_FRAME + 1)):
            with self.assertRaises(ValueError):
                fixture.read_frame(BytesStream(data))

    def test_masked_text_round_trip_and_upgrade_key_validation(self):
        self.assertEqual(fixture.read_frame(BytesStream(client_frame(1, b"a" * 180))), (1, b"a" * 180))
        headers = {"upgrade": "websocket", "connection": "Upgrade", "sec-websocket-version": "13", "sec-websocket-key": "dGhlIHNhbXBsZSBub25jZQ=="}
        self.assertIn(b"s3pPLMBiTxaQ9kYGzzhZRbK+xOo=", fixture.upgrade(headers))
        headers["sec-websocket-key"] = "bad"
        with self.assertRaises(ValueError):
            fixture.upgrade(headers)

    def test_request_headers_are_bounded_and_body_framing_is_rejected(self):
        for data in (b"GET / HTTP/1.1\r\nX: " + b"x" * fixture.MAX_HEADER,
                     b"GET /api/v2/ HTTP/1.1\r\nContent-Length: 1\r\n\r\n",
                     b"GET /api/v2/ HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n"):
            with self.assertRaises(ValueError):
                fixture.request(BytesStream(data))

    def test_binding_cannot_broaden_to_wildcard_public_or_multicast(self):
        for address in ("0.0.0.0", "8.8.8.8", "224.0.0.1", "::1"):
            with self.assertRaises((ValueError, fixture.argparse.ArgumentTypeError)):
                fixture.bind_address(address)
        self.assertEqual(fixture.bind_address("127.0.0.1"), "127.0.0.1")


class TlsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory(prefix="m9-fixture-test-")
        cert = Path(cls.directory.name) / "cert.pem"
        key = Path(cls.directory.name) / "key.pem"
        subprocess.run(["/usr/bin/openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1", "-subj", "/CN=M9 Synthetic TV", "-keyout", str(key), "-out", str(cert)], check=True, capture_output=True)
        cls.context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        cls.context.load_cert_chain(cert, key)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def setUp(self):
        self.output = io.StringIO()
        self.redirect = contextlib.redirect_stdout(self.output)
        self.redirect.__enter__()
        self.fixture = fixture.Fixture(self.context)
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(8)
        self.worker = threading.Thread(target=self.fixture.serve, args=(self.listener,))
        self.worker.start()

    def tearDown(self):
        self.fixture.stopping.set()
        self.fixture.set_mode("offline")
        self.worker.join(timeout=2)
        self.listener.close()
        self.fixture.join_handlers()
        self.redirect.__exit__(None, None, None)
        self.assertFalse(self.worker.is_alive())
        self.assertNotIn(fixture.TOKEN, self.output.getvalue())
        self.assertNotIn("token=", self.output.getvalue())
        self.assertNotIn("127.0.0.1", self.output.getvalue())

    def connect(self):
        plain = socket.create_connection(self.listener.getsockname(), timeout=3)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        context.check_hostname = False
        context.verify_mode = ssl.CERT_NONE
        return context.wrap_socket(plain, server_hostname="fixture.invalid")

    def websocket(self, query="name=c3ludGhldGlj"):
        connection = self.connect()
        key = base64.b64encode(b"abcdefghijklmnop").decode("ascii")
        connection.sendall((f"GET {fixture.WS_PATH}?{query} HTTP/1.1\r\nHost: fixture.invalid:8002\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {key}\r\n\r\n").encode("ascii"))
        header = bytearray()
        while not header.endswith(b"\r\n\r\n"):
            header.extend(fixture.read_exact(connection, 1))
        self.assertTrue(header.startswith(b"HTTP/1.1 101"))
        return connection

    def test_metadata_pair_reconnect_key_and_ping_use_real_tls(self):
        with self.connect() as connection:
            connection.sendall(b"GET /api/v2/ HTTP/1.1\r\nHost: fixture.invalid\r\n\r\n")
            response = bytearray()
            while chunk := connection.recv(4096):
                response.extend(chunk)
            self.assertIn(b"M9 Synthetic TV", response)
        with self.websocket() as connection:
            _, payload = server_frame(connection)
            self.assertEqual(json.loads(payload)["data"]["token"], fixture.TOKEN)
        with self.websocket("name=c3ludGhldGlj&token=" + fixture.TOKEN) as connection:
            _, payload = server_frame(connection)
            self.assertEqual(json.loads(payload), {"event": "ms.channel.connect", "data": {}})
            connection.sendall(client_frame(1, b'{"method":"ms.remote.control","params":{"Cmd":"Click","DataOfCmd":"KEY_UP","Option":"false","TypeOfRemote":"SendRemoteKey"}}'))
            connection.sendall(client_frame(9, b"ping"))
            self.assertEqual(server_frame(connection), (10, b"ping"))
            connection.sendall(client_frame(8, b""))
            self.assertEqual(server_frame(connection), (8, b""))
        self.assertIn("synthetic_key_received", self.output.getvalue())

    def test_malformed_command_is_closed_without_counting_a_key(self):
        with self.websocket() as connection:
            server_frame(connection)
            connection.sendall(client_frame(1, b'{"method":"ms.remote.control","params":{"DataOfCmd":"KEY_UP"}}'))
            self.assertEqual(connection.recv(1), b"")
        self.assertNotIn("synthetic_key_received", self.output.getvalue())

    def test_deny_and_wrong_token_never_authorize(self):
        for query in ("name=c3ludGhldGlj&token=wrong", "name=c3ludGhldGlj"):
            if "wrong" not in query:
                self.fixture.set_mode("deny")
            with self.websocket(query) as connection:
                _, payload = server_frame(connection)
                self.assertEqual(json.loads(payload), {"event": "ms.channel.unauthorized"})

    def test_offline_interrupts_session_and_explicit_approve_recovers(self):
        with self.websocket() as connection:
            server_frame(connection)
            self.fixture.set_mode("offline")
            self.assertEqual(connection.recv(1), b"")
        with self.assertRaises(OSError):
            self.connect()
        self.fixture.set_mode("approve")
        with self.websocket("name=c3ludGhldGlj&token=" + fixture.TOKEN) as connection:
            _, payload = server_frame(connection)
            self.assertEqual(json.loads(payload)["event"], "ms.channel.connect")


if __name__ == "__main__":
    unittest.main()
