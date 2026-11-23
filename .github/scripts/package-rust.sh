#!/usr/bin/env bash
# Builds the Rust `smf` at a given version for one target and archives it into <out-dir> as
# smf-<version>-<target>.tar.gz (Linux, macOS) or .zip (Windows), with LICENSE and NOTICE.
# Called by the `package-rust`, `package-rust-dry-run` and `install-rust` jobs of ci.yml, from
# the repository root, on a runner of the target system (native build, no cross-compilation).
# The version reaches the binary through SMF_BUILD_VERSION; Cargo.toml is not modified.
set -euo pipefail

version="${1:?usage: package-rust.sh <version> <linux-x64|windows-x64|macos-arm64> <out-dir>}"
target="${2:?usage: package-rust.sh <version> <target> <out-dir>}"
out="${3:?usage: package-rust.sh <version> <target> <out-dir>}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "package-rust.sh: expected a release version such as 1.2.3, got '$version'" >&2
  exit 1
fi
case "$target" in
  linux-x64) triple=x86_64-unknown-linux-gnu exe=smf ;;
  windows-x64) triple=x86_64-pc-windows-msvc exe=smf.exe ;;
  macos-arm64) triple=aarch64-apple-darwin exe=smf ;;
  *)
    echo "package-rust.sh: unknown target '$target'" >&2
    exit 1
    ;;
esac

SMF_BUILD_VERSION="$version" cargo build --manifest-path rust/Cargo.toml --release --locked \
  -p smf --target "$triple"
binary="rust/target/$triple/release/$exe"

# The shipped bytes must name the version of the tag (guard of lot 10).
printed="$("$binary" --version | tr -d '\r')"
if [[ "$printed" != "Sermofur $version" ]]; then
  echo "package-rust.sh: the binary prints '$printed', expected 'Sermofur $version'" >&2
  exit 1
fi

name="smf-$version-$target"
stage="$(mktemp -d)"
cp "$binary" LICENSE NOTICE "$stage/"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
if [[ "$target" == windows-x64 ]]; then
  # Relative paths only: Windows PowerShell does not read the paths of Git Bash.
  (cd "$stage" && powershell.exe -NoProfile -NonInteractive -Command \
    "\$ErrorActionPreference = 'Stop'; Compress-Archive -Path smf.exe, LICENSE, NOTICE -DestinationPath archive.zip")
  mv "$stage/archive.zip" "$out/$name.zip"
else
  tar -czf "$out/$name.tar.gz" -C "$stage" "$exe" LICENSE NOTICE
fi
rm -rf "$stage"
echo "package-rust.sh: $(ls "$out" | grep -F "$name")"
