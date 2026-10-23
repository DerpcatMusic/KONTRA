-- Measured probe: polyphonic variable reads/writes in on init (no note, no audio needed).
-- Each case is one instrument; its volume is set by set_engine_par(VOLUME, 1000 * ($out + 1)).
-- Calibration (same run family, probes/ksp_polyphonic_read.lua): 1000 -> -167.38 dB, 2000 -> -149.38 dB.
-- Result: docs/re/engine/ksp-polyphonic-read.md (fixture: ksp-polyphonic-read-init.jsonl).
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
kt.reset_multi()
local ENC = "  set_engine_par($ENGINE_PAR_VOLUME, 1000 * ($out + 1), -1, -1, -1)\n"
local cases = {
  { "init_read_poly_no_note", "on init\n  declare polyphonic $v\n  declare $out := -1\n  $out := $v\n" .. ENC .. "end on\n" },
  { "init_write_then_read_poly", "on init\n  declare polyphonic $v\n  declare $out := -1\n  $v := 5\n  $out := $v\n" .. ENC .. "end on\n" },
  { "init_plain_expr", "on init\n  declare $out := -1\n  $out := 1\n" .. ENC .. "end on\n" },
  { "listener_plain", "on init\n  declare $out := -1\n  set_listener($NI_SIGNAL_TIMER_MS, 1000)\nend on\non listener\n  $out := 3\n" .. ENC .. "end on\n" },
}
local idx = {}
for _, c in ipairs(cases) do
  local i = kt.add_instrument()
  kt.set_instrument_volume(i, 0)
  idx[#idx + 1] = { c[1], i }
  local ok, err = pcall(kt.set_instrument_script_source, i, 0, c[2])
  log(string.format('{"compile":"%s","ok":%s}', c[1], tostring(ok)))
end
local t0 = os.time()
repeat until os.time() - t0 >= 4
for _, r in ipairs(idx) do
  log(string.format('{"case":"%s","volume_db":%s}', r[1], tostring(kt.get_instrument_volume(r[2]))))
end
log('{"done":true}')
out:close()
