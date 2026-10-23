-- Calibration of the bit channel. Readback of a KSP result is carried by group volumes:
-- bit b of a result is written as set_engine_par(ENGINE_PAR_VOLUME, 1000, group=b, -1, -1)
-- and read back as get_group_volume(i, b). A clear bit leaves the group at its default
-- (0 dB); a set bit must read back as a distinct, finite level. This probe checks the
-- two states on group addresses 0..3 and that the group index is 0-based.
-- Run through tools/kontakt-oracle/dump.py with ORACLE_LUA (see run notes in
-- docs/re/KSP_CONFORMANCE.md).
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
kt.reset_multi()

local CASES = {
  { "set_g0_1000", "  set_engine_par($ENGINE_PAR_VOLUME, 1000, 0, -1, -1)\n" },
  { "set_g1_1000", "  set_engine_par($ENGINE_PAR_VOLUME, 1000, 1, -1, -1)\n" },
  { "set_g2_2000", "  set_engine_par($ENGINE_PAR_VOLUME, 2000, 2, -1, -1)\n" },
  { "set_g3_0", "  set_engine_par($ENGINE_PAR_VOLUME, 0, 3, -1, -1)\n" },
  { "none", "" },
}

local ids = {}
for n, c in ipairs(CASES) do
  local i = kt.add_instrument()
  kt.set_instrument_volume(i, 0)
  for _ = 1, 3 do kt.add_group(i) end
  ids[n] = i
  local src = "on init\n" .. c[2] .. "end on\n"
  local ok, err = pcall(kt.set_instrument_script_source, i, 0, src)
  log(string.format('{"compile":"%s","ok":%s,"err":"%s"}', c[1], tostring(ok), tostring(err or "")))
end

local t0 = os.time()
repeat until os.time() - t0 >= 4
for n, c in ipairs(CASES) do
  local groups = {}
  for g = 0, 3 do
    groups[#groups + 1] = string.format("%.9g", kt.get_group_volume(ids[n], g))
  end
  log(string.format('{"case":"%s","group_volume_db":[%s]}', c[1], table.concat(groups, ",")))
end
log('{"done":true}')
out:close()
