-- Measured probe: what find_mod / get_mod_idx / get_target_idx answer for a group's
-- internal (InternalModArray16) and external (ExternalModArray32) modulators.
-- Input: ANALOG STRINGS.nki. Its group 0 holds internal ENV_Volume..LFO_Pitch_Retrigger
-- (slots 0-7) and external VEL_VOLUME..Phase_P2 (slots 0-19, 11-13 empty).
-- Each answer v is written to group k's tune as set_engine_par(TUNE, 500000 + 5000 * v);
-- groups 0-2 calibrate (v = 0, 10, 20) and are read first.
-- Result (Kontakt 8.13.1, 2026-10-28): one index per group. Internal slot n answers n
-- (ENV_Volume 0, LFO_P1 3, LFO_Pitch_Retrigger 7); external slot n answers 16 + n with empty
-- slots kept (VEL_VOLUME 16, Sample_Start 20, CV_DISTORTION 30, Phase_P1 33, Phase_P2 35).
-- A miss: find_mod 0, get_mod_idx $NI_NOT_FOUND. get_target_idx(LFO_P1, LFO_RES_F1_P1) = 3.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local names = {
  "ENV_Volume", "LFO_P1_Retrigger", "ENV_Filter", "LFO_P1", "LFO_P2", "LFO_Pitch_Retrigger",
  "VEL_VOLUME", "PB_PITCH", "CV_CUTOFF", "Sample_Start", "VEL_TALK", "CV_DISTORTION",
  "CV_SATURATION", "Phase_P1_Retrigger", "Phase_P1", "Phase_P2", "no_such_mod",
}
local lines = { "on init" }
local function enc(group, expr)
  lines[#lines + 1] = string.format("  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * (%s), %d, -1, -1)", expr, group)
end
enc(0, "0") enc(1, "10") enc(2, "20")
local cases = {}
local g = 3
for _, n in ipairs(names) do
  cases[#cases + 1] = { "find_mod", n, g } enc(g, string.format('find_mod(0, "%s")', n)) g = g + 1
  cases[#cases + 1] = { "get_mod_idx", n, g } enc(g, string.format('get_mod_idx(0, "%s")', n)) g = g + 1
end
-- A route by name under its modulator's index (internal LFO_P1, route 3 = LFO_RES_F1_P1).
cases[#cases + 1] = { "get_target_idx", "LFO_P1/LFO_RES_F1_P1", g }
enc(g, 'get_target_idx(0, get_mod_idx(0, "LFO_P1"), "LFO_RES_F1_P1")') g = g + 1
lines[#lines + 1] = "end on"
local src = table.concat(lines, "\n") .. "\n"

kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
log(string.format('{"loaded":%s,"groups":%s}', tostring(i), tostring(kt.get_num_groups(i))))
for s = 1, 4 do pcall(kt.set_instrument_script_source, i, s, "on init\nend on\n") end
local ok, err = pcall(kt.set_instrument_script_source, i, 0, src)
log(string.format('{"compile":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'")))
local t0 = os.time()
repeat until os.time() - t0 >= 6
for k = 0, g - 1 do
  log(string.format('{"group":%d,"tune":%s}', k, tostring(kt.get_group_tune(i, k))))
end
for _, c in ipairs(cases) do
  log(string.format('{"fn":"%s","name":"%s","group":%d}', c[1], c[2], c[3]))
end
log('{"done":true}')
out:close()
