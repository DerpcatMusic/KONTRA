-- Measured probe: the saved values a script's knob writes leave in compressor, delay and
-- LFO slots. Input: ANALOG STRINGS.nki: instrument insert 1 = Compressor, send 3 = Delay,
-- group k's LFO_P1 = slot 3. For each x the knobs are set, the instrument saved, and the
-- saves read back with KONTRA's parser.
-- Result (Kontakt 8.13.1, 2026-10-28), saved Compressor (mode 0) attack / release ms for x = 0, .1,
-- .25, .5, .75, .9, 1: 1, 3.364, 10.825, 52.827, 232.386, 558.393, 1000 / 50, 74.601, 134.954,
-- 358.699, 947.984, 1696.474, 2500 = lo + (hi - lo)(e^(kx) - 1)/(e^k - 1), k 5.811 / 3.874.
-- INTMOD_PULSEWIDTH on group 0 slot 3 (LFO_P1, a Multi) saves width 0.05, 0.14, 0.275, 0.5,
-- 0.725, 0.86, 0.95 = 0.05 + 0.9x (ANALOG's own write of 0.193 stores 0.224). The DL_* writes on
-- send 3 changed no saved delay: that slot holds no legacy Delay.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local dir = ORACLE_OUT:match("^(.*[\\/])")
local values = { 0, 100000, 250000, 500000, 750000, 900000, 1000000 }
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
for s = 1, 4 do pcall(kt.set_instrument_script_source, i, s, "on init\nend on\n") end
for n, v in ipairs(values) do
  local src = table.concat({
    "on init",
    string.format("  set_engine_par($ENGINE_PAR_COMP_ATTACK, %d, -1, 1, 1)", v),
    string.format("  set_engine_par($ENGINE_PAR_COMP_DECAY, %d, -1, 1, 1)", v),
    string.format("  set_engine_par($ENGINE_PAR_DL_TIME, %d, -1, 3, 0)", v),
    string.format("  set_engine_par($ENGINE_PAR_DL_FEEDBACK, %d, -1, 3, 0)", v),
    string.format("  set_engine_par($ENGINE_PAR_DL_PAN, %d, -1, 3, 0)", v),
    string.format("  set_engine_par($ENGINE_PAR_DL_DAMPING, %d, -1, 3, 0)", v),
    string.format("  set_engine_par($ENGINE_PAR_INTMOD_PULSEWIDTH, %d, 0, 3, -1)", v),
    "end on", ""}, "\n")
  local ok, err = pcall(kt.set_instrument_script_source, i, 0, src)
  local t0 = os.time() repeat until os.time() - t0 >= 5
  local name = string.format("knob_save_%d.nki", n)
  local saved, serr = pcall(kt.save_instrument, i, dir .. name, {})
  log(string.format('{"value":%d,"compile":%s,"saved":%s,"path":"%s","err":"%s"}', v, tostring(ok),
    tostring(saved), (dir .. name):gsub("\\", "/"), (tostring(err) .. tostring(serr)):gsub('"', "'")))
end
log('{"done":true}')
out:close()
