-- Measured probe: the key-colour and key-type constants, and what an untouched key and a set key
-- read back. Encoding as in find_mod_index.lua: group k's tune is case k's value in 0.36 st units.
-- Input: ANALOG STRINGS.nki; slots 0 and 1 carry one half of the cases each (slots 2..4 are
-- protected), so a name Kontakt does not know fails only its own half.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local cases = {
  "$KEY_COLOR_RED", "$KEY_COLOR_ORANGE", "$KEY_COLOR_LIGHT_ORANGE", "$KEY_COLOR_WARM_YELLOW",
  "$KEY_COLOR_YELLOW", "$KEY_COLOR_LIME", "$KEY_COLOR_GREEN", "$KEY_COLOR_MINT", "$KEY_COLOR_CYAN",
  "$KEY_COLOR_TURQUOISE", "$KEY_COLOR_BLUE", "$KEY_COLOR_PLUM", "$KEY_COLOR_VIOLET",
  "$KEY_COLOR_PURPLE", "$KEY_COLOR_MAGENTA", "$KEY_COLOR_FUCHSIA",
  "$KEY_COLOR_DEFAULT", "$KEY_COLOR_INACTIVE", "$KEY_COLOR_NONE", "$KEY_COLOR_WHITE",
  "$KEY_COLOR_BLACK", "$NI_KEY_TYPE_DEFAULT", "$NI_KEY_TYPE_CONTROL", "$NI_KEY_TYPE_NONE",
  "get_key_color(127)", "get_key_type(127)",
}
local slots = { { 1, 13 }, { 14, 26 } }
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
for s = 2, 4 do pcall(kt.set_instrument_script_source, i, s, "on init\nend on\n") end
for s, range in ipairs(slots) do
  local lines = { "on init" }
  for k = range[1], range[2] do
    lines[#lines + 1] = string.format("  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * (%s), %d, -1, -1)", cases[k], k - 1)
  end
  lines[#lines + 1] = "end on"
  local ok, err = pcall(kt.set_instrument_script_source, i, s - 1, table.concat(lines, "\n") .. "\n")
  log(string.format('{"slot":%d,"compile":%s,"err":"%s"}', s - 1, tostring(ok), tostring(err):gsub('"', "'")))
end
local t0 = os.time()
repeat until os.time() - t0 >= 6
for k, c in ipairs(cases) do
  log(string.format('{"case":"%s","units":%.2f}', c, kt.get_group_tune(i, k - 1) / 0.36))
end
log('{"done":true}')
out:close()
