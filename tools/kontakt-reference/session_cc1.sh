#!/usr/bin/env bash
# Quiet session: does unsent CC1 behave like a power-on constant? Barbarian Brass key 55 and Vista 3 Cellos key 48, each from a FRESH
# Kontakt start (unsent), twice. Run as ~/.cache/kontakto-quiet tools/kontakt-reference/session_cc1.sh
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd); . "$here/session_lib.sh"
OUT=$W/log/session_cc1.txt; : >"$OUT"
sess_begin
LIB=/mnt/MAIN_STORAGE/Libraries/Kontakt
for inst in barbarian vista; do
  case $inst in barbarian) nki="$LIB/Afflatus Chapter II Brass/Instruments/3. Curated Ensembles/Barbarian Brass.nki"; n=7; key=55;;
                vista) nki="$LIB/Performance Samples Vista/Instruments/Vista - 3 Cellos.nki"; n=7; key=48;; esac
  for r in a b; do
    "$here/kontakt.sh" start "$nki" || { say "$inst start failed"; continue; }
    rec cc1_${inst}_$r cc1_$inst 6 $n
    "$here/kontakt.sh" stop; sleep 3
  done
done
sess_end
