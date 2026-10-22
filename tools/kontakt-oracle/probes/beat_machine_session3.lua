-- Beat Machine session 3: one instrument, three zones, MIDI-timed (no KSP timers).
-- Zones (key range, root key, grid):
--   A  keys 0-59,   root 48, 1/16 at 120 BPM, 6000-frame cells (session 1 setup)
--   B  keys 60-84,  root 72, 1/16 at  90 BPM, 8000-frame cells
--   C  keys 85-127, root 96, 1/8 at 100 BPM, 14400-frame cells (set_zone_grid width 8)
-- All zones are in group 0, mode "beat_machine". The notes come from the MIDI file given to
-- render.py (tools/kontakt-oracle/render.py PROBE OUT SECONDS MIDI), so every note-on and
-- note-off is timed by the file, not by a script. The file is written by
-- beat_machine_session3_midi.py in this folder.
-- Saves the instrument to beat3.nki for the zone-marker readback.
local kt = Kontakt
local DIR = "Z:\\data\\dev\\workspace\\Research\\KONTRA\\scratch-beat4\\"
local out = assert(io.open(ORACLE_OUT, "w"))
local dirlog = assert(io.open(DIR .. "probe.log", "w"))
local function log(t) out:write(t .. "\n") out:flush() dirlog:write(t .. "\n") dirlog:flush() end

local function write_wav(name, cell, tones)
  local f = assert(io.open(DIR .. name, "wb"))
  local n = cell * #tones
  local function u32(v) return string.pack("<I4", v) end
  f:write("RIFF", u32(36 + n * 4), "WAVEfmt ", u32(16), string.pack("<I2I2I4I4I2I2", 3, 1, 48000, 192000, 4, 32), "data", u32(n * 4))
  for _, hz in ipairs(tones) do
    for i = 0, cell - 1 do
      f:write(string.pack("<f", 0.5 * math.sin(2 * math.pi * hz * i / 48000)))
    end
  end
  f:close()
  return DIR .. name
end

local WAV_A = write_wav("a_16_120.wav", 6000, { 200, 320, 440, 560 })
local WAV_B = write_wav("b_16_90.wav", 8000, { 240, 360, 480, 600 })
local WAV_C = write_wav("c_8_100.wav", 14400, { 200, 320, 440, 560 })

kt.reset_multi()
local inst = kt.add_instrument()

local function zone(wav, low, high, root, grid_args, label)
  local z = kt.add_zone(inst, 0, wav)
  kt.set_zone_geometry(inst, z, { root_key = root, low_key = low, high_key = high })
  local ok, err = pcall(kt.set_zone_grid, inst, z, table.unpack(grid_args))
  log(string.format('{"%s_grid_ok":%s,"args":"%s","err":"%s"}', label, tostring(ok),
    table.concat(grid_args, ","), tostring(err):gsub('"', "'"):sub(1, 120)))
end

zone(WAV_A, 0, 59, 48, { "fixed", 120.0 }, "A_16_120")
zone(WAV_B, 60, 84, 72, { "fixed", 90.0 }, "B_16_90")
zone(WAV_C, 85, 127, 96, { "fixed", 100.0, 8 }, "C_8_100")

local mok, merr = pcall(kt.set_group_playback_mode, inst, 0, "beat_machine")
log(string.format('{"mode_ok":%s,"readback":"%s","err":"%s"}', tostring(mok),
  mok and tostring(kt.get_group_playback_mode(inst, 0)) or "", tostring(merr):gsub('"', "'"):sub(1, 120)))
local sok, serr = pcall(kt.save_instrument, inst, DIR .. "beat3.nki", {})
log(string.format('{"save_ok":%s,"err":"%s"}', tostring(sok), tostring(serr):gsub('"', "'"):sub(1, 160)))
log('{"ready":true}')
out:close()
dirlog:close()
