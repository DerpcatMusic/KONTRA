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

-- Where ORACLE_OUT lives, separator included: the script under test is written
-- beside the probe's own output, because set_instrument_script_source takes a
-- file path (see below).
local DIR = ORACLE_OUT:match("^(.*[\\/])") or ""

local NAMES = { "$EVENT_PAR_POS", "$EVENT_PAR_NOTE_LENGTH", "$EVENT_PAR_ID", "$EVENT_PAR_TRACK_NR" }
local DIGITS = 4
local GROUPS = #NAMES * DIGITS
-- One extra group, written last and only by the script: its tune says whether
-- the script ran at all, so an unchanged instrument cannot be read as four
-- constants whose digits all happen to be zero.
local MARKER = GROUPS

kt.reset_multi()
local inst = kt.add_instrument()
for _ = 2, MARKER + 1 do kt.add_group(inst) end

-- KSP: a user variable must be `declare`d in `on init` (an undeclared one fails
-- to compile, and set_instrument_script_source still reports success), so each
-- constant is read into a declared variable. Its value then goes into four
-- groups' tune as base-10 digits, 5000 apart around the neutral 500000.
local src = { "on init" }
for n, name in ipairs(NAMES) do
  local v = string.format("$v%d", n)
  table.insert(src, string.format("  declare %s := %s", v, name))
  for d = 0, DIGITS - 1 do
    local g = (n - 1) * DIGITS + d
    table.insert(src, string.format(
      "  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * ((%s / %d) mod 100), %d, -1, -1)",
      v, 100 ^ d, g))
  end
end
table.insert(src, string.format(
  "  set_engine_par($ENGINE_PAR_TUNE, 900000, %d, -1, -1)", MARKER))
table.insert(src, "end on")
local text = table.concat(src, "\n") .. "\n"

-- The API takes an absolute path, not the source: "Sets the script source by
-- passing an absolute filepath as argument" (Kontakt Lua API reference,
-- set_instrument_script_source). The text is written next to the probe output
-- and logged, so a reader can see exactly what was compiled.
local script = DIR .. "eventpars.ksp"
local fh = assert(io.open(script, "w"))
fh:write(text)
fh:close()

kt.set_instrument_script_name(inst, 0, "eventpars")
local ok, err = pcall(kt.set_instrument_script_source, inst, 0, script)
log('{"script":' .. string.format("%q", text):gsub("\n", "\\n") ..
    ',"script_path":' .. string.format("%q", script) ..
    ',"script_set":' .. tostring(ok) ..
    ',"script_error":' .. (ok and 'null' or string.format("%q", tostring(err))) .. '}')
kt.set_instrument_script_bypassed(inst, 0, true)
kt.set_instrument_script_bypassed(inst, 0, false)

-- get_group_tune returns semitones (-36..36), not the 0..1000000 value
-- set_engine_par takes. Invert the measured law -- raw = st / 7.2e-5 + 500000,
-- ±36 st at the ends (crates/engine/plan/src/par_values.rs, LevelPar::Tune) --
-- or a neutral group would decode as -100 instead of 0.
local function normalized(semitones) return semitones / 7.2e-5 + 500000 end

local marker = normalized(kt.get_group_tune(inst, MARKER))
log(string.format('{"marker_group":%d,"marker_tune":%.3f,"script_ran":%s}',
                  MARKER, marker, tostring(math.abs(marker - 900000) < 1000)))

for n, name in ipairs(NAMES) do
  local value = 0
  for d = 0, DIGITS - 1 do
    local g = (n - 1) * DIGITS + d
    local t = normalized(kt.get_group_tune(inst, g))
    log(string.format('{"raw":%d,"group":%d,"tune":%.3f}', n, g, t))
    local digit = math.floor((t - 500000) / 5000 + 0.5)
    value = value + digit * 100 ^ d
  end
  log(string.format('{"name":"%s","value":%d}', name, value))
end
log('{"done":true}')
out:close()
