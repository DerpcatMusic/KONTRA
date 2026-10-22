-- Beat Machine single-zone probe (session A). One instrument, ONE zone over all keys (0-127,
-- root 48), 1/16 at 120 BPM on the session 1 sample (four 6000-frame cells), group 0 in mode
-- "beat_machine", no script. Notes come from the MIDI file beat_machine_session3_midi.py
-- scenario "single" (key 48 at 0 ms and 2000 ms, 1.2 s holds, velocity 100), so the note
-- source is MIDI, as in sessions 3 to 5. Compared with session 1 (one zone, KSP play_note) this
-- isolates the note source only if the zone count is the same; against session 3 (three zones,
-- MIDI) it isolates the zone count. Saves single.nki for the native render.
local kt = Kontakt
local DIR = "Z:\\data\\dev\\workspace\\Research\\KONTRA\\scratch-beat6\\"
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

kt.reset_multi()
local inst = kt.add_instrument()
local z = kt.add_zone(inst, 0, WAV_A)
kt.set_zone_geometry(inst, z, { root_key = 48, low_key = 0, high_key = 127 })
local ok, err = pcall(kt.set_zone_grid, inst, z, "fixed", 120.0)
log(string.format('{"grid_ok":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'"):sub(1, 120)))
local mok, merr = pcall(kt.set_group_playback_mode, inst, 0, "beat_machine")
log(string.format('{"mode_ok":%s,"readback":"%s","err":"%s"}', tostring(mok),
  mok and tostring(kt.get_group_playback_mode(inst, 0)) or "", tostring(merr):gsub('"', "'"):sub(1, 120)))
local sok, serr = pcall(kt.save_instrument, inst, DIR .. "single.nki", {})
log(string.format('{"save_ok":%s,"err":"%s"}', tostring(sok), tostring(serr):gsub('"', "'"):sub(1, 160)))
log('{"ready":true}')
out:close()
dirlog:close()
