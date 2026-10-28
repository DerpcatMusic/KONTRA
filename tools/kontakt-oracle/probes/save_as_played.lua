-- Measured probe: the state ANALOG STRINGS' own scripts leave, saved by Kontakt.
-- Loads the instrument, lets its scripts run for 6 s, saves; read the bypass and
-- intensity of every route with `KONTRA_GROUPS=<a>..<b> ... --example inspect`.
-- Result (Kontakt 8.13.1, 2026-10-29), group 88: LFO_P1_Retrigger @1 and
-- LFO_P2_Retrigger @4 active, LFO_P1 @3 and LFO_P2 @5 bypassed, all at intensity
-- 0.5714: the same state KONTRA's run of the script reaches.
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local dir = ORACLE_OUT:match("^(.*[\\/])")
local cases = {}
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
local ok, err = true, nil
log(string.format('{"compile":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'")))
local t0 = os.time()
repeat until os.time() - t0 >= 6
local saved, serr = pcall(kt.save_instrument, i, dir .. "as_played.nki", {})
log(string.format('{"saved":%s,"path":"%s","err":"%s"}', tostring(saved), (dir .. "as_played.nki"):gsub("\\", "/"), tostring(serr):gsub('"', "'")))
for _, c in ipairs(cases) do log(c) end
log('{"done":true}')
out:close()
