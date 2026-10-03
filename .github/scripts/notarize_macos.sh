#!/usr/bin/env bash
# Both hosted Mac architectures must pass; there is no unsigned fallback.
set -euo pipefail
umask 077
for name in APPLE_APPLICATION_CERTIFICATE_P12_BASE64 APPLE_CERTIFICATE_PASSWORD \
  APPLE_DEVELOPER_ID_APPLICATION APPLE_ID APPLE_APP_SPECIFIC_PASSWORD APPLE_TEAM_ID; do
  [ -n "${!name:-}" ] || { echo "Missing required signing secret: $name" >&2; exit 1; }
done
: "${STAGE:?}" "${KONTRA_TARGET:?}" "${GITHUB_SHA:?}"
case "$KONTRA_TARGET" in
  aarch64-apple-darwin) arch=arm64 ;;
  x86_64-apple-darwin) arch=x86_64 ;;
  *) echo 'Unsupported notarization target' >&2; exit 1 ;;
esac
source "$(dirname "$0")/macos_signing_keychain.sh"
for product in KONTRA.clap KONTRA.vst3 KONTRA.app; do
  binary="$STAGE/$product/Contents/MacOS/KONTRA"
  lipo "$binary" -verify_arch "$arch"
  codesign --force --sign "$APPLE_DEVELOPER_ID_APPLICATION" --keychain "$keychain" \
    --options runtime --timestamp "$STAGE/$product"
  chmod -R a+rX "$STAGE/$product"
  codesign --verify --deep --strict "$STAGE/$product"
done
xcrun swift .github/scripts/check_macos_bundles.swift --register "$STAGE/KONTRA.app" "$STAGE/KONTRA.clap" "$STAGE/KONTRA.vst3"
# ZIP containers cannot carry stapled tickets. A DMG holds
# these same signed products plus the legal/build metadata for offline delivery.
hdiutil create -quiet -format UDZO -volname KONTRA -srcfolder "$STAGE" "$work/KONTRA.dmg"
codesign --force --sign "$APPLE_DEVELOPER_ID_APPLICATION" --keychain "$keychain" --timestamp "$work/KONTRA.dmg"
codesign --verify --strict "$work/KONTRA.dmg"
xcrun notarytool submit "$work/KONTRA.dmg" --apple-id "$APPLE_ID" \
  --password "$APPLE_APP_SPECIFIC_PASSWORD" --team-id "$APPLE_TEAM_ID" \
  --wait --timeout 30m --output-format json > "$work/notary.json"
python3 - "$work/notary.json" <<'PY'
import json, sys, uuid
info = json.load(open(sys.argv[1]))
assert info['status'] == 'Accepted', 'Apple did not accept the notarization submission'
uuid.UUID(info['id'])
PY
xcrun stapler staple "$work/KONTRA.dmg"
xcrun stapler validate "$work/KONTRA.dmg"
hdiutil verify -quiet "$work/KONTRA.dmg"
spctl --assess --type open --context context:primary-signature "$work/KONTRA.dmg"
mv "$work/KONTRA.dmg" "$STAGE/KONTRA.dmg"
python3 - "$work/notary.json" <<'PY'
import hashlib, json, os, pathlib, sys
stage = pathlib.Path(os.environ['STAGE'])
info = json.load(open(sys.argv[1]))
identity = json.loads((stage / 'build-info.json').read_text())
assert identity['target'] == os.environ['KONTRA_TARGET'] and identity['revision'] == os.environ['GITHUB_SHA']
files = ['KONTRA.dmg', 'KONTRA.clap/Contents/MacOS/KONTRA', 'KONTRA.vst3/Contents/MacOS/KONTRA', 'KONTRA.app/Contents/MacOS/KONTRA']
receipt = dict(version=identity['version'], revision=identity['revision'], target=identity['target'],
    id=info['id'], status=info['status'], stapled=True, signatures_verified=True,
    sha256={name: hashlib.sha256((stage / name).read_bytes()).hexdigest() for name in files})
(stage / 'notarization.json').write_text(json.dumps(receipt, indent=2) + '\n')
PY
