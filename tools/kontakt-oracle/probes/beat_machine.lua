-- Beat Machine capture: one mono 48 kHz float sample of 0.5 s cut into four 1/16-note slices.
-- Zone grid is set to Fix mode at 120 BPM (Kontakt's default Fix width is 1/16 = 0.125 s), so the
-- markers fall at 0, 0.125, 0.25 and 0.375 s. Slice k is a sine of 25, 40, 55, 70 whole cycles
-- (200, 320, 440, 560 Hz), so each slice is recognisable and phase-continuous at its edges.
-- Group 0 is set to "beat_machine" with every other parameter at its default.
-- The probe plays the Beat Machine twice: once on the trigger note render.py sends about 1 s
-- into the recording (the trigger is ignored, it only starts the timer), then 2 s later.
-- Output: the readbacks as JSON lines, then {"ready":true} (render.py starts recording on it).
local kt = Kontakt
local DIR = "Z:\\data\\dev\\workspace\\Research\\KONTRA\\scratch-beat\\"
local out = assert(io.open(ORACLE_OUT, "w"))
local dirlog = assert(io.open(DIR .. "probe.log", "w"))
local function log(t) out:write(t .. "\n") out:flush() dirlog:write(t .. "\n") dirlog:flush() end

local SLICE = 6000 -- 0.125 s at 48 kHz
local FREQS = { 200, 320, 440, 560 }
local wav = DIR .. "beat_4slices_120bpm_48k_f32.wav"
do
  local f = assert(io.open(wav, "wb"))
  local n = SLICE * #FREQS
  local function u32(v) return string.pack("<I4", v) end
  f:write("RIFF", u32(36 + n * 4), "WAVEfmt ", u32(16), string.pack("<I2I2I4I4I2I2", 3, 1, 48000, 192000, 4, 32), "data", u32(n * 4))
  for k, hz in ipairs(FREQS) do
    for i = 0, SLICE - 1 do
      f:write(string.pack("<f", 0.5 * math.sin(2 * math.pi * hz * i / 48000)))
    end
  end
  f:close()
end

kt.reset_multi()
local inst = kt.add_instrument()
local zone = kt.add_zone(inst, 0, wav)
kt.set_zone_geometry(inst, zone, { root_key = 60, low_key = 0, high_key = 127 })
local ok, err = pcall(kt.set_zone_grid, inst, zone, "fixed", 120.0)
log(string.format('{"zone_grid":"fixed","bpm":120,"ok":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'"):sub(1, 120)))
if ok then
  log(string.format('{"grid_readback":"%s","grid_bpm":%s}', tostring(kt.get_zone_grid_mode(inst, zone)), tostring(kt.get_zone_grid_bpm(inst, zone))))
end
ok, err = pcall(kt.set_group_playback_mode, inst, 0, "beat_machine")
local now = "not read"
if ok then now = tostring(kt.get_group_playback_mode(inst, 0)) end
log(string.format('{"playback_mode":"beat_machine","ok":%s,"readback":"%s","err":"%s"}', tostring(ok), now, tostring(err):gsub('"', "'"):sub(1, 120)))

local src = [[
declare $fired
declare $hits
on init
  $fired := 0
  $hits := 0
end on
on note
  if (note = 36)
    ignore_event(event_id)
    if ($fired = 0)
      $fired := 1
      set_listener($NI_SIGNAL_TIMER_MS, 100)
    end if
  end if
end on
on listener
  inc($hits)
  play_note(60, 100, 0, -1)
  if ($hits < 2)
    set_listener($NI_SIGNAL_TIMER_MS, 2000)
  else
    set_listener($NI_SIGNAL_TIMER_MS, 1000000)
  end if
end on
]]
log('{"script_set":' .. tostring(pcall(kt.set_instrument_script_source, inst, 0, src)) .. '}')
log('{"ready":true}')
out:close()
dirlog:close()
