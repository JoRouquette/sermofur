"""Tests rust/dist/install.sh (Linux, macOS) or install.ps1 (Windows) against a local server.

    python test-install.py <dist-dir> <version>     archives built by package-rust.sh
    python test-install.py --fake <version>         Linux and macOS: a fake archive

The server mimics GitHub Releases: /latest redirects to /tag/v<version>, and
/download/v<version>/<file> serves the archives, a SHA256SUMS and an attestation bundle computed
here. Scenarios: install by tag and update in place to the latest Release, a wrong checksum and a
missing line (both must fail and install nothing), then the attestation: a valid one, one for another
archive, a missing one, no gh or a gh without the attestation command (warning, or refusal with
SMF_REQUIRE_ATTESTATION=1), SMF_SKIP_ATTESTATION=1, both switches together, and (Linux, macOS) an
attested older release renamed as the requested one, refused on the version the binary reports.
`gh` is a stand-in (`python test-install.py --gh ...`): it accepts an
archive only if its sha256 is listed in the bundle, which exercises the installers' control flow;
the real `gh attestation verify` is proven by the first real release. Standard library only; used
by the `install-rust` job of ci.yml.
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


def fake_gh(args):
    """Stand-in for `gh attestation verify <file> ... --bundle <bundle>`."""
    if args[:2] != ["attestation", "verify"]:
        print("fake gh: unexpected command", args, file=sys.stderr)
        return 2
    if args[2:] == ["--help"]:  # the installers probe for the command first
        print("Usage: gh attestation verify")
        return 0
    archive, bundle = pathlib.Path(args[2]), pathlib.Path(args[args.index("--bundle") + 1])
    # The values the installers must pass, copied from the `existing` step of package-rust-release.
    expected = {
        "--repo": "JoRouquette/sermofur",
        "--signer-workflow": "JoRouquette/sermofur/.github/workflows/ci.yml",
        "--source-ref": "refs/heads/main",
    }
    for flag, value in expected.items():
        if flag not in args or args[args.index(flag) + 1] != value:
            print(f"fake gh: {flag} is missing or not {value}", file=sys.stderr)
            return 2
    if "--deny-self-hosted-runners" not in args:
        print("fake gh: --deny-self-hosted-runners is missing", file=sys.stderr)
        return 2
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if digest in bundle.read_text().split():
        print(f"Loaded digest sha256:{digest}\nVerification succeeded!")  # ASCII: cp1252 pipes on Windows
        return 0
    print(f"fake gh: no attestation for sha256:{digest}", file=sys.stderr)
    return 1


def fake_gh_command(folder):
    """A `gh` the installers can run: a .cmd on Windows, a shell script elsewhere."""
    python, script = sys.executable, pathlib.Path(__file__).resolve()
    if WINDOWS:
        path = folder / "gh.cmd"
        path.write_text(f'@echo off\r\n"{python}" "{script}" --gh %*\r\nexit /b %ERRORLEVEL%\r\n')
    else:
        path = folder / "gh"
        path.write_text(f'#!/bin/sh\nexec "{python}" "{script}" --gh "$@"\n')
        path.chmod(0o755)
    return path


def fake_archive(dist, version, reported=None):
    """An archive named for `version` whose binary prints `reported` (default: the same version)."""
    target = "macos-arm64" if sys.platform == "darwin" else "linux-x64"
    script = f'#!/bin/sh\necho "Sermofur {reported or version}"\n'.encode()
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
            elif self.path == f"/download/{tag}/attestations-{tag[1:]}.jsonl":
                data = None if state["bundle"] is None else state["bundle"].encode()
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


def install(url, folder, tag, gh, extra=None):
    env = dict(os.environ, SMF_RELEASES_URL=url, SMF_INSTALL_DIR=str(folder), SMF_NO_MODIFY_PATH="1",
               SMF_GH=str(gh))
    for name in ("SMF_VERSION", "SMF_REQUIRE_ATTESTATION", "SMF_SKIP_ATTESTATION"):
        env.pop(name, None)
    env.update(extra or {})
    if WINDOWS:
        command = ["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
                   str(ROOT / "rust" / "dist" / "install.ps1")] + (["-Tag", tag] if tag else [])
    else:
        command = ["sh", str(ROOT / "rust" / "dist" / "install.sh")] + ([tag] if tag else [])
    run = subprocess.run(command, env=env, capture_output=True, text=True)
    print(run.stdout + run.stderr)
    return run.returncode, run.stdout + run.stderr


def main():
    if sys.argv[1] == "--gh":
        sys.exit(fake_gh(sys.argv[2:]))
    if sys.argv[1] == "--fake":
        version, dist = sys.argv[2], pathlib.Path(tempfile.mkdtemp())
        fake_archive(dist, version)
    else:
        dist, version = pathlib.Path(sys.argv[1]), sys.argv[2]
    tag = f"v{version}"
    archives = sorted(dist.glob("smf-*"))
    good = "".join(f"{hashlib.sha256(f.read_bytes()).hexdigest()}  {f.name}\n" for f in archives)
    attested = "".join(f"{hashlib.sha256(f.read_bytes()).hexdigest()}\n" for f in archives)
    state = {"sums": good, "bundle": attested}
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
        gh = fake_gh_command(scratch)
        no_gh = scratch / "no-such-gh"
        # The second install updates the first one in place: the replace path of an upgrade.
        # Both run with a valid attestation, so the whole chain is exercised.
        for name, given in (("install by tag", version), ("update to the latest Release", "")):
            folder = scratch / "installed"
            code, output = install(url, folder, given, gh)
            printed = ""
            if code == 0 and (folder / exe).is_file():
                printed = subprocess.run([str(folder / exe), "--version"], capture_output=True,
                                         text=True).stdout.strip()
            check(name, code == 0 and printed == f"Sermofur {version}" and "attestation verified" in output)

        # Refusals must install nothing. Each case: name, sums, bundle, gh, extra env, reason.
        wrong = "".join(f"{'0' * 64}  {line.split()[1]}\n" for line in good.splitlines())
        other = "".join(f"{'1' * 64}\n" for _ in archives)
        refused = (
            ("wrong checksum refused", wrong, attested, no_gh, {}, "checksum mismatch"),
            ("missing line refused", "", attested, no_gh, {}, "SHA256SUMS has no line"),
            ("attestation of another archive refused", good, other, gh, {}, "not verified"),
            ("missing attestation refused", good, None, gh, {}, "carries no attestation"),
            ("no gh refused when required", good, attested, no_gh,
             {"SMF_REQUIRE_ATTESTATION": "1"}, "attestation command is required"),
        )
        for name, sums, bundle, gh_used, extra, reason in refused:
            state["sums"], state["bundle"] = sums, bundle
            folder = scratch / name.replace(" ", "-")
            code, output = install(url, folder, tag, gh_used, extra)
            check(name, code != 0 and reason in output and not (folder / exe).exists())

        # Accepted on the checksum alone, with a warning: no gh, or an explicit skip.
        for name, bundle, gh_used, extra, reason in (
            ("no gh installs on the checksum with a warning", attested, no_gh, {}, "only the checksum"),
            ("skipped attestation installs with a warning", other, gh, {"SMF_SKIP_ATTESTATION": "1"},
             "attestation not checked"),
        ):
            state["sums"], state["bundle"] = good, bundle
            folder = scratch / name.replace(" ", "-")
            code, output = install(url, folder, tag, gh_used, extra)
            check(name, code == 0 and reason in output and (folder / exe).is_file())

        # Contradictory switches are refused before anything is written.
        state["sums"], state["bundle"] = good, attested
        folder = scratch / "both-switches"
        code, output = install(url, folder, tag, gh,
                               {"SMF_SKIP_ATTESTATION": "1", "SMF_REQUIRE_ATTESTATION": "1"})
        check("skip and require together refused",
              code != 0 and "contradict" in output and not (folder / exe).exists())

        # A gh without the attestation command counts as absent: warning, or refusal if required.
        old_gh = scratch / "old-gh-bin"
        old_gh.mkdir()
        if WINDOWS:
            (old_gh / "gh.cmd").write_text("@echo off\r\nexit /b 1\r\n")
        else:
            (old_gh / "gh").write_text("#!/bin/sh\nexit 1\n")
            (old_gh / "gh").chmod(0o755)
        old_gh = old_gh / ("gh.cmd" if WINDOWS else "gh")
        for name, extra, ok, reason in (
            ("old gh installs on the checksum with a warning", {}, True, "too old"),
            ("old gh refused when required", {"SMF_REQUIRE_ATTESTATION": "1"}, False, "attestation command"),
        ):
            folder = scratch / name.replace(" ", "-")
            code, output = install(url, folder, tag, old_gh, extra)
            check(name, (code == 0) == ok and reason in output and (folder / exe).exists() == ok)

        # An attested older release renamed as this one: the attestation passes (it binds bytes,
        # not names), the version the binary reports must not. Needs a script as binary: not Windows.
        if not WINDOWS:
            replay = pathlib.Path(tempfile.mkdtemp())
            fake_archive(replay, version, reported="0.0.0-old")
            files = sorted(replay.glob("smf-*"))
            sums = "".join(f"{hashlib.sha256(f.read_bytes()).hexdigest()}  {f.name}\n" for f in files)
            bundle = "".join(f"{hashlib.sha256(f.read_bytes()).hexdigest()}\n" for f in files)
            replay_server = serve(replay, tag, {"sums": sums, "bundle": bundle})
            folder = scratch / "version-replay"
            code, output = install(f"http://127.0.0.1:{replay_server.server_address[1]}", folder, tag, gh)
            check("version replay refused", code != 0 and "reports" in output and not (folder / exe).exists())
            replay_server.shutdown()
    server.shutdown()
    print("INSTALL TESTS", "FAILED: " + ", ".join(failures) if failures else "passed")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
