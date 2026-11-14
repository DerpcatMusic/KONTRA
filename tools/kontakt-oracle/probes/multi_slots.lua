-- Multi slot probe: what Kontakt stores for each instrument of a multi.
-- 1. Builds a multi of three empty instruments with known MIDI channels,
--    volumes, pans and outputs, reads them back and saves it as
--    ORACLE_OUT .. ".nkm" (KONTRA's reader then maps the bytes).
-- 2. Loads each ORACLE_INPUTS multi and records each instrument's
--    MIDI channel, output, volume and pan as Kontakt reports them.
-- Ends with {"done":true}. Writes no library content.

local kt = Kontakt
local file = assert(io.open(ORACLE_OUT, "w"))

local function json(v)
  local t = type(v)
  if t == "nil" then return "null"
  elseif t == "boolean" or t == "number" then return tostring(v)
  elseif t == "string" then return '"' .. v:gsub('[%c"\\]', function(c) return string.format("\\u%04x", c:byte()) end) .. '"'
  elseif t == "table" then
    local out = {}
    for k, val in pairs(v) do out[#out + 1] = json(tostring(k)) .. ":" .. json(val) end
    return "{" .. table.concat(out, ",") .. "}"
  end
  return json(tostring(v))
end

local function emit(t)
  file:write(json(t) .. "\n")
  file:flush()
end

local function try(f, ...)
  local ok, v = pcall(f, ...)
  if ok then return v end
  return "error: " .. tostring(v)
end

local function slots(label)
  local indices = try(kt.get_instrument_indices)
  if type(indices) ~= "table" then
    emit({ label = label, error = indices })
    return
  end
  for _, i in ipairs(indices) do
    emit({
      label = label,
      index = i,
      name = try(kt.get_instrument_name, i),
      midi = try(kt.get_instrument_midi_channel, i),
      output = try(kt.get_instrument_output_channel, i),
      volume = try(kt.get_instrument_volume, i),
      pan = try(kt.get_instrument_pan, i),
    })
  end
end

-- 1. A known multi.
for _, i in ipairs(try(kt.get_instrument_indices) or {}) do
  try(kt.remove_instrument, i)
end
local made = {
  { midi = 0, volume = -6.0, pan = -50.0, output = 0 },
  { midi = 3, volume = 0.0, pan = 0.0, output = 1 },
  { midi = 18, volume = 3.5, pan = 25.0, output = 0 },
}
for n, want in ipairs(made) do
  -- Instrument indices are slot * 128 (measured: a multi's slots read 0, 128, 256).
  local i = try(kt.add_instrument, (n - 1) * 128)
  emit({ step = "add", want = (n - 1) * 128, got = i })
  if type(i) == "number" then
    emit({ step = "set", index = i,
      midi = try(kt.set_instrument_midi_channel, i, want.midi),
      volume = try(kt.set_instrument_volume, i, want.volume),
      pan = try(kt.set_instrument_pan, i, want.pan),
      output = try(kt.set_instrument_output_channel, i, want.output) })
  end
end
slots("made")
emit({ step = "save", result = try(kt.save_multi, ORACLE_OUT .. ".nkm", {}) })

-- 2. The multis given.
for _, path in ipairs(ORACLE_INPUTS) do
  local loaded = try(kt.load_multi, path)
  emit({ step = "load", path = path, result = loaded })
  slots(path)
end

file:write('{"done":true}\n')
file:close()
