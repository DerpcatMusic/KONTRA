-- Render feasibility: a 32-bit float impulse sample, an instrument whose KSP timer plays it
-- once a second. render.py records the null sink and checks the impulses arrive.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local DIR = "Z:\\tmp\\w3\\audit\\"

local function write_wav(path, samples)
  local f = assert(io.open(path, "wb"))
  local n = #samples
  f:write("RIFF", string.pack("<I4", 36 + n * 4), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 1, 48000, 192000, 4, 32), "data", string.pack("<I4", n * 4))
  for i = 1, n do f:write(string.pack("<f", samples[i])) end
  f:close()
end
local imp = {}
for i = 1, 24000 do imp[i] = 0.0 end
imp[1] = 0.5
write_wav(DIR .. "impulse.wav", imp)

kt.reset_multi()
local inst = kt.add_instrument()
local z = kt.add_zone(inst, 0, DIR .. "impulse.wav")
kt.set_zone_geometry(inst, z, { root_key = 60, low_key = 0, high_key = 127 })
kt.set_instrument_volume(inst, 0)
local src = [[
on init
  set_listener($NI_SIGNAL_TIMER_MS, 1000000)
end on
on listener
  play_note(60, 100, 0, -1)
end on
]]
log('{"script_set":' .. tostring(pcall(kt.set_instrument_script_source, inst, 0, src)) .. '}')
log('{"ready":true}')
out:close()
