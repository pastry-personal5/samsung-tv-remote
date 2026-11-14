#!/usr/bin/env python3
"""Replace a macOS bundle only after its own processes have settled.

No profile, API or TV credentials are opened. SIGTERM also works when a live
server's signature was invalidated by an earlier in-place installation.
"""
import argparse
from contextlib import contextmanager, ExitStack
from dataclasses import dataclass
import fcntl
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time

BINARY = "samsung-tv-remote"
IDENTIFIER = "dev.samsungtvremote.local"


class InstallError(Exception):
    pass


@dataclass(frozen=True)
class Process:
    uid: int
    pid: int
    started: str
    executable: str


def command(arguments):
    try:
        return subprocess.run(
            arguments, check=True, capture_output=True, text=True,
            env={**os.environ, "LC_ALL": "C"},
        ).stdout
    except subprocess.CalledProcessError as error:
        raise InstallError(f"{arguments[0]} failed: {error.stderr.strip()}") from error


def bundle_path(value):
    path = Path(os.path.abspath(os.path.expanduser(value)))
    if path.suffix != ".app" or path.is_symlink():
        raise InstallError(f"Expected an application directory, not a symlink: {path}")
    return path.parent.resolve() / path.name


def parse_processes(output):
    processes = []
    for line in output.splitlines():
        if not line.strip():
            continue
        # lstart has five fields in the C locale; comm retains path spaces.
        fields = line.split(None, 7)
        if len(fields) != 8 or not fields[0].isdigit() or not fields[1].isdigit():
            raise InstallError("Could not safely read the process list; bundle preserved.")
        processes.append(Process(int(fields[0]), int(fields[1]),
                                 " ".join(fields[2:7]), fields[7]))
    return processes


def bundle_processes(bundle):
    expected = str(bundle / "Contents/MacOS" / BINARY)
    output = command(["ps", "-ww", "-axo", "uid=,pid=,lstart=,comm="])
    return [process for process in parse_processes(output)
            if process.executable == expected]


def ensure_idle(bundle):
    processes = bundle_processes(bundle)
    if processes:
        pids = ", ".join(str(process.pid) for process in processes)
        raise InstallError(
            f"Bundle is still running (PID {pids}): {bundle}. "
            "Close its GUI and settle its server before replacing it, "
            "or use make bundle-candidate to preserve the running build."
        )


