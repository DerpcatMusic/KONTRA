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
# Labels identify failures without echoing command arguments or credentials.
phase() {
  local label="$1" status
  shift
  echo "Starting $label" >&2
  if "$@"; then
    echo "Completed $label" >&2
  else
    status=$?
    echo "$label failed (exit $status)" >&2
    return "$status"
  fi
}
for product in KONTRA2.clap KONTRA2.vst3 KONTRA2.app; do
  binary="$STAGE/$product/Contents/MacOS/KONTRA2"
  lipo "$binary" -verify_arch "$arch"
  codesign --force --sign "$APPLE_DEVELOPER_ID_APPLICATION" --keychain "$keychain" \
    --options runtime --timestamp "$STAGE/$product"
  chmod -R a+rX "$STAGE/$product"
  codesign --verify --deep --strict "$STAGE/$product"
done
phase "Native bundle/factory verification" xcrun swift .github/scripts/check_macos_bundles.swift --register "$STAGE/KONTRA2.app" "$STAGE/KONTRA2.clap" "$STAGE/KONTRA2.vst3"
# ZIP containers cannot carry stapled tickets. A DMG holds
# these same signed products plus the legal/build metadata for offline delivery.
phase "DMG creation" hdiutil create -format UDZO -volname KONTRA -srcfolder "$STAGE" "$work/KONTRA2.dmg"
phase "DMG signing" codesign --force --sign "$APPLE_DEVELOPER_ID_APPLICATION" --keychain "$keychain" --timestamp "$work/KONTRA2.dmg"
phase "DMG signature verification" codesign --verify --strict "$work/KONTRA2.dmg"
if phase "Apple notarization submission" xcrun notarytool submit "$work/KONTRA2.dmg" --apple-id "$APPLE_ID" \
  --password "$APPLE_APP_SPECIFIC_PASSWORD" --team-id "$APPLE_TEAM_ID" \
  --wait --timeout 30m --output-format json > "$work/notary.json"; then
  :
else
  status=$?
  # Error JSON is otherwise lost during private-keychain cleanup. Only these
  # reviewed result fields may be logged; never print arbitrary server messages.
  python3 - "$work/notary.json" <<'PYERROR'
import json, sys, uuid
try:
    info = json.load(open(sys.argv[1]))
    safe = {}
    if info.get("status") in ("Accepted", "Invalid", "Rejected", "In Progress", "Uploaded"):
        safe["status"] = info["status"]
    try:
        safe["id"] = str(uuid.UUID(info["id"]))
    except (KeyError, ValueError, TypeError, AttributeError):
        pass
    if type(info.get("statusCode")) is int:
        safe["statusCode"] = info["statusCode"]
    print("Apple submission failure result: " + json.dumps(safe), file=sys.stderr)
except (OSError, ValueError, TypeError, AttributeError):
    print("Apple submission failed without a parseable result", file=sys.stderr)
PYERROR
  exit "$status"
fi
python3 - "$work/notary.json" <<'PY'
import json, sys, uuid
info = json.load(open(sys.argv[1]))
assert info['status'] == 'Accepted', 'Apple did not accept the notarization submission'
uuid.UUID(info['id'])
PY
xcrun stapler staple "$work/KONTRA2.dmg"
xcrun stapler validate "$work/KONTRA2.dmg"
hdiutil verify -quiet "$work/KONTRA2.dmg"
spctl --assess --type open --context context:primary-signature "$work/KONTRA2.dmg"
mv "$work/KONTRA2.dmg" "$STAGE/KONTRA2.dmg"
python3 - "$work/notary.json" <<'PY'
import hashlib, json, os, pathlib, sys
stage = pathlib.Path(os.environ['STAGE'])
info = json.load(open(sys.argv[1]))
identity = json.loads((stage / 'build-info.json').read_text())
assert identity['target'] == os.environ['KONTRA_TARGET'] and identity['revision'] == os.environ['GITHUB_SHA']
files = ['KONTRA2.dmg', 'KONTRA2.clap/Contents/MacOS/KONTRA2', 'KONTRA2.vst3/Contents/MacOS/KONTRA2', 'KONTRA2.app/Contents/MacOS/KONTRA2']
receipt = dict(version=identity['version'], revision=identity['revision'], target=identity['target'],
    id=info['id'], status=info['status'], stapled=True, signatures_verified=True,
    sha256={name: hashlib.sha256((stage / name).read_bytes()).hexdigest() for name in files})
(stage / 'notarization.json').write_text(json.dumps(receipt, indent=2) + '\n')
PY
