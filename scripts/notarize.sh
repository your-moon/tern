#!/usr/bin/env bash
# Adapted from zeron scripts/package-macos.sh (notarize()) (MIT).
# Submits a zip/dmg to Apple's notary service and waits for the verdict. Zeron relies on the
# following `stapler staple` to fail when a rejection exits 0; this checks the verdict itself.
#
#   TERN_NOTARY_PROFILE=<keychain profile> scripts/notarize.sh <path>
#   TERN_NOTARY_KEY=<AuthKey_ID.p8> TERN_NOTARY_KEY_ID=<id> TERN_NOTARY_ISSUER=<uuid> scripts/notarize.sh <path>
# The second form uses an App Store Connect API key file directly, with no Keychain item.
set -euo pipefail
if [ -n "${TERN_NOTARY_KEY:-}" ]; then
  auth=(--key "$TERN_NOTARY_KEY" --key-id "${TERN_NOTARY_KEY_ID:?set TERN_NOTARY_KEY_ID}" \
    --issuer "${TERN_NOTARY_ISSUER:?set TERN_NOTARY_ISSUER}")
else
  : "${TERN_NOTARY_PROFILE:?set TERN_NOTARY_PROFILE (xcrun notarytool store-credentials) or TERN_NOTARY_KEY}"
  auth=(--keychain-profile "$TERN_NOTARY_PROFILE")
fi
log=$(mktemp)
trap 'rm -f "$log"' EXIT
xcrun notarytool submit "$1" "${auth[@]}" --wait \
  --output-format json | tee "$log"
echo
status=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("status",""))' "$log")
if [ "$status" != "Accepted" ]; then
  id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("id",""))' "$log")
  echo "notarisation of $1 finished as \"$status\", not Accepted" >&2
  [ -z "$id" ] || xcrun notarytool log "$id" "${auth[@]}" >&2 || true
  exit 1
fi
