-- Kontakt 8.13 release-trigger laws (run with gui_session.py; Release Trigger is switched on in the
-- editor for group 1: no Lua or KSP route sets it, KSP RELEASE_TRIGGER writes read back 0).
-- Instrument polyphony 3. Group 0 (normal, default envelope, panned left): keys 60..64, a 12 s sine.
-- Group 1 (release trigger, panned right, voice group "oldest" 2 voices, no envelope): keys 60..69,
-- a 2 s one-shot sine. On both, key 60 + k sounds 261.6 * 2^(5k/12) Hz. Real MIDI notes (rt.mid).
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local HERE = "Z:\\data\\dev\\workspace\\BuildScratch\\voices\\probes2\\"
local function main()
  kt.reset_multi()
  local i = kt.add_instrument()
  kt.set_instrument_midi_channel(i, ORACLE_CHANNEL or 1)
  kt.set_instrument_polyphony(i, 3)
  local g = kt.add_group(i)
  for k = 0, 9 do
    if k <= 4 then
      local z = kt.add_zone(i, 0, HERE .. "sine.wav")
      kt.set_zone_geometry(i, z, { root_key = 60 - 4 * k, low_key = 60 + k, high_key = 60 + k })
    end
    local z = kt.add_zone(i, g, HERE .. "rt2s.wav")
    kt.set_zone_geometry(i, z, { root_key = 60 - 4 * k, low_key = 60 + k, high_key = 60 + k })
  end
  kt.set_group_pan(i, 0, -100)
  kt.set_group_pan(i, g, 100)
  kt.set_group_name(i, g, "RT")
  kt.set_voice_group(i, g, 1)
  log(tostring(pcall(kt.set_voice_groups, i, { { mode = "oldest", voices = 2, fade_time = 10, prefer_released = false } })))
  local o = kt.get_instrument_options(i)
  log(string.format('{"poly":%s,"steal":"%s","fade":%s,"vg":%s}', kt.get_instrument_polyphony(i),
    tostring(o.voice_stealing_mode), tostring(o.voice_stealing_fadeout), tostring(kt.get_voice_group(i, g))))
end
local ok, err = xpcall(main, debug and debug.traceback or tostring)
if not ok then log('{"error":"' .. tostring(err):gsub('"', "'"):gsub("\n", " | ") .. '"}') end
log('{"ready":true}')
out:close()
