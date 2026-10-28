-- Is $ENGINE_PAR_CUTOFF / RESONANCE the filter's saved 0..1 knob (x 1e6)?
-- Input: ANALOG STRINGS.nki (483 groups, free script slots 0 and 1). Reads go into group tunes
-- 470.. as three base-100 digits each (500000 + 5000 d, 0.36 st per unit).
-- Result (Kontakt 8.13.1, 2026-10-28): 766585, 561250, 0, 0 = the saved knobs x 1e6 (our reader:
-- group 0 slot 0 cutoff 0.766585, group 2 slot 0 0.56125, instrument insert 6 cutoff 0, resonance 0).
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
local reads = {
  "get_engine_par($ENGINE_PAR_CUTOFF, 0, 0, -1)",
  "get_engine_par($ENGINE_PAR_CUTOFF, 2, 0, -1)",
  "get_engine_par($ENGINE_PAR_CUTOFF, -1, 6, 1)",
  "get_engine_par($ENGINE_PAR_RESONANCE, 0, 0, -1)",
}
local src = {"on init", "  declare $v"}
for k, expr in ipairs(reads) do
  table.insert(src, "  $v := " .. expr)
  for d = 0, 2 do
    table.insert(src, string.format("  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * (($v / %d) mod 100), %d, -1, -1)",
      100 ^ d // 1, 470 + 3 * (k - 1) + d))
  end
end
table.insert(src, "end on")
local ok, err = pcall(kt.set_instrument_script_source, i, 0, table.concat(src, "\n") .. "\n")
log(string.format('{"ok":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'")))
local t0 = os.time() repeat until os.time() - t0 >= 8
for k = 1, #reads do
  local v = 0
  for d = 0, 2 do v = v + math.floor(kt.get_group_tune(i, 470 + 3 * (k - 1) + d) / 0.36 + 0.5) * 100 ^ d end
  log(string.format('{"read":"%s","value":%d}', reads[k]:gsub("%$", ""), v))
end
out:close()
