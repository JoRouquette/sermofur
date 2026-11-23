"""Tests rust/dist/install.sh (Linux, macOS) or install.ps1 (Windows) against a local server.

    python test-install.py <dist-dir> <version>     archives built by package-rust.sh
    python test-install.py --fake <version>         Linux and macOS: a fake archive

The server mimics GitHub Releases: /latest redirects to /tag/v<version>, and
/download/v<version>/<file> serves the archives and a SHA256SUMS computed here. Scenarios:
install by tag, update in place to the latest Release, then a wrong checksum and a missing line, which
must fail and install nothing. Standard library only; used by the `install-rust` job of ci.yml.
"""

import hashlib
import http.server
import io
import os
import pathlib
import subprocess
import sys
import tarfile
import tempfile
import threading

ROOT = pathlib.Path(__file__).resolve().parents[2]
WINDOWS = os.name == "nt"


def fake_archive(dist, version):
    target = "macos-arm64" if sys.platform == "darwin" else "linux-x64"
    script = f'#!/bin/sh\necho "Sermofur {version}"\n'.encode()
    with tarfile.open(dist / f"smf-{version}-{target}.tar.gz", "w:gz") as tar:
        info = tarfile.TarInfo("smf")
        info.size, info.mode = len(script), 0o755
        tar.addfile(info, io.BytesIO(script))


def serve(dist, tag, state):
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_HEAD(self):
            self.answer(body=False)

        def do_GET(self):
            self.answer(body=True)

        def answer(self, body):
            if self.path == "/latest":
                self.send_response(302)
                self.send_header("Location", f"/tag/{tag}")
                self.end_headers()
                return
            data = None
            if self.path == f"/tag/{tag}":
                data = b"release page"
            elif self.path == f"/download/{tag}/SHA256SUMS":
                data = state["sums"].encode()
            elif self.path.startswith(f"/download/{tag}/"):
                file = dist / self.path.rsplit("/", 1)[1]
                data = file.read_bytes() if file.is_file() else None
            if data is None:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            if body:
                self.wfile.write(data)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def install(url, folder, tag):
    env = dict(os.environ, SMF_RELEASES_URL=url, SMF_INSTALL_DIR=str(folder), SMF_NO_MODIFY_PATH="1")
    env.pop("SMF_VERSION", None)
    if WINDOWS:
        command = ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
                   str(ROOT / "rust" / "dist" / "install.ps1")] + (["-Tag", tag] if tag else [])
    else:
        command = ["sh", str(ROOT / "rust" / "dist" / "install.sh")] + ([tag] if tag else [])
    run = subprocess.run(command, env=env, capture_output=True, text=True)
    print(run.stdout + run.stderr)
    return run.returncode, run.stdout + run.stderr


def main():
    if sys.argv[1] == "--fake":
        version, dist = sys.argv[2], pathlib.Path(tempfile.mkdtemp())
        fake_archive(dist, version)
    else:
        dist, version = pathlib.Path(sys.argv[1]), sys.argv[2]
    tag = f"v{version}"
    good = "".join(
        f"{hashlib.sha256(f.read_bytes()).hexdigest()}  {f.name}\n" for f in sorted(dist.glob("smf-*"))
    )
    state = {"sums": good}
    server = serve(dist, tag, state)
    url = f"http://127.0.0.1:{server.server_address[1]}"
    exe = "smf.exe" if WINDOWS else "smf"
    failures = []

    def check(name, ok):
        print(f"{'ok  ' if ok else 'FAIL'} {name}")
        if not ok:
            failures.append(name)

    with tempfile.TemporaryDirectory() as scratch:
        scratch = pathlib.Path(scratch)
        # The second install updates the first one in place: the replace path of an upgrade.
        for name, given in (("install by tag", version), ("update to the latest Release", "")):
            folder = scratch / "installed"
            code, _ = install(url, folder, given)
            printed = ""
            if code == 0 and (folder / exe).is_file():
                printed = subprocess.run([str(folder / exe), "--version"], capture_output=True,
                                         text=True).stdout.strip()
            check(name, code == 0 and printed == f"Sermofur {version}")

        wrong = "".join(f"{'0' * 64}  {line.split()[1]}\n" for line in good.splitlines())
        for name, sums, reason in (
            ("wrong checksum refused", wrong, "checksum mismatch"),
            ("missing line refused", "", "SHA256SUMS has no line"),
        ):
            state["sums"] = sums
            folder = scratch / name.replace(" ", "-")
            code, output = install(url, folder, tag)
            check(name, code != 0 and reason in output and not (folder / exe).exists())
    server.shutdown()
    print("INSTALL TESTS", "FAILED: " + ", ".join(failures) if failures else "passed")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
