-- Measured probe: the values of the callback-type and listener-signal constants.
-- Encoding as in find_mod_index.lua: group k's tune is case k's value in 0.36 st units.
-- Input: ANALOG STRINGS.nki (any instrument with enough groups).
-- Result (Kontakt 8.13.1, 2026-10-28): $NI_CB_TYPE_ NOTE 1, RELEASE 2, CONTROLLER 3, POLY_AT 4,
-- INIT 5, UI_CONTROL 6, UI_UPDATE 7, PGS 8, RPN 10, NRPN 11, LISTENER 12, ASYNC_COMPLETE 13,
-- PERSISTENCE_CHANGED 14, UI_CONTROLS 15; $NI_SIGNAL_ TRANSP_START 1, TRANSP_STOP 2,
-- TIMER_MS 3, TIMER_BEAT 4.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local cases = {
  "$NI_CB_TYPE_INIT", "$NI_CB_TYPE_NOTE", "$NI_CB_TYPE_RELEASE", "$NI_CB_TYPE_CONTROLLER",
  "$NI_CB_TYPE_POLY_AT", "$NI_CB_TYPE_RPN", "$NI_CB_TYPE_NRPN", "$NI_CB_TYPE_UI_CONTROL",
  "$NI_CB_TYPE_UI_UPDATE", "$NI_CB_TYPE_UI_CONTROLS", "$NI_CB_TYPE_LISTENER", "$NI_CB_TYPE_PGS",
  "$NI_CB_TYPE_PERSISTENCE_CHANGED", "$NI_CB_TYPE_ASYNC_COMPLETE",
  "$NI_SIGNAL_TIMER_MS", "$NI_SIGNAL_TIMER_BEAT", "$NI_SIGNAL_TRANSP_START", "$NI_SIGNAL_TRANSP_STOP",
}
-- Two passes, each on a fresh instrument: ANALOG protects slots 2..4, so slots 0 and 1 take
-- one group of names each, and a name Kontakt does not know fails only its own slot.
local passes = { { { 1, 4 }, { 5, 9 } }, { { 10, 14 }, { 15, 18 } } }
for pass, slots in ipairs(passes) do
  kt.reset_multi()
  local i = kt.load_instrument(ORACLE_INPUTS[1])
  local compiled = {}
  for s, range in ipairs(slots) do
    local lines = { "on init" }
    for k = range[1], range[2] do
      lines[#lines + 1] = string.format("  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * (%s), %d, -1, -1)", cases[k], k - 1)
    end
    lines[#lines + 1] = "end on"
    local ok, err = pcall(kt.set_instrument_script_source, i, s - 1, table.concat(lines, "\n") .. "\n")
    compiled[s] = ok
    log(string.format('{"pass":%d,"slot":%d,"compile":%s,"err":"%s"}', pass, s - 1, tostring(ok), tostring(err):gsub('"', "'")))
  end
  local t0 = os.time()
  repeat until os.time() - t0 >= 6
  for s, range in ipairs(slots) do
    -- A slot whose script did not install measured nothing.
    for k = range[1], compiled[s] and range[2] or range[1] - 1 do
      log(string.format('{"case":"%s","units":%.2f}', cases[k], kt.get_group_tune(i, k - 1) / 0.36))
    end
  end
end
log('{"done":true}')
out:close()
