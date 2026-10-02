#!/usr/bin/env bash
# Source inside the signing step; restore the runner search list on EXIT.
set -euo pipefail
umask 077
for name in APPLE_APPLICATION_CERTIFICATE_P12_BASE64 APPLE_CERTIFICATE_PASSWORD APPLE_DEVELOPER_ID_APPLICATION; do
  [ -n "${!name:-}" ] || { echo "Missing required signing secret: $name" >&2; exit 1; }
done
# security prints quoted paths; preserve spaces and macOS Bash3.2 support.
original_keychain_search=$(security list-keychains -d user)
original_keychains=()
while IFS= read -r -d '' path; do original_keychains+=("$path"); done < <(
  python3 -c 'import shlex,sys; sys.stdout.write("".join(p+"\0" for p in shlex.split(sys.argv[1])))' "$original_keychain_search")
work=$(mktemp -d)
keychain="$work/signing.keychain-db"
keychain_password=$(uuidgen)
cleanup() {
  security list-keychains -d user -s "${original_keychains[@]}" >/dev/null 2>&1 || true
  security delete-keychain "$keychain" >/dev/null 2>&1 || true
  rm -rf -- "$work"
}
trap cleanup EXIT
export APPLICATION_P12="$work/application.p12"
python3 - <<'PY'
import base64, os, pathlib
pathlib.Path(os.environ['APPLICATION_P12']).write_bytes(base64.b64decode(''.join(os.environ['APPLE_APPLICATION_CERTIFICATE_P12_BASE64'].split()), validate=True))
PY
security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
# Import may report a duplicate CA already carried inside a P12. Only that
# exact error is safe; the application identity/chain is verified below.
import_certificate() {
  if ! security import "$@" > /dev/null 2> "$work/certificate-import.log"; then
    error=$(cat "$work/certificate-import.log")
    [ "$error" = 'security: SecKeychainItemImport: The specified item already exists in the keychain.' ] || {
      printf '%s\n' "$error" >&2; exit 1;
    }
  fi
}
echo 'Importing the Developer ID signing identity'
import_certificate "$APPLICATION_P12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" -t cert -f pkcs12 -T /usr/bin/codesign
if [ -n "${APPLE_INSTALLER_CERTIFICATE_P12_BASE64:-}" ]; then
  python3 - "$work/installer.p12" <<'PYINSTALLER'
import base64, os, pathlib, sys
pathlib.Path(sys.argv[1]).write_bytes(base64.b64decode(''.join(os.environ['APPLE_INSTALLER_CERTIFICATE_P12_BASE64'].split()), validate=True))
PYINSTALLER
  import_certificate "$work/installer.p12" -k "$keychain" -P "$APPLE_CERTIFICATE_PASSWORD" -t cert -f pkcs12 -T /usr/bin/productsign -T /usr/bin/pkgbuild
fi
# codesign needs the imported identity in its search list, and its chain
# must include Apple's Developer ID intermediates on a fresh hosted runner.
security list-keychains -d user -s "$keychain" "${original_keychains[@]}"
echo 'Completing the Developer ID certificate chain'
for certificate in DeveloperIDCA DeveloperIDG2CA; do
  curl --fail --silent --show-error "https://www.apple.com/certificateauthority/$certificate.cer" -o "$work/$certificate.cer"
  import_certificate "$work/$certificate.cer" -k "$keychain" -t cert
done
security set-key-partition-list -S apple-tool:,apple: -s -k "$keychain_password" "$keychain" >/dev/null
echo 'Validating the configured Developer ID identity and certificate chain'
security find-certificate -c "$APPLE_DEVELOPER_ID_APPLICATION" -p "$keychain" > "$work/application.cer"
security verify-cert -c "$work/application.cer" -p codeSign -k "$keychain" >/dev/null
export APPLE_SIGNING_KEYCHAIN="$keychain"
