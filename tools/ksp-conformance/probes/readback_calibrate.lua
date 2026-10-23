-- Calibration of the conformance readback channel. Each case is one instrument whose
-- `on init` writes a known integer through set_engine_par; the Lua side reads the
-- matching instrument property back (VOLUME -> get_instrument_volume, PAN ->
-- get_instrument_pan, TUNE -> get_instrument_tune). The mapping value -> readback is what
-- the conformance harness inverts, so it must be measured before it is trusted.
-- Run: flock /tmp/kontra-oracle.lock ORACLE_LUA=tools/ksp-conformance/probes/readback_calibrate.lua \
--        tools/kontakt-oracle/dump.py OUT.jsonl
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
kt.reset_multi()

local PARS = {
  { "VOLUME", "$ENGINE_PAR_VOLUME", "get_instrument_volume" },
  { "PAN", "$ENGINE_PAR_PAN", "get_instrument_pan" },
  { "TUNE", "$ENGINE_PAR_TUNE", "get_instrument_tune" },
}
local VALUES = { 0, 1, -1, 1000, -1000 }

local cases = {}
for _, p in ipairs(PARS) do
  for _, v in ipairs(VALUES) do
    local src = "on init\n  set_engine_par(" .. p[2] .. ", " .. v .. ", -1, -1, -1)\nend on\n"
    cases[#cases + 1] = { p[1], v, src, p[3] }
  end
end

local ids = {}
for n, c in ipairs(cases) do
  local i = kt.add_instrument()
  kt.set_instrument_volume(i, 0)
  local ok, err = pcall(kt.set_instrument_script_source, i, 0, c[3])
  ids[n] = i
  log(string.format('{"compile":"%s=%d","ok":%s,"err":"%s"}', c[1], c[2], tostring(ok), tostring(err or "")))
end

local t0 = os.time()
repeat until os.time() - t0 >= 4
for n, c in ipairs(cases) do
  local readback = kt[c[4]] and kt[c[4]](ids[n]) or kt.get_instrument_volume(ids[n])
  log(string.format('{"par":"%s","value":%d,"readback":%.9g}', c[1], c[2], readback))
end
log('{"done":true}')
out:close()
