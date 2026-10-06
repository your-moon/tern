#!/usr/bin/env bash
# Adapted from zeron scripts/package-macos.sh (notarize()) (MIT).
# Submits a zip/dmg to Apple's notary service and waits for the verdict. Zeron relies on the
# following `stapler staple` to fail when a rejection exits 0; this checks the verdict itself.
#
#   TERN_NOTARY_PROFILE=<keychain profile> scripts/notarize.sh <path>
set -euo pipefail
: "${TERN_NOTARY_PROFILE:?set TERN_NOTARY_PROFILE (xcrun notarytool store-credentials)}"
log=$(mktemp)
trap 'rm -f "$log"' EXIT
xcrun notarytool submit "$1" --keychain-profile "$TERN_NOTARY_PROFILE" --wait \
  --output-format json | tee "$log"
echo
status=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("status",""))' "$log")
if [ "$status" != "Accepted" ]; then
  id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("id",""))' "$log")
  echo "notarisation of $1 finished as \"$status\", not Accepted" >&2
  [ -z "$id" ] || xcrun notarytool log "$id" --keychain-profile "$TERN_NOTARY_PROFILE" >&2 || true
  exit 1
fi
