#!/usr/bin/env bash
# Builds the tool package at a given version into artifacts/release. Called only by the
# `package` job of ci.yml, which copies it from main so that older tags are built the same way.
# semantic-release must not call it (ADR 0011). The caller runs `dotnet restore --locked-mode`
# first.
set -euo pipefail

version="${1:?usage: pack.sh <version>}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "pack.sh: expected a release version such as 1.2.3, got '$version'" >&2
  exit 1
fi

dotnet pack src/Sermofur.Cli -c Release --no-restore -p:Version="$version" -o artifacts/release
