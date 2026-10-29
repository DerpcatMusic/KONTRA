-- Reads, in ANALOG STRINGS group 88, the indices ANALOG's control push uses for its retrigger
-- LFO routes and the depth its init push leaves on them, from slot 1's init (the dump harness
-- runs no audio, so no timer ticks). Each value + 1000 goes out in four base-100 digits.
-- Kontakt 8.13.1, 2026-10-28: find_mod 1, 4, 33; find_target VOL 0, CUT 2; intensity 755893,
-- 0, 755893. KONTRA answers the same (an appended init with the same reads). The route that
-- ANALOG rewrites after init is its own first-timer-tick ui__refresh re-pushing the same 914900
-- depth, not a find_mod miss or a misaddressed route.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
local reads = {
  'find_mod(88, "LFO_P1_Retrigger")', 'find_mod(88, "LFO_P2_Retrigger")', 'find_mod(88, "Phase_P1")',
  'find_target(88, find_mod(88, "LFO_P1_Retrigger"), "LFO_VOL_P1_Retrigger")',
  'find_target(88, find_mod(88, "LFO_P1_Retrigger"), "LFO_CUT_F1_P1_Retrigger")',
  'get_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY, 88, find_mod(88, "LFO_P1_Retrigger"), 0)',
  'get_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY, 88, find_mod(88, "LFO_P1_Retrigger"), 2)',
  'get_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY, 88, find_mod(88, "LFO_P2_Retrigger"), 0)',
}
local src = {"on init", "  declare $v"}
for k, expr in ipairs(reads) do
  table.insert(src, "    $v := " .. expr .. " + 1000")
  for d = 0, 3 do
    table.insert(src, string.format("    set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * (($v / %d) mod 100), %d, -1, -1)", 100 ^ d // 1, 440 + 4 * (k - 1) + d))
  end
end
table.insert(src, "end on")
local ok, err = pcall(kt.set_instrument_script_source, i, 1, table.concat(src, "\n") .. "\n")
log(string.format('{"ok":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'")))
local t0 = os.time() repeat until os.time() - t0 >= 6
for k = 1, #reads do
  local v = 0
  for d = 0, 3 do v = v + math.floor(kt.get_group_tune(i, 440 + 4 * (k - 1) + d) / 0.36 + 0.5) * 100 ^ d end
  log(string.format('{"read":"%s","value":%d}', reads[k]:gsub('"', "'"):gsub("%$", ""), v - 1000))
end
out:close()
