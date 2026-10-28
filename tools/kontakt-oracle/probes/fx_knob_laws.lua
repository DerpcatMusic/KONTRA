-- Measured probe: the saved values a script's reverb, limiter, IR-convolution and amp knobs leave.
-- Input: ANALOG STRINGS.nki, at the slots its own script writes: bus 2 (1026) slot 7 = Reverb,
-- slot 4 = amp (JMP); instrument insert 4 = Limiter; send 5 = Convolution. For each x the knobs
-- are set, the instrument saved, and the saves read back with KONTRA's parser.
-- Result (Kontakt 8.13.1, 2026-10-28), x = 0, .1, .25, .5, .75, .9, 1: Reverb predelay ms 0, 1.140,
-- 4.177, 17.079, 56.927, 113.922, 180 = 180 (e^(4.511x) - 1)/(e^4.511 - 1); size, colour, stereo,
-- damping saved as x. Limiter input dB -24 + 48x; release ms 10, 16.679, 34.275, 107.654, 329.471,
-- 641.723, 1000 = 10 + 990 (e^(4.425x) - 1)/(e^4.425 - 1). Amp (0x4d): all six saved as x (not
-- carried by KONTRA). Convolution (send 5, 0x16): no IRC_* write moved any saved field.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local dir = ORACLE_OUT:match("^(.*[\\/])")
local values = { 0, 100000, 250000, 500000, 750000, 900000, 1000000 }
local pars = {
  { "RV_PREDELAY", 7, 1026 }, { "RV_SIZE", 7, 1026 }, { "RV_COLOUR", 7, 1026 },
  { "RV_STEREO", 7, 1026 }, { "RV_DAMPING", 7, 1026 },
  { "LIM_IN_GAIN", 4, 1 }, { "LIM_RELEASE", 4, 1 },
  { "IRC_PREDELAY", 5, 0 }, { "IRC_LENGTH_RATIO_ER", 5, 0 }, { "IRC_LENGTH_RATIO_LR", 5, 0 },
  { "IRC_FREQ_HIGHPASS_ER", 5, 0 }, { "IRC_FREQ_LOWPASS_ER", 5, 0 },
  { "IRC_FREQ_HIGHPASS_LR", 5, 0 }, { "IRC_FREQ_LOWPASS_LR", 5, 0 },
  { "JMP_PREAMP", 4, 1026 }, { "JMP_BASS", 4, 1026 }, { "JMP_MID", 4, 1026 },
  { "JMP_TREBLE", 4, 1026 }, { "JMP_MASTER", 4, 1026 }, { "JMP_PRESENCE", 4, 1026 },
}
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
for s = 1, 4 do pcall(kt.set_instrument_script_source, i, s, "on init\nend on\n") end
for n, v in ipairs(values) do
  local src = { "on init" }
  for _, p in ipairs(pars) do
    src[#src + 1] = string.format("  set_engine_par($ENGINE_PAR_%s, %d, -1, %d, %d)", p[1], v, p[2], p[3])
  end
  src[#src + 1] = "end on\n"
  local ok, err = pcall(kt.set_instrument_script_source, i, 0, table.concat(src, "\n"))
  local t0 = os.time() repeat until os.time() - t0 >= 5
  local name = string.format("fx_save_%d.nki", n)
  local saved, serr = pcall(kt.save_instrument, i, dir .. name, {})
  log(string.format('{"value":%d,"compile":%s,"saved":%s,"err":"%s"}', v, tostring(ok), tostring(saved),
    (tostring(err) .. tostring(serr)):gsub('"', "'")))
end
log('{"done":true}')
out:close()
