-- Audit batch A: stored laws for tune/pan/volume (instrument, group, zone), loops, voice groups,
-- and what Kontakt does with out-of-range values (clamp vs keep). One instrument, one zone per
-- group, every value set through the Lua API, then saved to Z:\tmp\w3\audit\levels.nki.
-- The same session logs what Kontakt's own getters return, so the stored file, the getters and
-- KONTRA's IR (ir_json) can be compared.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local DIR = "Z:\\tmp\\w3\\audit\\"
local function log(t) out:write(t .. "\n") out:flush() end
local function num(v) if type(v) == "number" and (v ~= v or v == math.huge or v == -math.huge) then return '"' .. tostring(v) .. '"' end if type(v) ~= "number" then return '"' .. tostring(v):gsub('"', "'") .. '"' end return string.format("%.9g", v) end

-- a tiny mono 16-bit wav, 4000 frames
local wav = DIR .. "probe.wav"
do
  local f = assert(io.open(wav, "wb"))
  local n = 4000
  local function u32(v) return string.pack("<I4", v) end
  f:write("RIFF", u32(36 + n * 2), "WAVEfmt ", u32(16), string.pack("<I2I2I4I4I2I2", 1, 1, 44100, 88200, 2, 16), "data", u32(n * 2))
  for i = 0, n - 1 do f:write(string.pack("<i2", math.floor(8000 * math.sin(i * 0.05)))) end
  f:close()
end

kt.reset_multi()
local inst = kt.add_instrument()
local function try(name, f, ...)
  local ok, r = pcall(f, ...)
  if not ok then log(string.format('{"call":"%s","error":"%s"}', name, tostring(r):gsub('"', "'"):sub(1, 120))) end
  return ok, r
end

-- series of (set value) for each property; out-of-range values at the ends
local tunes = { -40, -36, -12, -1, -0.5, -0.01, 0, 0.01, 0.5, 1, 7.5, 12, 36, 40 }
local pans = { -150, -100, -50, -10, 0, 10, 50, 100, 150 }
local vols = { -200, -60, -12, -6, -1, 0, 1, 6, 12, 15 }

