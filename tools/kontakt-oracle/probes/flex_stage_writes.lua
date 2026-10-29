-- Does a flex envelope (Dolce ENV_FLEX, group 0 slot 0) take $ENGINE_PAR_ATTACK / SUSTAIN?
-- Reads are written to group tunes as 500000 + 5000*(v/10000) + 5000 (0.36 st per unit; 1 = read 0).
-- Input: Audio Imperia Dolce "Dolce - 01 7 1st Violins - Legato.nki" (240 groups, free slot 4).
-- Result (Kontakt 8.13.1, 2026-10-28): every read is 0. ATTACK before and after writing 123456,
-- SUSTAIN before and after 777777: a flex envelope has no stages and keeps its points.
-- find_mod(0, "ENV_FLEX") = 0 and find_mod(0, "ENV_AHDSR") (absent) = 0, so Dolce's stage writes to
-- "ENV_AHDSR" land on the flex in slot 0 and do nothing.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
log(string.format('{"groups":%s}', tostring(kt.get_num_groups(i))))
local function enc(expr, g) return string.format("  set_engine_par($ENGINE_PAR_TUNE, 505000 + 5000 * ((%s) / 10000), %d, -1, -1)", expr, g) end
local src = table.concat({
  "on init",
  enc("get_engine_par($ENGINE_PAR_ATTACK, 0, 0, -1)", 1),
  "  set_engine_par($ENGINE_PAR_ATTACK, 123456, 0, 0, -1)",
  enc("get_engine_par($ENGINE_PAR_ATTACK, 0, 0, -1)", 2),
  enc("get_engine_par($ENGINE_PAR_SUSTAIN, 0, 0, -1)", 3),
  "  set_engine_par($ENGINE_PAR_SUSTAIN, 777777, 0, 0, -1)",
  enc("get_engine_par($ENGINE_PAR_SUSTAIN, 0, 0, -1)", 4),
  enc("find_mod(0, \"ENV_FLEX\")", 5),
  enc("find_mod(0, \"ENV_AHDSR\")", 6),
  "  set_engine_par($ENGINE_PAR_TUNE, 600000, 7, -1, -1)",
  "end on", ""}, "\n")
for _, sl in ipairs({4, 3}) do
  local ok, err = pcall(kt.set_instrument_script_source, i, sl, src)
  log(string.format('{"slot":%d,"ok":%s,"err":"%s"}', sl, tostring(ok), tostring(err):gsub('"', "'")))
  local t0 = os.time() repeat until os.time() - t0 >= 6
  if kt.get_group_tune(i, 7) > 1 then break end
end
for g = 1, 7 do log(string.format('{"g":%d,"units":%.2f}', g, kt.get_group_tune(i, g) / 0.36)) end
log('{"done":true}')
out:close()
