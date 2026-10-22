-- Beat Machine capture with saved instruments. Same sample and grid as beat_machine.lua
-- (0.5 s mono, four 1/16 slices at 120 BPM), plus:
--   * three instruments saved as patches for the file-level diff:
--       beat_fixed.nki    zone grid fixed 120, group 0 "beat_machine"
--       sampler_fixed.nki zone grid fixed 120, group 0 sampler
--       plain.nki         no grid, group 0 sampler
--   * the audio run uses the beat_fixed setup. Note 60 is released after 0.8 s
--     (play_note duration is in microseconds). The first version held the note with
--     duration -1, which may be why the second trigger was silent.
-- render.py starts recording on the {"ready":true} line.
local kt = Kontakt
local DIR = "Z:\\data\\dev\\workspace\\Research\\KONTRA\\scratch-beat2\\"
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
  for _, hz in ipairs(FREQS) do
    for i = 0, SLICE - 1 do
      f:write(string.pack("<f", 0.5 * math.sin(2 * math.pi * hz * i / 48000)))
    end
  end
  f:close()
end

-- Builds one instrument with one zone; returns its index.
local function build(mode, grid, label)
  kt.reset_multi()
  local inst = kt.add_instrument()
  local zone = kt.add_zone(inst, 0, wav)
  kt.set_zone_geometry(inst, zone, { root_key = 60, low_key = 0, high_key = 127 })
  if grid then
    local ok, err = pcall(kt.set_zone_grid, inst, zone, "fixed", 120.0)
    log(string.format('{"%s_grid_ok":%s,"err":"%s"}', label, tostring(ok), tostring(err):gsub('"', "'"):sub(1, 120)))
  end
  if mode then
    local ok, err = pcall(kt.set_group_playback_mode, inst, 0, mode)
    log(string.format('{"%s_mode":"%s","ok":%s,"readback":"%s","err":"%s"}', label, mode, tostring(ok),
      ok and tostring(kt.get_group_playback_mode(inst, 0)) or "", tostring(err):gsub('"', "'"):sub(1, 120)))
  end
  return inst
end

local function save(inst, name)
  local ok, err = pcall(kt.save_instrument, inst, DIR .. name, {})
  log(string.format('{"save":"%s","ok":%s,"err":"%s"}', name, tostring(ok), tostring(err):gsub('"', "'"):sub(1, 160)))
end

save(build("beat_machine", true, "beat_fixed"), "beat_fixed.nki")
save(build(nil, true, "sampler_fixed"), "sampler_fixed.nki")
save(build(nil, false, "plain"), "plain.nki")

local inst = build("beat_machine", true, "audio")
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
  play_note(60, 100, 0, 800000)
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
