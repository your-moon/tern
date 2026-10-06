#!/usr/bin/env bash
# Builds, bundles and zips tern for a GitHub release, fills the Homebrew cask and prints the
# `gh release create` command. It publishes nothing: no tag, no release, no push.
#
#   scripts/release.sh
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
[ -n "$version" ] || { echo "no version in Cargo.toml" >&2; exit 1; }
tag="v$version"

scripts/bundle-app.sh

out=target/release/bundle
zip="$out/tern-$version-macos-arm64.zip"
rm -f "$zip"
# ditto keeps the bundle's signature and resource forks intact, which plain zip does not.
ditto -c -k --keepParent "$out/tern.app" "$zip"
sha=$(shasum -a 256 "$zip" | awk '{print $1}')

cask="$out/tern.rb"
sed -e "s/@VERSION@/$version/" -e "s/@SHA256@/$sha/" packaging/homebrew/tern.rb > "$cask"

echo
echo "zip:    $zip"
echo "sha256: $sha"
echo "cask:   $cask (copy to the tap's Casks/tern.rb)"
echo
echo "Not run. When ready, with $tag tagged and pushed:"
echo
echo "  gh release create $tag \"$zip\" --repo your-moon/tern --title \"tern $version\" --notes-file <(awk '/^## \\[$version\\]/{f=1;next} /^## \\[/{f=0} f' CHANGELOG.md)"
