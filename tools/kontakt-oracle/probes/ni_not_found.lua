-- Measured probe: the value of $NI_NOT_FOUND and what lookups answer for a miss.
-- Encoding as in find_mod_index.lua (tune = 0.36 st per unit; a true condition writes 1).
-- Result (Kontakt 8.13.1, 2026-10-28): $NI_NOT_FOUND = -100; get_mod_idx and get_target_idx
-- answer it for a miss, get_group_idx too. find_group and find_mod answer 0 for a miss.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local lines = { "on init", "  declare $m", "  $m := get_mod_idx(0, \"no_such_mod\")" }
local function enc(group, expr)
  lines[#lines + 1] = string.format("  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * (%s), %d, -1, -1)", expr, group)
end
enc(0, "0") enc(1, "10") enc(2, "20")
local cases = {
  { "NI_NOT_FOUND = -100", "$NI_NOT_FOUND = -100" }, { "miss = -100", "$m = -100" },
  { "target miss = -100", 'get_target_idx(0, 3, \"nope\") = -100' }, { "NI_NOT_FOUND mod 1000", "$NI_NOT_FOUND mod 1000" },
}
for k, c in ipairs(cases) do
  local e = c[2]
  if e:find(" = ") or e:find(" < ") then
    lines[#lines + 1] = "  if (" .. e .. ")"
    enc(k + 2, "1")
    lines[#lines + 1] = "  else"
    enc(k + 2, "0")
    lines[#lines + 1] = "  end if"
  else
    enc(k + 2, e)
  end
end
local g = #cases + 3
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
for k = 0, g - 1 do
  log(string.format('{"group":%d,"tune":%s}', k, tostring(kt.get_group_tune(i, k))))
end
for _, c in ipairs(cases) do
  log(string.format('{"case":"%s","group":%d}', c[1], _ + 2))
end
log('{"done":true}')
out:close()