local rows = {} -- {prop, set, group, zone}
local function add_group_zone(prop, v)
  local g = (#rows == 0) and 0 or kt.add_group(inst)
  local z = kt.add_zone(inst, g, wav)
  rows[#rows + 1] = { prop = prop, v = v, g = g, z = z }
  return g, z
end
for _, v in ipairs(tunes) do
  local g, z = add_group_zone("group_tune", v); try("set_group_tune", kt.set_group_tune, inst, g, v)
  g, z = add_group_zone("zone_tune", v); try("set_zone_tune", kt.set_zone_tune, inst, z, v)
end
for _, v in ipairs(pans) do
  local g, z = add_group_zone("group_pan", v); try("set_group_pan", kt.set_group_pan, inst, g, v)
  g, z = add_group_zone("zone_pan", v); try("set_zone_pan", kt.set_zone_pan, inst, z, v)
end
for _, v in ipairs(vols) do
  local g, z = add_group_zone("group_volume", v); try("set_group_volume", kt.set_group_volume, inst, g, v)
  g, z = add_group_zone("zone_volume", v); try("set_zone_volume", kt.set_zone_volume, inst, z, v)
end
for _, mode in ipairs({ "off", "until_end", "until_end_alt", "until_release", "until_release_alt" }) do
  local g, z = add_group_zone("loop_mode", mode)
  try("set_sample_loop_count", kt.set_sample_loop_count, inst, z, 0, 1)
  try("set_sample_loop_mode", kt.set_sample_loop_mode, inst, z, 0, mode)
  try("set_sample_loop_start", kt.set_sample_loop_start, inst, z, 0, 100)
  try("set_sample_loop_length", kt.set_sample_loop_length, inst, z, 0, 1000)
  try("set_sample_loop_xfade", kt.set_sample_loop_xfade, inst, z, 0, 200)
  try("set_sample_loop_tune", kt.set_sample_loop_tune, inst, z, 0, 0.25)
end
for _, mode in ipairs({ "sampler", "dfd", "time_machine_1", "time_machine_2", "time_machine_pro", "beat_machine", "mpc60_machine", "s1200_machine" }) do
  local g = add_group_zone("playback_mode", mode)
  try("set_group_playback_mode", kt.set_group_playback_mode, inst, g, mode)
end

-- instrument level
local ivals = { tune = { -12, -0.5, 0.5, 12 }, pan = { -100, -37, 0, 37, 100, 130 }, volume = { -60, -6, 0, 6, 12, 20 } }
-- (single instrument: sequence of set+save is done below; getters logged per value)
for _, v in ipairs(ivals.tune) do try("itune", kt.set_instrument_tune, inst, v); log(string.format('{"kind":"inst_tune","set":%s,"get":%s}', num(v), num(kt.get_instrument_tune(inst)))) end
for _, v in ipairs(ivals.pan) do try("ipan", kt.set_instrument_pan, inst, v); log(string.format('{"kind":"inst_pan","set":%s,"get":%s}', num(v), num(kt.get_instrument_pan(inst)))) end
for _, v in ipairs(ivals.volume) do try("ivol", kt.set_instrument_volume, inst, v); log(string.format('{"kind":"inst_volume","set":%s,"get":%s}', num(v), num(kt.get_instrument_volume(inst)))) end
for _, v in ipairs({ 1, 16, 128, 500, 0 }) do try("poly", kt.set_instrument_polyphony, inst, v); log(string.format('{"kind":"polyphony","set":%d,"get":%s}', v, num(kt.get_instrument_polyphony(inst)))) end
try("ipoly_final", kt.set_instrument_polyphony, inst, 24)
try("itune_final", kt.set_instrument_tune, inst, 0)
try("ipan_final", kt.set_instrument_pan, inst, 0)
try("ivol_final", kt.set_instrument_volume, inst, -9)

-- voice groups: each candidate alone (set_voice_groups replaces the table), then the stable table
local function vg_try(field, v)
  local e = { mode = "oldest", name = "p", voices = 1, fade_time = 10, prefer_released = true }
  e[field] = v
  local ok, err = pcall(kt.set_voice_groups, inst, { e })
  local g = ok and kt.get_voice_groups(inst)[1] or {}
  log(string.format('{"kind":"voice_group","field":"%s","set":%s,"ok":%s,"get":%s,"err":"%s"}', field, num(v), tostring(ok), num(g[field]), ok and "" or tostring(err):gsub('"', "'"):sub(1, 90)))
end
for _, v in ipairs({ 0, 1, 128, 1000, 1001 }) do vg_try("voices", v) end
for _, v in ipairs({ -5, 0, 10, 1000, 1001 }) do vg_try("fade_time", v) end
for _, v in ipairs({ 0, 1, 15, 16, 127 }) do vg_try("exclusive_group", v) end
for _, v in ipairs({ "any", "oldest", "lowest", "highest" }) do vg_try("mode", v) end
local vg = {}
vg[1] = { mode = "oldest", name = "a", voices = 4, fade_time = 10, prefer_released = true }
vg[2] = { mode = "lowest", name = "b", voices = 1, fade_time = 0, prefer_released = false, exclusive_group = 3 }
vg[3] = { mode = "highest", name = "c", voices = 128, fade_time = 1000, prefer_released = true }
try("set_voice_groups", kt.set_voice_groups, inst, vg)

-- getters for everything that was set (Kontakt's own view, after its clamping)
for _, r in ipairs(rows) do
  local get
  if r.prop == "group_tune" then get = kt.get_group_tune(inst, r.g)
  elseif r.prop == "zone_tune" then get = kt.get_zone_tune(inst, r.z)
  elseif r.prop == "group_pan" then get = kt.get_group_pan(inst, r.g)
  elseif r.prop == "zone_pan" then get = kt.get_zone_pan(inst, r.z)
  elseif r.prop == "group_volume" then get = kt.get_group_volume(inst, r.g)
  elseif r.prop == "zone_volume" then get = kt.get_zone_volume(inst, r.z)
  elseif r.prop == "loop_mode" then get = kt.get_sample_loop_mode(inst, r.z, 0)
  elseif r.prop == "playback_mode" then get = kt.get_group_playback_mode(inst, r.g) end
  log(string.format('{"kind":"%s","group":%d,"zone":%d,"set":%s,"get":%s}', r.prop, r.g, r.z, num(r.v), num(get)))
end
for _, v in ipairs({ 0, 36, 127, 128, -1 }) do
  local ok = try("key_switch", kt.set_instrument_options, inst, { key_switch = v })
  local ok2, o = pcall(kt.get_instrument_options, inst)
  log(string.format('{"kind":"key_switch","set":%d,"ok":%s,"get":%s}', v, tostring(ok), num(ok2 and o.key_switch or "err")))
end
try("key_switch_final", kt.set_instrument_options, inst, { key_switch = 36 })
try("save", kt.save_instrument, inst, DIR .. "levels.nki", {})
log('{"done":true}')
out:close()
