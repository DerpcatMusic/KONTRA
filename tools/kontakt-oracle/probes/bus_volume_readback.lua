-- Reads ANALOG STRINGS' group, instrument and bus VOLUME through get_engine_par in slot 1, three
-- base-100 digits per value in group tunes 460+. Kontakt 8.13.1, 2026-10-28: groups 88 and 139
-- 680395 and 629883, instrument 631027, buses 2 and 3 (1026, 1027) 593624 and 525135.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
local reads = {
  "get_engine_par($ENGINE_PAR_VOLUME, 88, -1, -1)", "get_engine_par($ENGINE_PAR_VOLUME, 139, -1, -1)",
  "get_engine_par($ENGINE_PAR_VOLUME, -1, -1, -1)", "get_engine_par($ENGINE_PAR_VOLUME, -1, -1, 1026)",
  "get_engine_par($ENGINE_PAR_VOLUME, -1, -1, 1027)",
}
local src = {"on init", "  declare $v"}
for k, expr in ipairs(reads) do
  table.insert(src, "  $v := " .. expr)
  for d = 0, 2 do
    table.insert(src, string.format("  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * (($v / %d) mod 100), %d, -1, -1)", 100 ^ d // 1, 460 + 3 * (k - 1) + d))
  end
end
table.insert(src, "end on")
local ok, err = pcall(kt.set_instrument_script_source, i, 1, table.concat(src, "\n") .. "\n")
log(string.format('{"ok":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'")))
local t0 = os.time() repeat until os.time() - t0 >= 8
for k = 1, #reads do
  local v = 0
  for d = 0, 2 do v = v + math.floor(kt.get_group_tune(i, 460 + 3 * (k - 1) + d) / 0.36 + 0.5) * 100 ^ d end
  log(string.format('{"read":"%s","value":%d}', reads[k]:gsub("%$", ""), v))
end
out:close()
