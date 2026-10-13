-- Probe: how group start options are stored. Sets known start conditions through
-- Kontakt's own API, reads them back, and saves the instrument so KONTRA's reader
-- can be checked against the stored fields (crates/sampler-kontakt/tests/native_oracle.rs).
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
kt.reset_multi()
local i = kt.add_instrument()
local cases = {
  { { mode = "round_robin", position = 1 } },
  { { mode = "round_robin", position = 2 } },
  { { mode = "round_robin", position = 3 } },
  { { mode = "round_robin", position = 5 } },
  { { mode = "random" } },
  { { mode = "key", key_min = 36, key_max = 40, next = "and" }, { mode = "round_robin", position = 2 } },
  { { mode = "controller", controller = 11, cc_min = 10, cc_max = 90 } },
}
for n, opts in ipairs(cases) do
  local g = n == 1 and 0 or kt.add_group(i)
  local ok, err = pcall(kt.set_group_start_options, i, g, opts)
  local back = kt.get_group_start_options(i, g)
  local parts = {}
  for _, o in ipairs(back) do
    local fields = {}
    for k, v in pairs(o) do fields[#fields + 1] = string.format('"%s":"%s"', k, tostring(v)) end
    parts[#parts + 1] = "{" .. table.concat(fields, ",") .. "}"
  end
  out:write(string.format('{"group":%d,"ok":%s,"err":"%s","back":[%s]}\n', g, tostring(ok), tostring(err or ""), table.concat(parts, ",")))
end
local path = ORACLE_OUT:gsub("[^\\/]+$", "") .. "start_options.nki"
kt.save_instrument(i, path, {})
out:write('{"done":true}\n')
out:close()
