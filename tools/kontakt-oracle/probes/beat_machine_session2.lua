-- Beat Machine session 2: grid and BPM change, retrigger timing, hold length.
-- Three zones, each in group 0 "beat_machine", each with its own KSP timer sequence started by
-- the render.py MIDI note 36 (ignored by the sequence):
--   A  1/16 at 120 BPM, 6000-frame cells (the session 1 setup), 0.3 s to 7.3 s after the note:
--      a retrigger 0.4 s after the first, a 3 s hold, and a plain 0.8 s note.
--   B  1/16 at 90 BPM, 8000-frame cells, two notes 2 s apart.
--   C  1/8 at 100 BPM (14400-frame cells; `set_zone_grid` with a width argument is tried, its
--      error is logged), two notes 3 s apart.
-- Durations are play_note microseconds. Slice tones are chosen so each cell holds whole cycles.
-- RESULT (one run, session2_grid_retrigger.wav): only A's first note sounded; A's later notes and
-- zones B and C were not heard. Standalone Kontakt sends MIDI to the selected instrument only, and
-- the KSP timer chain did not re-arm audibly. Not a valid grid or retrigger test: see the doc.
local kt = Kontakt
local DIR = "Z:\\data\\dev\\workspace\\Research\\KONTRA\\scratch-beat3\\"
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

-- Timer sequence: `at` are offsets (ms) from the note, `dur` play_note durations (us).
local function sequence(at, dur)
  local n = #at
  local lines = { "declare $i", "declare %dur[" .. n .. "]", "declare %dt[" .. n .. "]", "on init", "  $i := 0" }
  for k = 1, n do
    local dt = (k < n) and (at[k + 1] - at[k]) or 1000000
    lines[#lines + 1] = string.format("  %%dur[%d] := %d", k - 1, dur[k])
    lines[#lines + 1] = string.format("  %%dt[%d] := %d", k - 1, dt)
  end
  lines[#lines + 1] = "end on"
  lines[#lines + 1] = "on note"
  lines[#lines + 1] = "  if (note = 36)"
  lines[#lines + 1] = "    ignore_event(event_id)"
  lines[#lines + 1] = "    $i := 0"
  lines[#lines + 1] = string.format("    set_listener($NI_SIGNAL_TIMER_MS, %d)", at[1])
  lines[#lines + 1] = "  end if"
  lines[#lines + 1] = "end on"
  lines[#lines + 1] = "on listener"
  lines[#lines + 1] = string.format("  if ($i < %d)", n)
  lines[#lines + 1] = "    play_note(60, 100, 0, %dur[$i])"
  lines[#lines + 1] = "    set_listener($NI_SIGNAL_TIMER_MS, %dt[$i])"
  lines[#lines + 1] = "    inc($i)"
  lines[#lines + 1] = "  end if"
  lines[#lines + 1] = "end on"
  return table.concat(lines, "\n") .. "\n"
end

local function build(wav, grid_args, label, at, dur)
  local inst = kt.add_instrument()
  local zone = kt.add_zone(inst, 0, wav)
  kt.set_zone_geometry(inst, zone, { root_key = 60, low_key = 0, high_key = 127 })
  local ok, err = pcall(kt.set_zone_grid, inst, zone, table.unpack(grid_args))
  log(string.format('{"%s_grid_ok":%s,"args":"%s","err":"%s"}', label, tostring(ok),
    table.concat(grid_args, ","), tostring(err):gsub('"', "'"):sub(1, 120)))
  local mok, merr = pcall(kt.set_group_playback_mode, inst, 0, "beat_machine")
  log(string.format('{"%s_mode_ok":%s,"readback":"%s","err":"%s"}', label, tostring(mok),
    mok and tostring(kt.get_group_playback_mode(inst, 0)) or "", tostring(merr):gsub('"', "'"):sub(1, 120)))
  local sok, serr = pcall(kt.set_instrument_script_source, inst, 0, sequence(at, dur))
  log(string.format('{"%s_script_ok":%s,"err":"%s"}', label, tostring(sok), tostring(serr):gsub('"', "'"):sub(1, 120)))
  return inst
end

kt.reset_multi()
build(WAV_A, { "fixed", 120.0 }, "A_16_120", { 300, 700, 3300, 7300 }, { 800000, 800000, 3000000, 800000 })
build(WAV_B, { "fixed", 90.0 }, "B_16_90", { 10300, 12300 }, { 800000, 800000 })
build(WAV_C, { "fixed", 100.0, 8 }, "C_8_100", { 15300, 18300 }, { 800000, 800000 })
log('{"ready":true}')
out:close()
dirlog:close()
