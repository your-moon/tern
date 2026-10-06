#!/usr/bin/env bash
# Creates a self-signed code-signing identity "tern local" in the login keychain, once.
# bundle-app.sh signs with it, so every build of tern has the same designated requirement and
# macOS keeps the Keychain's "Always Allow" across updates. It is not trusted beyond this Mac
# and changes nothing about Developer ID signing or notarisation.
set -euo pipefail
if security find-identity -p codesigning 2>/dev/null | grep -q '"tern local"'; then
  echo "\"tern local\" already exists"; exit 0
fi
dir=$(mktemp -d); chmod 700 "$dir"; trap 'rm -rf "$dir"' EXIT
openssl req -x509 -newkey rsa:2048 -keyout "$dir/key.pem" -out "$dir/cert.pem" -days 3650 -nodes \
  -subj "/CN=tern local" -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning" 2>/dev/null
pass=$(openssl rand -hex 16)
openssl pkcs12 -export -legacy -inkey "$dir/key.pem" -in "$dir/cert.pem" -name "tern local" \
  -out "$dir/id.p12" -passout "pass:$pass" 2>/dev/null
security import "$dir/id.p12" -k "$HOME/Library/Keychains/login.keychain-db" -P "$pass" -T /usr/bin/codesign >/dev/null
echo "created \"tern local\""