def settle(bundle, timeout):
    deadline = time.monotonic() + timeout
    signalled = set()
    while True:
        processes = bundle_processes(bundle)
        if not processes:
            return
        if any(process.uid != os.getuid() for process in processes):
            raise InstallError("Another user is running this bundle; installation aborted.")
        for process in processes:
            if process in signalled:
                continue
            # Recheck path, owner and start time before signalling a reused PID.
            if process not in bundle_processes(bundle):
                continue
            print(f"Waiting for PID {process.pid} to exit gracefully…", flush=True)
            try:
                os.kill(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            signalled.add(process)
        if time.monotonic() >= deadline:
            raise InstallError(
                "Timed out waiting for app/server settlement; installed bundle preserved. "
                "No process was force-killed."
            )
        time.sleep(0.1)


def verify_bundle(bundle):
    executable = bundle / "Contents/MacOS" / BINARY
    if not executable.is_file() or executable.is_symlink():
        raise InstallError(f"Missing regular executable: {executable}")
    command(["/usr/bin/codesign", "--verify", "--strict",
             "-R=anchor apple generic", str(bundle)])
    # codesign writes signing metadata to stderr, even on success.
    result = subprocess.run(
        ["/usr/bin/codesign", "--display", "--verbose=4", str(bundle)],
        capture_output=True, text=True, check=False,
    )
    metadata = result.stdout + result.stderr
    if (result.returncode != 0
            or f"Identifier={IDENTIFIER}" not in metadata.splitlines()
            or not re.search(r"^TeamIdentifier=[A-Za-z0-9]{10}$", metadata, re.MULTILINE)):
        raise InstallError("Expected the Apple-signed Samsung TV Remote application.")


@contextmanager
def install_lock(parent):
    lock_path = parent / ".samsung-tv-remote.install.lock"
    descriptor = os.open(lock_path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        metadata = os.fstat(descriptor)
        if (not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != os.getuid()
                or metadata.st_nlink != 1):
            raise InstallError("Unsafe installation lock.")
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise InstallError("Another installation is already in progress.") from error
        yield
    finally:
        os.close(descriptor)


@contextmanager
def publication_lock(bundle):
    """Lock the executable inode, so renaming a bundle never drops the fence."""
    executable = bundle / "Contents/MacOS" / BINARY
    try:
        descriptor = os.open(executable, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    except FileNotFoundError:
        # A damaged bundle without an executable has no entrypoint to fence
        # and must stay replaceable by a verified candidate.
        yield
        return
    try:
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            raise InstallError("Unsafe publication executable.")
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise InstallError("App/server relaunched during settlement; installation aborted.") from error
        yield
    finally:
        os.close(descriptor)


def install(source, destination, timeout=30.0):
    if (source == destination or source in destination.parents
            or destination in source.parents):
        raise InstallError("Source and destination bundles must be separate.")
    verify_bundle(source)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with install_lock(destination.parent):
        stage = Path(tempfile.mkdtemp(prefix=".samsung-tv-remote.install-",
                                      suffix=".noindex", dir=destination.parent))
        staged = stage / destination.name
        backup = None
        keep_stage = False
        published = False
        publication = ExitStack()
        try:
            # Copy and verify before interrupting any running app or server.
            command(["/usr/bin/ditto", "--noacl", str(source), str(staged)])
            verify_bundle(staged)
            # New entrypoints take a shared inode lock for their entire life.
            # Fence the candidate before publication and the old inode after
            # settlement. A launch in between causes a safe abort. Keep both
            # locks through verification/rollback, including bundle renames.
            publication.enter_context(publication_lock(staged))
            settle(destination, timeout)
            if destination.exists():
                if not destination.is_dir() or destination.is_symlink():
                    raise InstallError("Destination is not an application directory.")
                publication.enter_context(publication_lock(destination))
            ensure_idle(destination)
            if destination.exists():
                previous = Path(tempfile.mkdtemp(prefix=".samsung-tv-remote.previous-",
                                                 suffix=".noindex", dir=destination.parent))
                backup = previous / destination.name
                destination.rename(backup)
            try:
                # Also inspect legacy processes after removing their launch path.
                # Pre-publication builds do not participate in executable locks.
                # An observed late launch restores the unchanged old inode.
                ensure_idle(destination)
                if backup is not None:
                    ensure_idle(backup)
                staged.rename(destination)
                published = True
                verify_bundle(destination)
            except Exception as error:
                try:
                    if published:
                        ensure_idle(destination)
                        destination.rename(staged)
                    if backup is not None:
                        backup.rename(destination)
                        backup.parent.rmdir()
                except Exception as rollback_error:
                    keep_stage = True
                    raise InstallError(
                        f"Installation failed; rollback needs attention. "
                        f"Previous bundle: {backup}. Staged files: {stage}. "
                        f"{rollback_error}"
                    ) from error
                recovery = ("previous bundle restored" if backup is not None
                            else "no application installed")
                raise InstallError(f"Installation failed; {recovery}. {error}") from error
            print(f"Installed and signature verified: {destination}")
            if backup is not None:
                print(f"Previous bundle retained: {backup}")
            print("Launch with make run-mac. Existing server Stop state and credentials are preserved.")
        finally:
            publication.close()
            if not keep_stage:
                shutil.rmtree(stage)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action", required=True)
    check = actions.add_parser("check-idle", help="refuse to replace a running build bundle")
    check.add_argument("bundle")
    update = actions.add_parser("install", help="settle the installed app/server, then replace the bundle")
    update.add_argument("source")
    update.add_argument("destination")
    update.add_argument("--timeout", type=float, default=30.0)
    arguments = parser.parse_args()
    try:
        if sys.platform != "darwin":
            raise InstallError("macOS is required.")
        if arguments.action == "check-idle":
            ensure_idle(bundle_path(arguments.bundle))
        else:
            if not 0 < arguments.timeout <= 300:
                raise InstallError("Settlement timeout must be between 0 and 300 seconds.")
            install(bundle_path(arguments.source), bundle_path(arguments.destination),
                    arguments.timeout)
    except (InstallError, OSError) as error:
        print(f"Install error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
