-- Kontakt's voice limits, measured: voice group modes, prefer-released, fade time and exclusive
-- groups, and the instrument's own polyphony and stealing mode. Run with render.py (trigger note 36).
-- Every zone is a 12 s 261.6256 Hz sine; key base + k of a case sounds 5k semitones above it, so
-- each key has its own frequency in the recording. Cases are apart by silence (1.2 s).
-- Each case plays keys +1, +0, +4, +2 (oldest .. newest; +4 highest, +0 lowest) and then +3,
-- which needs a fifth voice where four are allowed. Cases run one after another, CASE_S apart.
-- The patches are also saved (DIR/voice_limits_*.nki) with distinct values to find the fields.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local dir = ORACLE_OUT:match("^(.*[\\/])")
local WAV = dir .. "sine.wav"
local f = assert(io.open(WAV, "wb"))
local rate, n = 48000, 48000 * 12
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

local CASE_S = 3.5
local SLOW = 850000
-- Key base + k sounds 5k semitones above 261.6 Hz (a fourth apart), so 40 ms frames tell them apart.
local function zones(inst, g, base)
  for k = 0, 4 do
    local z = kt.add_zone(inst, g, WAV)
    kt.set_zone_geometry(inst, z, { root_key = base - 4 * k, low_key = base + k, high_key = base + k })
  end
