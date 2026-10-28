-- Measured probe: INTMOD_INTENSITY on a pitch route against a volume route.
-- Input: ANALOG STRINGS.nki. Groups 0..5 set LFO_Pitch_Retrigger (find_mod 7, its
-- one route: pitch, 12 semitones), groups 6..11 set LFO_P1 (volume) to 0, .25, .5,
-- .6, .75, 1 (x 1e6); the instrument is saved and read back with KONTRA's parser
-- (`KONTRA_GROUPS=0..12 cargo run -p kontra-format-kontakt --example inspect`).
-- Result (Kontakt 8.13.1, 2026-10-29): both store (2x-1)^3: -1, -0.125, 0, 0.008,
-- 0.125, 1. Pitch routes share the volume law; 500000 is no depth.
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local dir = ORACLE_OUT:match("^(.*[\\/])")
local values = { 0, 250000, 500000, 600000, 750000, 1000000 }
local lines = { "on init" }
local cases = {}
local g = 0
for _, m in ipairs({ "LFO_Pitch_Retrigger", "LFO_P1" }) do
  for _, v in ipairs(values) do
    lines[#lines + 1] = string.format('  set_engine_par($ENGINE_PAR_INTMOD_INTENSITY, %d, %d, find_mod(%d, "%s"), -1)', v, g, g, m)
    cases[#cases + 1] = string.format('{"case":%d,"mod":"%s","value":%d}', g, m, v)
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
local saved, serr = pcall(kt.save_instrument, i, dir .. "pitch_law.nki", {})
log(string.format('{"saved":%s,"path":"%s","err":"%s"}', tostring(saved), (dir .. "pitch_law.nki"):gsub("\\", "/"), tostring(serr):gsub('"', "'")))
for _, c in ipairs(cases) do log(c) end
log('{"done":true}')
out:close()
