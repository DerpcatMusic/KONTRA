#!/usr/bin/env bash
# Quiet session: Una Cotton g39/g94 solo with the instrument's own scripts ON (only the solo script added in slot 5).
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd); . "$here/session_lib.sh"
OUT=$W/log/session_una_on.txt; : >"$OUT"
sess_begin
UNA="/mnt/MAIN_STORAGE/Libraries/Kontakt/Una Corda Library/Instruments/Una Corda Cotton.nki"
"$here/kontakt.sh" start "$UNA" && "$here/una_setup_on.sh"
for r in a b; do rec on_g39_v100_$r una_g39_v100 6 1; rec on_g94_v100_$r una_g94_v100 6 1; rec on_g39_multi_$r una_g39_multi 6 3; done
"$here/kontakt.sh" stop; sleep 3
sess_end