end
local function pattern(base, at, extra)
  -- {seconds, kind ("on"/"off"), key, velocity}
  local e = {
    {at + 0.0, "on", base + 1, 100}, {at + 0.1, "on", base + 0, 100}, {at + 0.2, "on", base + 4, 100},
    {at + 0.3, "on", base + 2, 100}, {at + 0.6, "on", base + 3, 100},
  }
  for _, x in ipairs(extra or {}) do e[#e + 1] = x end
  for k = 0, 4 do e[#e + 1] = {at + 1.6, "off", base + k, 0} end
  return e
end

-- KSP that plays a list of timed events when the trigger arrives; other notes are ignored.
local function script(events, release)
  table.sort(events, function(a, b) return a[1] < b[1] end)
  local decl = { "on init", "  declare %ids[128]", "  declare $s", "  declare $n := " .. #events,
    "  declare %t[" .. #events .. "]", "  declare %k[" .. #events .. "]", "  declare %v[" .. #events .. "]",
    "  declare %on[" .. #events .. "]", "  declare $g", "  declare $m", "  declare $w" }
  if release then
    -- The volume envelope releases slowly (ENGINE_PAR_RELEASE `release`).
    for _, l in ipairs({ "  $g := 0", "  while ($g < $NUM_GROUPS)", "    $m := 0", "    while ($m < 16)",
      "      if (get_engine_par($ENGINE_PAR_INTMOD_TYPE, $g, $m, -1) = $INTMOD_TYPE_ENVELOPE)",
      "        set_engine_par($ENGINE_PAR_RELEASE, " .. release .. ", $g, $m, -1)", "      end if",
      "      inc($m)", "    end while", "    inc($g)", "  end while" }) do decl[#decl + 1] = l end
  end
  for i, e in ipairs(events) do
    decl[#decl + 1] = string.format("  %%t[%d] := %d", i - 1, math.floor(e[1] * 1000000 + 0.5))
    decl[#decl + 1] = string.format("  %%k[%d] := %d", i - 1, e[3])
    decl[#decl + 1] = string.format("  %%v[%d] := %d", i - 1, e[4])
    decl[#decl + 1] = string.format("  %%on[%d] := %d", i - 1, e[2] == "on" and 1 or 0)
  end
  return table.concat(decl, "\n") .. [==[

end on
on note
  ignore_event($EVENT_ID)
  if ($EVENT_NOTE = 36)
    $s := 0
    while ($s < $n)
      if ($s = 0)
        $w := %t[0] + 1
      else
        $w := %t[$s] - %t[$s - 1]
      end if
      { One long wait() misfires (every instrument started together): wait in 1 s steps. }
      while ($w > 1000000)
        wait(1000000)
        $w := $w - 1000000
      end while
      if ($w > 0)
        wait($w)
      end if
      if (%on[$s] = 1)
        %ids[%k[$s]] := play_note(%k[$s], %v[$s], 0, 0)
      else
        note_off(%ids[%k[$s]])
      end if
      inc($s)
    end while
  end if
end on
]==]
end

local function main()
kt.reset_multi()

-- Each case is its own instrument: its group 0 has the default volume envelope (groups added
-- by add_group have none, and their notes stop within 20 ms of note-off), and the prefer-released
-- cases release slowly, so a released voice still sounds when the fifth note needs its place.
local cases = {
  {mode = "oldest"}, {mode = "newest"}, {mode = "highest"}, {mode = "lowest"}, {mode = "any"},
  {mode = "oldest", prefer_released = true, release = true},
  {mode = "oldest", prefer_released = false, release = true},
  {mode = "oldest", fade_time = 300},
  {mode = "any", prefer_released = true, release = true},
  {mode = "any", prefer_released = false, release = true},
  {mode = "any", quiet = true},
  {mode = "oldest", exclusive = 1},
}
for c, case in ipairs(cases) do
  local i = kt.add_instrument()
  kt.set_instrument_midi_channel(i, ORACLE_CHANNEL or 1)
  kt.set_instrument_polyphony(i, 128)
  zones(i, 0, 60)
  local at = (c - 1) * CASE_S
  local groups = { { mode = case.mode, voices = 4, fade_time = case.fade_time or 10,
    prefer_released = case.prefer_released or false, exclusive_group = case.exclusive } }
  kt.set_voice_group(i, 0, 1)
  local events
  if case.exclusive then
    -- A second group (voice group 2, same exclusive group) on keys 70..74; its +1 (349 Hz)
    -- starts while group 0's +0 (261.6 Hz) sounds.
    local g = kt.add_group(i)
    zones(i, g, 70)
    kt.set_voice_group(i, g, 2)
    groups[2] = { mode = "oldest", voices = 4, fade_time = 10, prefer_released = false, exclusive_group = 1 }
    events = { {at, "on", 60, 100}, {at + 0.6, "on", 71, 100}, {at + 1.6, "off", 60, 0}, {at + 1.6, "off", 71, 0} }
  else
    local extra = {}
    if case.release then extra[1] = {at + 0.55, "off", 64, 0} end
    events = pattern(60, at, extra)
    -- The newest note (+2) very quiet: does "any" take the quietest rather than the oldest?
    if case.quiet then events[4][4] = 1 end
  end
  local ok, err = pcall(kt.set_voice_groups, i, groups)
  local v = kt.get_voice_groups(i)[1]
  log(string.format('{"case":%d,"ok":%s,"mode":"%s","voices":%s,"fade":%s,"pref":%s,"excl":%s}', c, tostring(ok),
    tostring(v and v.mode), tostring(v and v.voices), tostring(v and v.fade_time), tostring(v and v.prefer_released),
    tostring(v and v.exclusive_group)))
  log(tostring(pcall(kt.set_instrument_script_source, i, 0, script(events, case.release and SLOW or nil))))
  if c == 1 then log('{"saved_vg":' .. tostring(pcall(kt.save_instrument, i, dir .. "voice_limits_vg.nki", {})) .. '}') end
end

-- Instruments 1..5: the instrument's own limit, 4 voices, one stealing mode each, after the cases.
local modes = { "oldest", "newest", "highest", "lowest", "any" }
for m, mode in ipairs(modes) do
  local i = kt.add_instrument()
  kt.set_instrument_midi_channel(i, ORACLE_CHANNEL or 1)
  zones(i, 0, 60)
  -- Saved first with distinct values (polyphony 7 + m, fadeout 30 + m) to find the fields.
  kt.set_instrument_polyphony(i, 7 + m)
  pcall(kt.set_instrument_options, i, { voice_stealing_mode = mode, voice_stealing_fadeout = 30 + m })
  log(string.format('{"saved_inst":"%s","ok":%s}', mode,
    tostring(pcall(kt.save_instrument, i, dir .. "voice_limits_inst_" .. mode .. ".nki", {}))))
  kt.set_instrument_polyphony(i, 4)
  pcall(kt.set_instrument_options, i, { voice_stealing_mode = mode, voice_stealing_fadeout = 10 })
  local o = kt.get_instrument_options(i)
  log(string.format('{"inst":"%s","polyphony":%s,"mode":"%s","fade":%s}', mode,
    tostring(kt.get_instrument_polyphony(i)), tostring(o.voice_stealing_mode), tostring(o.voice_stealing_fadeout)))
  local at = (#cases + m - 1) * CASE_S
  log(tostring(pcall(kt.set_instrument_script_source, i, 0, script(pattern(60, at)))))
end
log('{"cases":' .. #cases .. ',"case_s":' .. CASE_S .. ',"total_s":' .. (#cases + #modes) * CASE_S .. '}')
end
local ok, err = xpcall(main, debug and debug.traceback or tostring)
if not ok then log('{"error":"' .. tostring(err):gsub('"', "'"):gsub("\n", " | ") .. '"}') end
log('{"ready":true}')
out:close()
