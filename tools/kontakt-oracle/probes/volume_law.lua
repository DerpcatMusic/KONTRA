-- Probe: which stored value does Kontakt write for a group/zone/instrument volume in dB?
-- Creates one scratch instrument, sets known dB values, saves it next to ORACLE_OUT.
-- KONTRA's reader then reads the raw stored floats (tools/kontakt-oracle/volume_law.py).
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
kt.reset_multi()
local i = kt.add_instrument()
local values = { -60, -36, -30, -24, -16, -12, -6, -3, -1, 0, 1, 3, 6, 12 }
for n, v in ipairs(values) do
  local g = n == 1 and 0 or kt.add_group(i)
  kt.set_group_volume(i, g, v)
  out:write(string.format('{"kind":"group","index":%d,"set_db":%g,"get_db":%.9g}\n', g, v, kt.get_group_volume(i, g)))
end
kt.set_instrument_volume(i, -16)
out:write(string.format('{"kind":"instrument","set_db":-16,"get_db":%.9g}\n', kt.get_instrument_volume(i)))
local path = ORACLE_OUT:gsub("[^\\/]+$", "") .. "volume_law.nki"
kt.save_instrument(i, path, {})
out:write(string.format('{"saved":"%s"}\n', path:gsub("\\", "\\\\")))
out:write('{"done":true}\n')
out:close()
