#!/usr/bin/env bash
# Sign and notarize the payload from package_macos.py. No unsigned fallback.
# Source macos_signing_keychain.sh first; this helper never imports credentials.
set -euo pipefail
stage=${1:?Usage: package_macos.sh COMPOSED_STAGE OUTPUT.pkg}
output=${2:?Usage: package_macos.sh COMPOSED_STAGE OUTPUT.pkg}
case "$output" in *.pkg) ;; *) echo 'Output must be a .pkg installer' >&2; exit 1 ;; esac
for name in APPLE_SIGNING_KEYCHAIN APPLE_DEVELOPER_ID_APPLICATION APPLE_DEVELOPER_ID_INSTALLER \
  APPLE_ID APPLE_APP_SPECIFIC_PASSWORD APPLE_TEAM_ID; do
  [ -n "${!name:-}" ] || { echo "Missing required signing configuration: $name" >&2; exit 1; }
done
case "$APPLE_DEVELOPER_ID_APPLICATION" in "Developer ID Application: "*" ($APPLE_TEAM_ID)") ;; *) echo 'Application signing identity/team mismatch' >&2; exit 1 ;; esac
case "$APPLE_DEVELOPER_ID_INSTALLER" in "Developer ID Installer: "*" ($APPLE_TEAM_ID)") ;; *) echo 'Installer signing identity/team mismatch' >&2; exit 1 ;; esac
[ ! -e "$output" ] || { echo 'Output installer already exists' >&2; exit 1; }
[ -f "$stage/build-info.json" ] || { echo 'Missing composed build identity' >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
version=$(python3 - "$stage/build-info.json" <<'PY'
import json, sys
info = json.load(open(sys.argv[1]))
assert info['target'] == 'universal-apple-darwin'
assert info['architectures'] == ['arm64', 'x86_64']
assert info['profile'] == 'release'
print(info['version'].split('-', 1)[0])
PY
)
for bundle in Library/Audio/Plug-Ins/CLAP/KONTRA.clap Library/Audio/Plug-Ins/VST3/KONTRA.vst3 Applications/KONTRA.app; do
  binary="$stage/payload/$bundle/Contents/MacOS/KONTRA"
  lipo "$binary" -verify_arch arm64 x86_64
  codesign --force --sign "$APPLE_DEVELOPER_ID_APPLICATION" --keychain "$APPLE_SIGNING_KEYCHAIN" \
    --options runtime --timestamp "$binary"
  codesign --force --sign "$APPLE_DEVELOPER_ID_APPLICATION" --keychain "$APPLE_SIGNING_KEYCHAIN" \
    --options runtime --timestamp "$stage/payload/$bundle"
  # codesign inherits the credential helper's private umask. Its new
  # signature resources must be readable by ordinary installed users.
  chmod -R a+rX "$stage/payload/$bundle"
  codesign --verify --deep --strict --all-architectures "$stage/payload/$bundle"
done
# Disable relocation: an old bundle elsewhere must not redirect installation
# away from the explicit /Library plugin folders and /Applications.
pkgbuild --analyze --root "$stage/payload" "$work/components.plist"
python3 - "$work/components.plist" <<'PY'
import plistlib, sys
path = sys.argv[1]
components = plistlib.load(open(path, 'rb'))
for component in components:
    component['BundleIsRelocatable'] = False
    # Nightlies share a numeric bundle version; an explicit previous-release
    # installer must also replace a newer bundle when the user rolls back.
    component['BundleIsVersionChecked'] = False
    component['BundleOverwriteAction'] = 'upgrade'
with open(path, 'wb') as output:
    plistlib.dump(components, output)
PY
pkgbuild --root "$stage/payload" --component-plist "$work/components.plist" \
  --identifier audio.matari.kontra.installer --version "$version" --install-location / \
  --ownership recommended "$work/unsigned.pkg"
productsign --sign "$APPLE_DEVELOPER_ID_INSTALLER" --keychain "$APPLE_SIGNING_KEYCHAIN" \
  --timestamp "$work/unsigned.pkg" "$work/KONTRA.pkg"
pkgutil --check-signature "$work/KONTRA.pkg"
xcrun notarytool submit "$work/KONTRA.pkg" --apple-id "$APPLE_ID" \
  --password "$APPLE_APP_SPECIFIC_PASSWORD" --team-id "$APPLE_TEAM_ID" \
  --wait --timeout 30m --output-format json > "$work/notary.json"
python3 - "$work/notary.json" <<'PY'
import json, sys, uuid
info = json.load(open(sys.argv[1]))
assert info['status'] == 'Accepted', 'Apple did not accept the installer notarization'
uuid.UUID(info['id'])
PY
xcrun stapler staple "$work/KONTRA.pkg"
xcrun stapler validate "$work/KONTRA.pkg"
spctl --assess --type install "$work/KONTRA.pkg"
# Publish only after real Apple acceptance, ticket validation and assessment.
mkdir -p "$(dirname "$output")"
python3 - "$stage" "$work/KONTRA.pkg" "$work/notary.json" "$work/receipt.json" <<'PY'
import hashlib, json, pathlib, sys
stage, package, notary, receipt = map(pathlib.Path, sys.argv[1:])
info = json.loads((stage / 'build-info.json').read_text())
accepted = json.loads(notary.read_text())
paths = ['Library/Audio/Plug-Ins/CLAP/KONTRA.clap/Contents/MacOS/KONTRA',
         'Library/Audio/Plug-Ins/VST3/KONTRA.vst3/Contents/MacOS/KONTRA',
         'Applications/KONTRA.app/Contents/MacOS/KONTRA']
info.update(targets=['aarch64-apple-darwin', 'x86_64-apple-darwin'], id=accepted['id'],
            status=accepted['status'], stapled=True, signatures_verified=True,
            package_sha256=hashlib.sha256(package.read_bytes()).hexdigest(),
            products={name: hashlib.sha256((stage / 'payload' / name).read_bytes()).hexdigest() for name in paths})
receipt.write_text(json.dumps(info, indent=2) + '\n')
PY
mv "$work/KONTRA.pkg" "$output"
mv "$work/receipt.json" "${output%.pkg}.notarization.json"
