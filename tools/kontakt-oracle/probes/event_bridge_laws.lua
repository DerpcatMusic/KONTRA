-- Event-command laws, measured as audio: one instrument, a 440 Hz sine at root A4 with
-- its zone at -6 dB, and a KSP script whose listener plays one note per second. Each tick
-- applies one bridge case (change_tune, change_vol, change_pan, fade_in, fade_out) to the
-- note it plays. render.py records the output; analyze_event_laws.py measures each tick
-- against the plain baseline (tick 1). Run under render.py only (needs the audio thread).
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local dir = ORACLE_OUT:gsub("[^\\/]+$", "")

local function write_wav(path, samples)
  local f = assert(io.open(path, "wb"))
  local n = #samples
  f:write("RIFF", string.pack("<I4", 36 + n * 4), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 1, 48000, 192000, 4, 32), "data", string.pack("<I4", n * 4))
  for i = 1, n do f:write(string.pack("<f", samples[i])) end
  f:close()
end
local sine = {}
for i = 1, 48000 * 8 do sine[i] = 0.5 * math.sin(2 * math.pi * 440 * (i - 1) / 48000) end
write_wav(dir .. "event_laws_sine440.wav", sine)

kt.reset_multi()
local inst = kt.add_instrument()
local z = kt.add_zone(inst, 0, dir .. "event_laws_sine440.wav")
kt.set_zone_geometry(inst, z, { root_key = 69, low_key = 0, high_key = 127 })
kt.set_instrument_volume(inst, 0)
local zone_ok = pcall(kt.set_zone_volume, inst, z, -6)
log('{"zone_volume_set":' .. tostring(zone_ok) .. '}')

-- Tick n plays one note at 1 s spacing; the tick number is the case index in the analysis.
local src = [[
on init
  declare $tick := 0
  declare $id := 0
  declare $old := 0
  set_listener($NI_SIGNAL_TIMER_MS, 1000)
end on
on listener
  inc($tick)
  if $tick = 1 then
    play_note(69, 100, 0, 800000)
  end if
  if $tick = 2 then
    $id := play_note(69, 100, 0, 800000)
    change_tune($id, 100000, 0)
  end if
  if $tick = 3 then
    $id := play_note(69, 100, 0, 800000)
    change_tune($id, 100000, 0)
    change_tune($id, 50000, 1)
  end if
  if $tick = 4 then
    $id := play_note(69, 100, 0, 800000)
    change_tune($id, 100000, 2)
    change_tune($id, -50000, 2)
  end if
  if $tick = 5 then
    $id := play_note(69, 100, 0, 800000)
    change_vol($id, -6000, 0)
  end if
  if $tick = 6 then
    $id := play_note(69, 100, 0, 800000)
    change_vol($id, -6000, 1)
  end if
  if $tick = 7 then
    $id := play_note(69, 100, 0, 800000)
    change_vol($id, 0, 2)
  end if
  if $tick = 8 then
    $id := play_note(69, 100, 0, 800000)
    change_vol($id, 0, 0)
  end if
  if $tick = 9 then
    $id := play_note(69, 100, 0, 800000)
    change_pan($id, -1000, 0)
  end if
  if $tick = 10 then
    $id := play_note(69, 100, 0, 800000)
    change_pan($id, 500, 0)
  end if
  if $tick = 11 then
    $id := play_note(69, 100, 0, 800000)
    fade_in($id, 500000)
  end if
  if $tick = 12 then
    $id := play_note(69, 100, 0, -1)
  end if
  if $tick = 13 then
    fade_out($id, 500000, 1)
  end if
end on
]]
log('{"script_set":' .. tostring(pcall(kt.set_instrument_script_source, inst, 0, src)) .. '}')
log('{"ready":true}')
out:close()
