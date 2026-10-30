-- Values of the constants `mf_set_event_par`/`mf_get_event_par` take.
--
-- The manual lists $EVENT_PAR_POS, $EVENT_PAR_NOTE_LENGTH, $EVENT_PAR_ID and
-- $EVENT_PAR_TRACK_NR but gives no numbers, and KONTRA's compiler treats them
-- as opaque catalog names. A KSP `on init` writes each constant's value into a
-- group's tune, four digits at a time, and the Lua side reads the groups back.
--
--   ORACLE_LUA=probes/event_par_values.lua tools/kontakt-oracle/dump.py OUT.jsonl
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(s) out:write(s .. "\n") out:flush() end

local NAMES = { "$EVENT_PAR_POS", "$EVENT_PAR_NOTE_LENGTH", "$EVENT_PAR_ID", "$EVENT_PAR_TRACK_NR" }
local GROUPS = #NAMES * 4

kt.reset_multi()
local inst = kt.add_instrument()
for _ = 2, GROUPS do kt.add_group(inst) end

local src = { "on init", "  set_engine_par($ENGINE_PAR_TUNE, 900000, 15, -1, -1)" }
for n, name in ipairs(NAMES) do
  local v = string.format("$v%d", n)
  table.insert(src, string.format("  %s := %s", v, name))
  for d = 0, 3 do
    local g = (n - 1) * 4 + d
    table.insert(src, string.format(
      "  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * ((%s / %d) mod 100), %d, -1, -1)",
      v, 100 ^ d, g))
  end
end
table.insert(src, "end on")
local text = table.concat(src, "\n")
kt.set_instrument_script_name(inst, 0, "eventpars")
kt.set_instrument_script_source(inst, 0, text)
kt.set_instrument_script_bypassed(inst, 0, true)
kt.set_instrument_script_bypassed(inst, 0, false)

log('{"script":' .. string.format("%q", text):gsub("\n", "\\n") .. '}')
for n, name in ipairs(NAMES) do
  local value = 0
  for d = 0, 3 do
    local g = (n - 1) * 4 + d
    local t = kt.get_group_tune(inst, g) * 1000000
    log(string.format('{"raw":%d,"group":%d,"tune":%s}', n, g, tostring(t)))
    local digit = math.floor((t - 500000) / 5000 + 0.5)
    value = value + digit * 100 ^ d
  end
  log(string.format('{"name":"%s","value":%d}', name, value))
end
log('{"done":true}')
out:close()
