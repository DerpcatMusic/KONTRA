-- Measured probe: the transport builtins a script reads in standalone Kontakt (clock stopped).
-- Encoding as in find_mod_index.lua: group k's tune is case k's value in 0.36 st units.
-- Input: ANALOG STRINGS.nki (any instrument with enough groups).
-- Result (Kontakt 8.13.1, 2026-10-28, 120 BPM): $DURATION_QUARTER 500000, ms_to_ticks of it 960,
-- ticks_to_ms(960) 500000, ms_to_ticks(1000000) 1920, $DURATION_BAR 0 (clock stopped),
-- $DURATION_EIGHTH_TRIPLET 166667 (rounded), $DURATION_SIXTEENTH_TRIPLET 83333, $SIGNATURE 4/4,
-- $NI_TRANSPORT_RUNNING 0, $NI_SONG_POSITION 0, $NI_SIGNAL_TRANSP_START 1, _STOP 2.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local cases = {
  "$DURATION_QUARTER / 10000",
  "ms_to_ticks($DURATION_QUARTER) - 900",
  "ticks_to_ms(960) / 10000",
  "$DURATION_BAR / 10000",
  "$NI_SIGNAL_TRANSP_START",
  "$NI_SIGNAL_TRANSP_STOP",
  "$NI_TRANSPORT_RUNNING",
  "$SIGNATURE_NUM",
  "$SIGNATURE_DENOM",
  "$DURATION_EIGHTH_TRIPLET / 10000",
  "$NI_SONG_POSITION",
  "$DURATION_SIXTEENTH_TRIPLET / 1000 - 50",
  "ms_to_ticks(1000000) - 1900",
  "$DURATION_EIGHTH_TRIPLET - 166600",
  "$DURATION_SIXTEENTH_TRIPLET - 83300",
}
local lines = { "on init" }
for k, c in ipairs(cases) do
  lines[#lines + 1] = string.format("  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * (%s), %d, -1, -1)", c, k - 1)
end
lines[#lines + 1] = "end on"
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
for s = 1, 4 do pcall(kt.set_instrument_script_source, i, s, "on init\nend on\n") end
local ok, err = pcall(kt.set_instrument_script_source, i, 0, table.concat(lines, "\n") .. "\n")
log(string.format('{"compile":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'")))
local t0 = os.time()
repeat until os.time() - t0 >= 6
for k, c in ipairs(cases) do
  log(string.format('{"case":"%s","units":%.2f}', c, kt.get_group_tune(i, k - 1) / 0.36))
end
log('{"done":true}')
out:close()
