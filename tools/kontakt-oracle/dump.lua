-- Native state dump: runs INSIDE Kontakt 8 (Options > Developer enabled) through the
-- Kontakt Lua API (Kontakt 8 API Reference, en 260924). dump.py prepends two globals:
--   ORACLE_INPUTS = { "C:\\...\\a.nki", ... }   ORACLE_OUT = "C:\\...\\out.jsonl"
-- One JSON line per instrument. Every API call goes through pcall, so an API that
-- refuses (protected content, missing feature) is recorded as {"error": ...} instead
-- of aborting. Script sources are NEVER written: only length + FNV-1a 64 hash, so the
-- dump can be compared with KONTRA's decrypted source without storing library code.

local kt = Kontakt

local function fnv1a64(s)
  local h = -3750763034362895579 -- 0xcbf29ce484222325 as a signed 64-bit integer
  for i = 1, #s do
    h = h ~ s:byte(i)
    h = h * 1099511628211
  end
  return string.format("%016x", h)
end

local function json(v)
  local t = type(v)
  if t == "nil" then return "null"
  elseif t == "boolean" then return tostring(v)
  elseif t == "number" then
    if v ~= v or v == math.huge or v == -math.huge then return '"' .. tostring(v) .. '"' end
    if math.type and math.type(v) == "integer" then return tostring(v) end
    return string.format("%.17g", v)
  elseif t == "string" then
    return '"' .. v:gsub('[%c"\\]', function(c)
      return string.format("\\u%04x", c:byte())
    end) .. '"'
  elseif t == "table" then
    local n = #v
    local is_array = n > 0 or next(v) == nil
    if is_array then
      for k in pairs(v) do
        if math.type(k) ~= "integer" or k < 1 or k > n then is_array = false break end
      end
    end
    local out = {}
    if is_array then
      for i = 1, n do out[#out + 1] = json(v[i]) end
      return "[" .. table.concat(out, ",") .. "]"
    end
    local keys = {}
    for k in pairs(v) do keys[#keys + 1] = tostring(k) end
    table.sort(keys)
    for _, k in ipairs(keys) do
      local val = v[k]
      if val == nil then val = v[tonumber(k)] end
      out[#out + 1] = json(k) .. ":" .. json(val)
    end
    return "{" .. table.concat(out, ",") .. "}"
  end
  return json(tostring(v))
end

-- call(f, ...) -> value or {error = message}
local function call(name, ...)
  local f = kt[name]
  if f == nil then return { error = "missing api " .. name } end
  local ok, a, b = pcall(f, ...)
  if not ok then return { error = tostring(a) } end
  if b ~= nil then return { a, b } end
  return a
end

local function dump_instrument(i)
  local inst = {
    name = call("get_instrument_name", i),
    options = call("get_instrument_options", i),
    volume_db = call("get_instrument_volume", i),
    pan = call("get_instrument_pan", i),
    tune = call("get_instrument_tune", i),
    polyphony = call("get_instrument_polyphony", i),
    midi_channel = call("get_instrument_midi_channel", i),
    output_channel = call("get_instrument_output_channel", i),
    voice_groups = call("get_voice_groups", i),
    scripts = {},
    groups = {},
    zones = {},
  }
  for s = 0, 4 do
    local src = call("get_instrument_script_source", i, s)
    inst.scripts[#inst.scripts + 1] = {
      slot = s,
      name = call("get_instrument_script_name", i, s),
      bypassed = call("is_instrument_script_bypassed", i, s),
      protected = call("is_instrument_script_protected", i, s),
      linked = call("is_instrument_script_linked", i, s),
      source_len = type(src) == "string" and #src or nil,
      source_fnv64 = type(src) == "string" and #src > 0 and fnv1a64(src) or nil,
      source_error = type(src) == "table" and src.error or nil,
    }
  end
  local ng = call("get_num_groups", i)
  inst.num_groups = ng
  if math.type(ng) == "integer" then
    for g = 0, ng - 1 do
      inst.groups[#inst.groups + 1] = {
        index = g,
        name = call("get_group_name", i, g),
        volume_db = call("get_group_volume", i, g),
        pan = call("get_group_pan", i, g),
        tune = call("get_group_tune", i, g),
        playback_mode = call("get_group_playback_mode", i, g),
        start_options = call("get_group_start_options", i, g),
        voice_group = call("get_voice_group", i, g),
      }
    end
  end
  local nz = call("get_num_zones", i)
  inst.num_zones = nz
  if math.type(nz) == "integer" then
    for z = 0, nz - 1 do
      local zone = {
        index = z,
        group = call("get_zone_group", i, z),
        geometry = call("get_zone_geometry", i, z),
        sample = call("get_zone_sample", i, z),
        sample_frames = call("get_zone_sample_frames", i, z),
        sample_channels = call("get_zone_sample_channels", i, z),
        sample_start = call("get_zone_sample_start", i, z),
        sample_end = call("get_zone_sample_end", i, z),
        sample_start_mod_range = call("get_zone_sample_start_mod_range", i, z),
        volume_db = call("get_zone_volume", i, z),
        pan = call("get_zone_pan", i, z),
        tune = call("get_zone_tune", i, z),
        grid_mode = call("get_zone_grid_mode", i, z),
        grid_bpm = call("get_zone_grid_bpm", i, z),
        loops = {},
      }
      for l = 0, 7 do
        local mode = call("get_sample_loop_mode", i, z, l)
        if type(mode) == "table" and mode.error then break end
        zone.loops[#zone.loops + 1] = {
          mode = mode,
          start = call("get_sample_loop_start", i, z, l),
          length = call("get_sample_loop_length", i, z, l),
          xfade = call("get_sample_loop_xfade", i, z, l),
          count = call("get_sample_loop_count", i, z, l),
          tune = call("get_sample_loop_tune", i, z, l),
        }
      end
      inst.zones[#inst.zones + 1] = zone
    end
  end
  return inst
end

local out = assert(io.open(ORACLE_OUT, "w"))
for _, path in ipairs(ORACLE_INPUTS) do
  call("reset_multi")
  local record = { input = path, kontakt_version = kt.version }
  local idx = call("load_instrument", path)
  if math.type(idx) == "integer" then
    record.instrument = dump_instrument(idx)
  else
    record.load_error = idx
  end
  out:write(json(record), "\n")
  out:flush()
end
out:write('{"done":true}\n')
out:close()
print("ORACLE DONE")
