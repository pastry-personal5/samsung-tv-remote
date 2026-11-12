"""Installer regressions use temporary bundles and fake processes/signatures."""
from contextlib import ExitStack
import importlib.util
import io
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("macos_install", ROOT / "scripts/install_macos_bundle.py")
installer = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = installer
SPEC.loader.exec_module(installer)


class ProcessTests(unittest.TestCase):
    def setUp(self):
        self.bundle = Path("/Applications/Samsung TV Remote.app")
        self.executable = str(self.bundle / "Contents/MacOS/samsung-tv-remote")
        self.process = installer.Process(os.getuid(), 1234, "Thu Nov 12 22:11:43 2026", self.executable)

    def test_discovery_matches_exact_executable_and_keeps_spaces_and_start_time(self):
        rows = "\n".join(
            f" {os.getuid()} {pid} Thu Nov 12 22:11:43 2026 {path}"
            for pid, path in [
                (1234, self.executable),
                (1235, self.executable + "-helper"),
                (1236, "/somewhere/else/Samsung TV Remote.app/Contents/MacOS/samsung-tv-remote"),
                (1237, "/bin/zsh"),
            ]
        )
        with patch.object(installer, "command", return_value=rows) as command:
            self.assertEqual(installer.bundle_processes(self.bundle), [self.process])
        self.assertEqual(command.call_args.args[0], ["ps", "-ww", "-axo", "uid=,pid=,lstart=,comm="])

    def test_failed_or_unparseable_process_list_never_claims_idle(self):
        with patch.object(installer, "command", return_value="unparseable"):
            with self.assertRaises(installer.InstallError):
                installer.ensure_idle(self.bundle)
        with patch.object(installer, "command", side_effect=installer.InstallError("ps denied")):
            with self.assertRaisesRegex(installer.InstallError, "ps denied"):
                installer.ensure_idle(self.bundle)

    def test_settlement_sends_only_term_once_and_waits_for_exit(self):
        with patch.object(installer, "bundle_processes", side_effect=[
            [self.process], [self.process], [self.process], [],
        ]), patch.object(installer.os, "kill") as kill, patch.object(installer.time, "sleep"):
            installer.settle(self.bundle, 30)
        kill.assert_called_once_with(self.process.pid, signal.SIGTERM)

    def test_changed_pid_identity_is_not_signalled(self):
        reused = installer.Process(self.process.uid, self.process.pid,
                                   "Thu Nov 12 22:33:32 2026", self.executable)
        with patch.object(installer, "bundle_processes", side_effect=[
            [self.process], [reused], [],
        ]), patch.object(installer.os, "kill") as kill, patch.object(installer.time, "sleep"):
            installer.settle(self.bundle, 30)
        kill.assert_not_called()

    def test_foreign_owner_blocks_replacement_without_signals(self):
        foreign = installer.Process(os.getuid() + 1, 1234, self.process.started, self.executable)
        with patch.object(installer, "bundle_processes", return_value=[foreign]), \
                patch.object(installer.os, "kill") as kill:
            with self.assertRaisesRegex(installer.InstallError, "Another user"):
                installer.settle(self.bundle, 30)
        kill.assert_not_called()

    def test_timeout_never_escalates_to_force_kill(self):
        with patch.object(installer, "bundle_processes", return_value=[self.process]), \
                patch.object(installer.os, "kill") as kill, \
                patch.object(installer.time, "monotonic", side_effect=[0, 31]):
            with self.assertRaisesRegex(installer.InstallError, "Timed out"):
                installer.settle(self.bundle, 30)
        kill.assert_called_once_with(self.process.pid, signal.SIGTERM)


class InstallationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="samsung-install-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "build/Samsung TV Remote.app"
        self.destination = self.root / "Applications/Samsung TV Remote.app"
        self.write_bundle(self.source, "new")
        self.write_bundle(self.destination, "old")
        self.events = []
        stack = ExitStack()
        self.addCleanup(stack.close)
        self.verify = stack.enter_context(patch.object(installer, "verify_bundle", side_effect=self.verify_bundle))
        self.copy = stack.enter_context(patch.object(installer, "command", side_effect=self.command))
        self.settle = stack.enter_context(patch.object(installer, "settle", side_effect=lambda *args: self.events.append("settle")))
        self.idle = stack.enter_context(patch.object(installer, "ensure_idle", side_effect=lambda *args: self.events.append("idle")))
        stack.enter_context(patch("sys.stdout", new_callable=io.StringIO))
        # A test bug must never reach a real process or signing tool.
        self.kill = stack.enter_context(patch.object(installer.os, "kill", side_effect=AssertionError("real kill")))
        stack.enter_context(patch.object(installer.subprocess, "run", side_effect=AssertionError("real command")))

    @staticmethod
    def write_bundle(path, contents):
        executable = path / "Contents/MacOS/samsung-tv-remote"
        executable.parent.mkdir(parents=True)
        executable.write_text(contents)

    @staticmethod
    def contents(path):
        return (path / "Contents/MacOS/samsung-tv-remote").read_text()

    def verify_bundle(self, path):
        self.events.append(("verify", path))
        self.assertTrue((path / "Contents/MacOS/samsung-tv-remote").is_file())

    def command(self, arguments):
        self.assertEqual(arguments[:2], ["/usr/bin/ditto", "--noacl"])
        self.events.append("copy")
        shutil.copytree(arguments[2], arguments[3])
        return ""

    def previous(self):
        return list(self.destination.parent.glob(".samsung-tv-remote.previous-*.noindex/*.app"))

    def test_copy_and_verify_precede_settlement_and_old_bundle_is_retained(self):
        installer.install(self.source, self.destination)
        self.assertEqual(self.contents(self.destination), "new")
        self.assertEqual(self.contents(self.source), "new")
        self.assertEqual(self.contents(self.previous()[0]), "old")
        self.assertEqual([event[0] if isinstance(event, tuple) else event for event in self.events],
                         ["verify", "copy", "verify", "settle", "idle", "verify"])
        self.assertFalse(list(self.destination.parent.glob(".samsung-tv-remote.install-*.noindex")))
        self.kill.assert_not_called()

    def test_first_install_does_not_need_a_previous_bundle(self):
        shutil.rmtree(self.destination)
        installer.install(self.source, self.destination)
        self.assertEqual(self.contents(self.destination), "new")
        self.assertEqual(self.previous(), [])

    def test_invalid_source_never_closes_the_running_app(self):
        self.verify.side_effect = installer.InstallError("invalid source signature")
        with self.assertRaisesRegex(installer.InstallError, "invalid source signature"):
            installer.install(self.source, self.destination)
        self.settle.assert_not_called()
        self.copy.assert_not_called()
        self.assertEqual(self.contents(self.destination), "old")

    def test_staging_copy_or_verification_failure_preserves_old_app_without_signalling(self):
        for failure in ("copy", "staged signature"):
            with self.subTest(failure=failure):
                self.copy.side_effect = self.command if failure != "copy" else installer.InstallError("copy")
                self.verify.side_effect = [None, installer.InstallError("staged signature")]
                with self.assertRaises(installer.InstallError):
                    installer.install(self.source, self.destination)
                self.settle.assert_not_called()
                self.assertEqual(self.contents(self.destination), "old")

    def test_settlement_timeout_and_last_moment_launch_preserve_the_old_bundle(self):
        for failing_check in (self.settle, self.idle):
            with self.subTest(check=failing_check):
                original = failing_check.side_effect
                failing_check.side_effect = installer.InstallError("still running")
                with self.assertRaisesRegex(installer.InstallError, "still running"):
                    installer.install(self.source, self.destination)
                failing_check.side_effect = original
                self.assertEqual(self.contents(self.destination), "old")
                self.assertEqual(self.previous(), [])

    def test_failed_publication_restores_old_bundle(self):
        original = Path.rename

        def fail_publication(path, target):
            if ".install-" in str(path.parent):
                raise OSError("publication failed")
            return original(path, target)

        with patch.object(Path, "rename", new=fail_publication):
            with self.assertRaisesRegex(installer.InstallError, "previous bundle restored"):
                installer.install(self.source, self.destination)
        self.assertEqual(self.contents(self.destination), "old")
        self.assertEqual(self.previous(), [])

    def test_failed_final_verification_restores_old_bundle(self):
        self.verify.side_effect = [None, None, installer.InstallError("installed signature")]
        with self.assertRaisesRegex(installer.InstallError, "previous bundle restored"):
            installer.install(self.source, self.destination)
        self.assertEqual(self.contents(self.destination), "old")

    def test_failed_rollback_retains_backup_and_staged_files(self):
        self.verify.side_effect = [None, None, installer.InstallError("installed signature")]
        original = Path.rename

        def fail_restore(path, target):
            if ".previous-" in str(path.parent):
                raise OSError("restore failed")
            return original(path, target)

        with patch.object(Path, "rename", new=fail_restore):
            with self.assertRaisesRegex(installer.InstallError, "rollback needs attention"):
                installer.install(self.source, self.destination)
        self.assertEqual(self.contents(self.previous()[0]), "old")
        staged = list(self.destination.parent.glob(".samsung-tv-remote.install-*.noindex/*.app"))
        self.assertEqual(self.contents(staged[0]), "new")

    def test_parallel_installer_cannot_copy_or_stop_processes(self):
        with installer.install_lock(self.destination.parent):
            with self.assertRaisesRegex(installer.InstallError, "already in progress"):
                installer.install(self.source, self.destination)
        self.settle.assert_not_called()
        self.copy.assert_not_called()

    def test_overlapping_paths_and_symlink_destination_are_rejected(self):
        with self.assertRaisesRegex(installer.InstallError, "must be separate"):
            installer.install(self.source, self.source)
        alias = self.root / "alias.app"
        alias.symlink_to(self.destination)
        with self.assertRaisesRegex(installer.InstallError, "not a symlink"):
            installer.bundle_path(str(alias))
        self.settle.assert_not_called()


class SignatureTests(unittest.TestCase):
    def test_signer_check_requires_apple_anchor_identifier_and_team(self):
        with tempfile.TemporaryDirectory() as directory:
            bundle = Path(directory) / "Samsung TV Remote.app"
            InstallationTests.write_bundle(bundle, "synthetic")
            for metadata, accepted in [
                ("Identifier=dev.samsungtvremote.local\nTeamIdentifier=ABCDEFGHIJ\n", True),
                ("Identifier=other\nTeamIdentifier=ABCDEFGHIJ\n", False),
                ("Identifier=dev.samsungtvremote.local\nTeamIdentifier=not set\n", False),
            ]:
                with self.subTest(metadata=metadata):
                    result = subprocess.CompletedProcess([], 0, "", metadata)
                    with patch.object(installer, "command") as command, \
                            patch.object(installer.subprocess, "run", return_value=result):
                        if accepted:
                            installer.verify_bundle(bundle)
                        else:
                            with self.assertRaises(installer.InstallError):
                                installer.verify_bundle(bundle)
                    self.assertIn("-R=anchor apple generic", command.call_args.args[0])


if __name__ == "__main__":
    unittest.main()
