#!/usr/bin/env bash
# Fail if a family vault, legacy data file, or env file is tracked.
set -euo pipefail

cd "$(dirname "$0")/.."

fail=0

while IFS= read -r path; do
  case "$path" in
    vault.cofferly | */vault.cofferly | data.json | */data.json | .env | .env.* | */.env | */.env.*)
      echo "::error::Forbidden path is tracked: $path"
      fail=1
      ;;
  esac
done < <(git ls-files)

for p in vault.cofferly data.json .env; do
  if ! git check-ignore -q "$p"; then
    echo "::error::$p is not gitignored"
    fail=1
  fi
done

if [[ "$fail" -ne 0 ]]; then
  exit 1
fi

echo "vault-guard OK"
