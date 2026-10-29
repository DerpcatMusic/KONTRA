-- Measured probe: the saved Hz, octaves and dB a script's EQ knobs leave in the 1/2/3-band EQ.
-- Input: ANALOG STRINGS.nki, instrument insert 0 = 3-band EQ. For each x every band's FREQ,
-- BW and GAIN are set, the instrument saved, and the saves read back with KONTRA's parser.
-- Result (Kontakt 8.13.1, 2026-10-28), every band alike, x = 0, .1, .25, .5, .75, .9, 1:
-- Hz 20, 39.905, 112.468, 632.456, 3556.56, 10023.7, 20000 = 20 * 1000^x; octaves 0.333, 0.6, 1,
-- 1.667, 2.333, 2.733, 3 = 1/3 + 8x/3; dB -18, -14.4, -9, 0, 9, 14.4, 18 = -18 + 36x.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local dir = ORACLE_OUT:match("^(.*[\\/])")
local values = { 0, 100000, 250000, 500000, 750000, 900000, 1000000 }
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
for s = 1, 4 do pcall(kt.set_instrument_script_source, i, s, "on init\nend on\n") end
for n, v in ipairs(values) do
  local src = { "on init" }
  for b = 1, 3 do
    for _, p in ipairs({ "FREQ", "BW", "GAIN" }) do
      src[#src + 1] = string.format("  set_engine_par($ENGINE_PAR_%s%d, %d, -1, 0, 1)", p, b, v)
    end
  end
  src[#src + 1] = "end on\n"
  local ok, err = pcall(kt.set_instrument_script_source, i, 0, table.concat(src, "\n"))
  local t0 = os.time() repeat until os.time() - t0 >= 5
  local name = string.format("eq_save_%d.nki", n)
  local saved, serr = pcall(kt.save_instrument, i, dir .. name, {})
  log(string.format('{"value":%d,"compile":%s,"saved":%s,"err":"%s"}', v, tostring(ok), tostring(saved),
    (tostring(err) .. tostring(serr)):gsub('"', "'")))
end
log('{"done":true}')
out:close()
