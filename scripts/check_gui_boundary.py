#!/usr/bin/env python3
"""Required source dependency guard for GUI modules and default composition.

This checks architectural imports/calls, not a hostile-code sandbox. Shared
value types live in application/control_client; runtime, policy services and
adapters must remain behind the client. Cargo tests run the negative corpus.
"""
import argparse
from pathlib import Path
import re
import sys
import unittest

FORBIDDEN = {
    "infrastructure", "in_process", "server", "runtime",
    "tv_control_coordinator", "tv_setup_service", "tv_session",
    "InProcess", "Runtime", "RuntimeLimits", "ControlService", "AppServices",
    "SamsungGateway", "SsdpDiscovery", "UdpWakeTransport", "KeychainCredentialStore",
    "ServerKeychain", "ServerPairingCredentials", "ServerApiCredentials",
    "LocalDeviceRepository", "LocalCertificateTrustStore", "LocalRecoveryJournal",
    "Receipt", "ServiceError", "Lookup", "serde_json",
    "TvSessionGateway", "TvSessionControl", "TvConnection", "TvDiscovery",
    "WakeTransport", "CredentialStore", "DeviceRepository", "CertificateTrustStore",
    "RecoveryJournal", "TvSetupPort",
}

def code_only(text):
    """Mask Rust comments and string literals while preserving token spacing."""
    out = list(text)
    i = 0
    while i < len(text):
        start = i
        if text.startswith("//", i):
            end = text.find("\n", i)
            i = len(text) if end < 0 else end
        elif text.startswith("/*", i):
            depth = 1
            i += 2
            while i < len(text) and depth:
                if text.startswith("/*", i):
                    depth += 1
                    i += 2
                elif text.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
        elif match := re.match(r'(?:br|r)(#*)"', text[i:]):
            close = '"' + match[1]
            end = text.find(close, i + len(match[0]))
            i = len(text) if end < 0 else end + len(close)
        elif text[i] == '"':
            i += 1
            while i < len(text):
                if text[i] == "\\":
                    i += 2
                elif text[i] == '"':
                    i += 1
                    break
                else:
                    i += 1
        else:
            i += 1
            continue
        out[start:i] = " " * (i - start)
    return "".join(out)

def without_capture_io(code):
    # The only GUI file I/O is this opt-in, debug-only renderer evidence hook.
    # The global adapter/type checks still apply inside the hook. Every other
    # GUI file access, including a second screenshot handler, is rejected.
    pattern = (r'Message\s*::\s*ProbeCaptured\s*\(screenshot\)\s*if\s*'
               r'self\s*\.\s*development_probe\s*&&\s*cfg!\s*\(debug_assertions\)\s*=>\s*\{')
    match = re.search(pattern, code)
    if not match:
        return code
    depth, end = 1, match.end()
    while end < len(code) and depth:
        depth += (code[end] == "{") - (code[end] == "}")
        end += 1
    return code[:match.start()] + " " * (end - match.start()) + code[end:]

def violations(text, main=False):
    code = code_only(text)
    if main:
        # Early mode dispatch is the sole allowed server reference in main.
        code = re.sub(r'samsung_tv_remote\s*::\s*server\s*::\s*entry\s*::\s*dispatch', 'dispatch', code)
    identifiers = set(re.findall(r'\b[A-Za-z_][A-Za-z_0-9]*\b', code))
    errors = sorted(FORBIDDEN & identifiers)
    if re.search(r'\buse\b[^;]*\bapplication\b[^;]*\*', code):
        errors.append("wildcard application/service import")
    io_code = without_capture_io(code) if not main else code
    for token in ["fs", "File", "OpenOptions", "include"]:
        if re.search(r'\b' + token + r'\b', io_code):
            errors.append(token)
    return errors

class NegativeCorpus(unittest.TestCase):
    def test_forbidden_dependencies_and_aliases(self):
        for source in [
            'use crate::application::runtime as owner;',
            'use crate::application::control_service::*;',
            'use crate::application::wake_transport::*;',
            'use crate::application::credential_store::CredentialStore as Store;',
            'use crate::{infrastructure as disk};',
            'fn boot() { ControlService::new(); }',
            'fn boot() { ServerKeychain::api(); }',
            'fn boot() { SamsungGateway; }',
            'fn boot() { AppServices::new(); }',
            'use crate::server::production::ProductionProfile;',
            'fn fallback() { InProcess::start(); }',
            'fn raw() { serde_json::from_slice(bytes); }',
            'fn settings() { std::fs::read(path); }',
            'include!("../../infrastructure/mod.rs");',
        ]:
            with self.subTest(source=source):
                self.assertTrue(violations(source))
    def test_main_exempts_only_dispatch(self):
        self.assertFalse(violations('samsung_tv_remote::server::entry::dispatch(&args);',main=True))
        self.assertTrue(violations('samsung_tv_remote::server::production::ProductionProfile::open();',main=True))
    def test_values_and_resource_literals_are_allowed(self):
        self.assertFalse(violations('use crate::control_client::model::WakeStage; // Runtime\nlet label=r#"server::Runtime"#; let asset=include_bytes!("remote.png");'))
    def test_capture_exception_requires_both_guards(self):
        guarded='Message::ProbeCaptured(screenshot) if self.development_probe && cfg!(debug_assertions) => { std::fs::File::create_new(path); }'
        self.assertFalse(violations(guarded))
        self.assertTrue(violations(guarded.replace('cfg!(debug_assertions)', 'true')))
        self.assertTrue(violations(guarded.replace('self.development_probe', 'true')))
        self.assertTrue(violations(guarded + ' fn read() {std::fs::read(path);}'))
        self.assertTrue(violations(guarded.replace('std::fs::File::create_new(path);','ServerKeychain::api();')))
    def test_external_module_attribute_is_rejected(self):
        import tempfile
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            gui = root / "src/presentation"
            gui.mkdir(parents=True)
            (root / "src/main.rs").write_text("fn main() {}")
            (gui / "view.rs").write_text('#[path="../../owner.rs"] mod owner;')
            self.assertTrue(any("external #[path]" in error for error in check(root)))

def check(root):
    failures = []
    gui = root / "src/presentation"
    sources = sorted(gui.rglob("*.rs")) + [root / "src/main.rs"]
    for path in sources:
        text = path.read_text()
        errors = violations(text, main=path.name == "main.rs")
        # Path attributes cannot smuggle modules from outside presentation.
        for target in re.findall(r'#\s*\[\s*path\s*=\s*"([^"]+)"\s*\]', text):
            resolved = (path.parent / target).resolve()
            if gui.resolve() not in resolved.parents:
                errors.append("external #[path] module")
        if errors:
            failures.append(f"{path.relative_to(root)}: {', '.join(errors)}")
    if (gui / "iced/in_process.rs").exists():
        failures.append("temporary in_process adapter still exists")
    return failures

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        result = unittest.TextTestRunner().run(unittest.defaultTestLoader.loadTestsFromTestCase(NegativeCorpus))
        if not result.wasSuccessful():
            sys.exit(1)
    problems = check(Path(__file__).resolve().parent.parent)
    if problems:
        print("GUI boundary violations:\n" + "\n".join(problems), file=sys.stderr)
        sys.exit(1)
    print("GUI boundary passed: presentation modules and main composition")
