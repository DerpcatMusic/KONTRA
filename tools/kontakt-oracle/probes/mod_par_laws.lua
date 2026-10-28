-- Measured probe: the laws of the modulation engine parameters a script sets.
-- Input: ANALOG STRINGS.nki (group k's internal LFO_P1 is slot 3: route 0 = volume,
-- route 2 = cutoff; external VEL_VOLUME is index 16, Phase_P1 33). Case k sets one
-- parameter on group k; the instrument is saved and read back with KONTRA's parser
-- (route intensity and invert, LFO delay and frequency).
-- Result (Kontakt 8.13.1, 2026-10-28): MOD_TARGET_INTENSITY depth = x^2, sign kept;
-- INTMOD_INTENSITY / MOD_TARGET_MP_INTENSITY depth = (2x-1)^3; generic -1 = route 0;
-- LFO_DELAY ms = stage law to 10 s. INTMOD_FREQUENCY on a synced LFO stores the note count
-- round(4609 * 21300^-x), at least 1 (x 0, .1, .25, .5, .75, 1 -> 4609, 1704, 385, 32, 3, 1):
-- the count of a 0.01 * 21300^x Hz rate. Not implemented yet (the IR keeps beats, not the count).
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local dir = ORACLE_OUT:match("^(.*[\\/])")
local values = { 0, 100000, 250000, 500000, 750000, 1000000 }
local pars = {
  { "$ENGINE_PAR_INTMOD_INTENSITY", 3, 0 }, { "$ENGINE_PAR_MOD_TARGET_INTENSITY", 3, 0 },
  { "$ENGINE_PAR_MOD_TARGET_MP_INTENSITY", 3, 0 }, { "$ENGINE_PAR_INTMOD_INTENSITY", 3, -1 },
  { "$ENGINE_PAR_MOD_TARGET_INTENSITY", 3, -1 }, { "$ENGINE_PAR_INTMOD_INTENSITY", 16, -1 },
  { "$ENGINE_PAR_MOD_TARGET_INTENSITY", 16, -1 }, { "$ENGINE_PAR_MOD_TARGET_MP_INTENSITY", 16, -1 },
  { "$ENGINE_PAR_INTMOD_INTENSITY", 33, -1 }, { "$ENGINE_PAR_INTMOD_FREQUENCY", 3, -1 },
  { "$ENGINE_PAR_LFO_DELAY", 3, -1 },
}
local lines = { "on init" }
local cases = {}
local g = 0
for _, p in ipairs(pars) do
  for _, v in ipairs(values) do
    lines[#lines + 1] = string.format("  set_engine_par(%s, %d, %d, %d, %d)", p[1], v, g, p[2], p[3])
    cases[#cases + 1] = string.format('{"case":%d,"par":"%s","slot":%d,"generic":%d,"value":%d}', g, p[1], p[2], p[3], v)
    g = g + 1
  end
end
lines[#lines + 1] = "end on"
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
for s = 1, 4 do pcall(kt.set_instrument_script_source, i, s, "on init\nend on\n") end
local ok, err = pcall(kt.set_instrument_script_source, i, 0, table.concat(lines, "\n") .. "\n")
log(string.format('{"compile":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'")))
local t0 = os.time()
repeat until os.time() - t0 >= 6
local saved, serr = pcall(kt.save_instrument, i, dir .. "mod_par_laws.nki", {})
log(string.format('{"saved":%s,"path":"%s","err":"%s"}', tostring(saved), (dir .. "mod_par_laws.nki"):gsub("\\", "/"), tostring(serr):gsub('"', "'")))
for _, c in ipairs(cases) do log(c) end
log('{"done":true}')
out:close()
