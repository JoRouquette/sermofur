#!/bin/sh
# Installs the Rust `smf` from a GitHub Release of Sermofur, on Linux x64 or macOS arm64.
#
#   curl -fsSL https://raw.githubusercontent.com/JoRouquette/sermofur/main/rust/dist/install.sh | sh
#   sh install.sh [vX.Y.Z]
#
# Without a tag (argument or SMF_VERSION), the latest Release is installed. The archive is checked against the
# SHA256SUMS of the same Release, then, when `gh` is installed, against its provenance attestation
# (signed by ci.yml on main, read from the Release and verified offline); any failure stops the
# install before anything is written. Without a usable `gh`, only the checksum is checked, and a warning says so.
# The version the binary reports must also equal the tag. The two SMF_*_ATTESTATION switches together are refused.
# Environment:
#   SMF_INSTALL_DIR          target folder (default: $HOME/.local/bin), created without sudo
#   SMF_RELEASES_URL         releases base (default: https://github.com/JoRouquette/sermofur/releases)
#   SMF_REQUIRE_ATTESTATION  set to 1 to refuse the install when gh is missing
#   SMF_SKIP_ATTESTATION     set to 1 to install on the checksum alone (offline gh, no login), with a warning
#   SMF_GH                   gh executable (default: gh)
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

# Provenance: the checksum comes from the same Release, so whoever can write to the Release can
# replace both. The attestation bundle is signed by ci.yml on main and cannot be forged for
# another archive; `gh` checks it against the archive just downloaded, with the bundle read from
# the Release (the signature roots are fetched by gh). An attestation binds bytes, not a version:
# the version the binary reports is compared with the tag below.
repo="JoRouquette/sermofur"
gh="${SMF_GH:-gh}"
if [ "${SMF_SKIP_ATTESTATION:-}" = 1 ] && [ "${SMF_REQUIRE_ATTESTATION:-}" = 1 ]; then
  fail "SMF_SKIP_ATTESTATION and SMF_REQUIRE_ATTESTATION contradict each other: nothing installed"
elif [ "${SMF_SKIP_ATTESTATION:-}" = 1 ]; then
  echo "install.sh: warning: attestation not checked (SMF_SKIP_ATTESTATION=1); only the checksum was verified." >&2
elif command -v "$gh" > /dev/null 2>&1 && "$gh" attestation verify --help > /dev/null 2>&1; then
  bundle="attestations-${tag#v}.jsonl"
  curl $secure -fsSL -o "$tmp/$bundle" "$releases/download/$tag/$bundle" ||
    fail "the Release $tag carries no attestation ($bundle): nothing installed"
  "$gh" attestation verify "$tmp/$archive" --repo "$repo" \
    --signer-workflow "$repo/.github/workflows/ci.yml" --source-ref refs/heads/main \
    --deny-self-hosted-runners --bundle "$tmp/$bundle" > "$tmp/gh.log" 2>&1 || {
    cat "$tmp/gh.log" >&2
    fail "attestation of $archive not verified: nothing installed"
  }
  echo "install.sh: attestation verified for $archive"
elif [ "${SMF_REQUIRE_ATTESTATION:-}" = 1 ]; then
  fail "a gh with the attestation command is required (SMF_REQUIRE_ATTESTATION=1): nothing installed"
else
  echo "install.sh: warning: gh not found or too old (no attestation command), so only the checksum of the same Release was verified; install a recent gh to verify the provenance." >&2
fi

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
# An older, genuinely attested release renamed as this one would pass the attestation.
[ "$printed" = "Sermofur ${tag#v}" ] || {
  rm -f "$dir/.smf.new"
  fail "the downloaded smf reports '$printed', not Sermofur ${tag#v}: nothing replaced"
}
mv -f "$dir/.smf.new" "$dir/smf"
echo "install.sh: installed $printed to $dir/smf"
case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "install.sh: $dir is not in PATH; add it to your shell profile." ;;
esac
}

main "$@"
