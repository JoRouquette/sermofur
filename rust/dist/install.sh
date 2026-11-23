#!/bin/sh
# Installs the Rust `smf` from a GitHub Release of Sermofur, on Linux x64 or macOS arm64.
#
#   curl -fsSL https://raw.githubusercontent.com/JoRouquette/sermofur/main/rust/dist/install.sh | sh
#   sh install.sh [vX.Y.Z]
#
# Without a tag (argument or SMF_VERSION), the latest Release is installed. The archive is checked against the
# SHA256SUMS of the same Release; any mismatch stops the install before anything is written.
# Environment:
#   SMF_INSTALL_DIR   target folder (default: $HOME/.local/bin), created without sudo
#   SMF_RELEASES_URL  releases base (default: https://github.com/JoRouquette/sermofur/releases)
set -eu

fail() {
  echo "install.sh: $*" >&2
  exit 1
}

# The whole body is one function, called on the last line: a download cut short under
# `curl | sh` runs nothing.
main() {
releases="${SMF_RELEASES_URL:-https://github.com/JoRouquette/sermofur/releases}"
dir="${SMF_INSTALL_DIR:-$HOME/.local/bin}"
tag="${1:-${SMF_VERSION:-}}"
# HTTPS stays HTTPS through redirects; a plain http:// base (local tests) is used as given.
case "$releases" in
  https://*) secure="--proto =https --tlsv1.2" ;;
  *) secure="" ;;
esac

command -v curl > /dev/null 2>&1 || fail "curl is required"
command -v sha256sum > /dev/null 2>&1 || command -v shasum > /dev/null 2>&1 ||
  fail "sha256sum or shasum is required"

case "$(uname -s)/$(uname -m)" in
  Linux/x86_64 | Linux/amd64) target=linux-x64 ;;
  Darwin/arm64 | Darwin/aarch64) target=macos-arm64 ;;
  *) fail "no smf build for $(uname -s) $(uname -m): Linux x64, macOS arm64 and Windows x64 only" ;;
esac

if [ -z "$tag" ]; then
  # /releases/latest redirects to /releases/tag/<tag>: no API call, no rate limit.
  url="$(curl $secure -fsSLI -o /dev/null -w '%{url_effective}' "$releases/latest")" ||
    fail "cannot reach $releases/latest"
  tag="${url##*/}"
fi
case "$tag" in v*) ;; *) tag="v$tag" ;; esac
echo "$tag" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' || fail "no release tag found (got '$tag')"

archive="smf-${tag#v}-$target.tar.gz"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

echo "install.sh: downloading $archive ($tag)"
curl $secure -fsSL -o "$tmp/$archive" "$releases/download/$tag/$archive" ||
  fail "cannot download $archive: the Release $tag carries no smf archive for $target (yet?)"
curl $secure -fsSL -o "$tmp/SHA256SUMS" "$releases/download/$tag/SHA256SUMS" || fail "cannot download SHA256SUMS"

expected="$(awk -v f="$archive" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS")"
[ -n "$expected" ] || fail "SHA256SUMS has no line for $archive"
if command -v sha256sum > /dev/null 2>&1; then
  actual="$(sha256sum "$tmp/$archive" | awk '{ print $1 }')"
else
  actual="$(shasum -a 256 "$tmp/$archive" | awk '{ print $1 }')"
fi
[ "$(echo "$expected" | tr 'A-F' 'a-f')" = "$actual" ] ||
  fail "checksum mismatch for $archive: nothing installed"

tar -xzf "$tmp/$archive" -C "$tmp" smf
mkdir -p "$dir"
# Copy then rename in the same folder: a running smf keeps its old file.
cp "$tmp/smf" "$dir/.smf.new"
chmod 755 "$dir/.smf.new"
# Start the new binary before it replaces a working one (glibc too old, for example).
printed="$("$dir/.smf.new" --version)" || {
  rm -f "$dir/.smf.new"
  fail "the downloaded smf does not run on this system: nothing replaced"
}
mv -f "$dir/.smf.new" "$dir/smf"
echo "install.sh: installed $printed to $dir/smf"
case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "install.sh: $dir is not in PATH; add it to your shell profile." ;;
esac
}

main "$@"
