-- Measured probe: what get_engine_par($ENGINE_PAR_OUTPUT_CHANNEL) answers on a group after
-- set_engine_par writes it. Encoding as in find_mod_index.lua (tune = 0.36 st per unit): a
-- read r is written as r + 1 below 1000 (so -1 reads 0) and as r - 950 from 1000 up.
-- Input: ANALOG STRINGS.nki (groups 0..4 exist).
-- Result (Kontakt 8.13.1, 2026-10-28): $NI_BUS_OFFSET = 1024 (not 1000); bus n reads back
-- 1024 + n; -1 reads -1; host output 0 reads 0. ANALOG's group 0 already sits on bus 2 (1026).
-- Also measured: $NI_SEND_BUS 0, $NI_INSERT_BUS 1, $NI_MAIN_BUS 2.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local lines = { "on init", "  declare $r" }
local function read(group)
  lines[#lines + 1] = string.format("  $r := get_engine_par($ENGINE_PAR_OUTPUT_CHANNEL, %d, -1, -1)", group)
  lines[#lines + 1] = "  if ($r < 1000)"
  lines[#lines + 1] = string.format("    set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * ($r + 1), %d, -1, -1)", group)
  lines[#lines + 1] = "  else"
  lines[#lines + 1] = string.format("    set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * ($r - 950), %d, -1, -1)", group)
  lines[#lines + 1] = "  end if"
end
local cases = {
  { "untouched", nil },
  { "set NI_BUS_OFFSET + 2", "$NI_BUS_OFFSET + 2" },
  { "set NI_BUS_OFFSET + 2, then -1", "-1" },
  { "set 0 (host output 0)", "0" },
  { "NI_BUS_OFFSET value", nil },
}
for k, c in ipairs(cases) do
  local g = k - 1
  if c[2] == "-1" then
    lines[#lines + 1] = string.format("  set_engine_par($ENGINE_PAR_OUTPUT_CHANNEL, $NI_BUS_OFFSET + 2, %d, -1, -1)", g)
  end
  if c[2] then
    lines[#lines + 1] = string.format("  set_engine_par($ENGINE_PAR_OUTPUT_CHANNEL, %s, %d, -1, -1)", c[2], g)
  end
  if k == 5 then
    lines[#lines + 1] = string.format("  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * ($NI_BUS_OFFSET - 950), %d, -1, -1)", g)
  else
    read(g)
  end
end
lines[#lines + 1] = "end on"
local src = table.concat(lines, "\n") .. "\n"
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
log(string.format('{"loaded":%s,"groups":%s}', tostring(i), tostring(kt.get_num_groups(i))))
for s = 1, 4 do pcall(kt.set_instrument_script_source, i, s, "on init\nend on\n") end
local ok, err = pcall(kt.set_instrument_script_source, i, 0, src)
log(string.format('{"compile":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'")))
local t0 = os.time()
repeat until os.time() - t0 >= 6
for k, c in ipairs(cases) do
  log(string.format('{"case":"%s","group":%d,"tune":%s}', c[1], k - 1, tostring(kt.get_group_tune(i, k - 1))))
end
log('{"done":true}')
out:close()
