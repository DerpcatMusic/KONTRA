-- KSP text probe: runs INSIDE Kontakt 8 (DeveloperFeatures=dw:1) through the Lua API.
-- oracle-mcp's oracle_dump_script prepends two globals (dump.py's contract):
--   ORACLE_INPUTS = { "Z:\\...\\a.nki" }   ORACLE_OUT = "Z:\\...\\out.jsonl"
-- One JSON line per input, last line {"done":true}.
--
-- Where dump.lua deliberately writes only a length + hash of each script, this probe
-- writes the TEXT: the point is to diff Kontakt's own copy of a script against
-- KONTRA's, so the caller can see the difference rather than two hashes.

local kt = Kontakt

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

local function call(name, ...)
  local f = kt[name]
  if f == nil then return { error = "missing api " .. name } end
  local ok, a, b = pcall(f, ...)
  if not ok then return { error = tostring(a) } end
  if b ~= nil then return { a, b } end
  return a
end

local out = assert(io.open(ORACLE_OUT, "w"))
for _, path in ipairs(ORACLE_INPUTS) do
  call("reset_multi")
  local record = { input = path, kontakt_version = kt.version }
  local idx = call("load_instrument", path)
  if math.type(idx) == "integer" then
    local inst = {
      index = idx,
      name = call("get_instrument_name", idx),
      scripts = {},
    }
    for s = 0, 4 do
      local src = call("get_instrument_script_source", idx, s)
      inst.scripts[#inst.scripts + 1] = {
        slot = s,
        name = call("get_instrument_script_name", idx, s),
        protected = call("is_instrument_script_protected", idx, s),
        bypassed = call("is_instrument_script_bypassed", idx, s),
        source = type(src) == "string" and src or nil,
        source_error = type(src) == "table" and src.error or nil,
      }
    end
    record.instrument = inst
  else
    record.load_error = idx
  end
  out:write(json(record), "\n")
  out:flush()
end
out:write('{"done":true}\n')
out:close()
print("ORACLE KSP DONE")
