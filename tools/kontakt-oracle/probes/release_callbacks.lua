-- Which scripts' `on release` runs for a note a script played (Kontakt 8.13; run with render.py,
-- trigger note 36, held ~0.5 s). Slot 0 plays three notes from the trigger:
--   key 60 with duration 0, ended by note_off() in the trigger's own `on release` (~0.5 s),
--   key 61 at 0.7 s, ended by note_off() at 1.2 s,
--   key 62 at 1.9 s with duration 300 ms (ends ~2.2 s).
-- (Run 1 had key 60 at duration -1: both slots' markers sounded for all three.)
-- Slot 0's `on release` plays a 150 ms marker on key (released key + 3), slot 1's on (key + 6).
-- Key 60 + k sounds 261.6 * 2^(5k/12) Hz, so each marker says which slot's on release ran for
-- which way a played note ended. A marker heard = that callback ran.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local dir = ORACLE_OUT:match("^(.*[\\/])")
local WAV = dir .. "sine.wav"
local f = assert(io.open(WAV, "wb"))
local rate, n = 48000, 48000 * 6
f:write("RIFF", string.pack("<I4", 36 + n * 4), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 1, rate, rate * 4, 4, 32),
  "data", string.pack("<I4", n * 4))
local w = 2 * math.pi * 261.6255653 / rate
local chunk = {}
for i = 0, n - 1 do
  chunk[#chunk + 1] = string.pack("<f", 0.15 * math.sin(w * i))
  if #chunk == 4800 then f:write(table.concat(chunk)) chunk = {} end
end
f:write(table.concat(chunk))
f:close()

local SLOT0 = [[
on init
  declare $c
  declare $d
  declare $e
  declare $m
end on
on note
  if ($EVENT_NOTE = 36)
    ignore_event($EVENT_ID)
    $c := play_note(60, 100, 0, 0)
    wait(700000)
    $d := play_note(61, 100, 0, 0)
    wait(500000)
    note_off($d)
    wait(700000)
    $e := play_note(62, 100, 0, 300000)
  end if
end on
on release
  if ($EVENT_NOTE = 36)
    note_off($c)
  end if
  if (in_range($EVENT_NOTE, 60, 62))
    $m := play_note($EVENT_NOTE + 3, 100, 0, 150000)
  end if
end on
]]
local SLOT1 = [[
on init
  declare $m
end on
on release
  if (in_range($EVENT_NOTE, 60, 62))
    $m := play_note($EVENT_NOTE + 6, 100, 0, 150000)
  end if
end on
]]

local function main()
  kt.reset_multi()
  local i = kt.add_instrument()
  kt.set_instrument_midi_channel(i, ORACLE_CHANNEL or 1)
  for k = 0, 8 do
    local z = kt.add_zone(i, 0, WAV)
    kt.set_zone_geometry(i, z, { root_key = 60 - 4 * k, low_key = 60 + k, high_key = 60 + k })
  end
  log(tostring(pcall(kt.set_instrument_script_source, i, 0, SLOT0)))
  log(tostring(pcall(kt.set_instrument_script_source, i, 1, SLOT1)))
end
local ok, err = xpcall(main, debug and debug.traceback or tostring)
if not ok then log('{"error":"' .. tostring(err):gsub('"', "'"):gsub("\n", " | ") .. '"}') end
log('{"ready":true}')
out:close()
