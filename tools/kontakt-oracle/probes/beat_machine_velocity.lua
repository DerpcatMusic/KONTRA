-- Velocity probe (session B): one instrument, two groups.
--   group 1 (sampler): zone keys 0-63, root 48, a 2 s 440 Hz sine at amplitude 0.5, mode sampler.
--   group 0 (Beat):    zone keys 64-127, root 96, 1/16 at 120 BPM on the four-tone sample
--                      (6000-frame cells), mode beat_machine.
-- Notes come from beat_machine_session3_midi.py scenario "velocity": velocities 32, 64, 100, 127
-- on key 48 (0 to 6 s, sampler) and on key 96 (8 to 14 s, Beat). Holds 1.2 s. The velocity
-- law of a plain sampler is then separated from a Beat-only constant: if both zones follow the
-- same curve against our render, the gap is shared; if only the Beat zone differs, it is the
-- Beat voice. Saves velocity.nki for the native render.
local kt = Kontakt
local DIR = "Z:\\data\\dev\\workspace\\Research\\KONTRA\\scratch-beat6\\"
local out = assert(io.open(ORACLE_OUT, "w"))
local dirlog = assert(io.open(DIR .. "probe.log", "w"))
local function log(t) out:write(t .. "\n") out:flush() dirlog:write(t .. "\n") dirlog:flush() end

local function write_wav(name, frames_per_tone, tones, amp)
  local f = assert(io.open(DIR .. name, "wb"))
  local n = frames_per_tone * #tones
  local function u32(v) return string.pack("<I4", v) end
  f:write("RIFF", u32(36 + n * 4), "WAVEfmt ", u32(16), string.pack("<I2I2I4I4I2I2", 3, 1, 48000, 192000, 4, 32), "data", u32(n * 4))
  for _, hz in ipairs(tones) do
    for i = 0, frames_per_tone - 1 do
      f:write(string.pack("<f", amp * math.sin(2 * math.pi * hz * i / 48000)))
    end
  end
  f:close()
  return DIR .. name
end

local WAV_BEAT = write_wav("a_16_120.wav", 6000, { 200, 320, 440, 560 }, 0.5)
local WAV_SINE = write_wav("sine_440_2s.wav", 96000, { 440 }, 0.5)

kt.reset_multi()
local inst = kt.add_instrument()

local zb = kt.add_zone(inst, 0, WAV_BEAT)
kt.set_zone_geometry(inst, zb, { root_key = 96, low_key = 64, high_key = 127 })
local gok, gerr = pcall(kt.set_zone_grid, inst, zb, "fixed", 120.0)
log(string.format('{"beat_grid_ok":%s,"err":"%s"}', tostring(gok), tostring(gerr):gsub('"', "'"):sub(1, 120)))
local bok, berr = pcall(kt.set_group_playback_mode, inst, 0, "beat_machine")
log(string.format('{"beat_mode_ok":%s,"readback":"%s","err":"%s"}', tostring(bok),
  bok and tostring(kt.get_group_playback_mode(inst, 0)) or "", tostring(berr):gsub('"', "'"):sub(1, 120)))

local g1 = kt.add_group(inst)
local zs = kt.add_zone(inst, g1, WAV_SINE)
kt.set_zone_geometry(inst, zs, { root_key = 48, low_key = 0, high_key = 63 })
local sok, serr = pcall(kt.set_group_playback_mode, inst, g1, "sampler")
log(string.format('{"sampler_group":%d,"mode_ok":%s,"readback":"%s","err":"%s"}', g1, tostring(sok),
  sok and tostring(kt.get_group_playback_mode(inst, g1)) or "", tostring(serr):gsub('"', "'"):sub(1, 120)))

local vok, verr = pcall(kt.save_instrument, inst, DIR .. "velocity.nki", {})
log(string.format('{"save_ok":%s,"err":"%s"}', tostring(vok), tostring(verr):gsub('"', "'"):sub(1, 160)))
log('{"ready":true}')
out:close()
dirlog:close()
