-- Pilot: can the Lua API load an effect preset into an insert slot, and when does a KSP
-- script set through set_instrument_script_source change engine parameters?
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(s) out:write(s .. "\n") out:flush() end
local dir = ORACLE_OUT:gsub("[^\\/]+$", "")
local preset = kt.factory_path .. "\\Presets\\Effects\\ACBox\\Clean.nkp"
local function try(name, f, ...)
  local ok, r = pcall(f, ...)
  log(string.format('{"call":"%s","ok":%s,"ret":"%s"}', name, tostring(ok), tostring(r):gsub('"', "'")))
  return ok, r
end
kt.reset_multi()
local i = kt.add_instrument()
try("load5", kt.load_fx_preset, preset, i, -1, 0, kt.insert_fx)
local function save(idx, tag) try("save_" .. tag, kt.save_instrument, idx, dir .. "pilot_" .. tag .. ".nki", {}) end
save(i, "before")
local src = "on init\n  set_engine_par($ENGINE_PAR_VOLUME, 500000, -1, -1, -1)\n  set_engine_par($ENGINE_PAR_AC_BASS, 900000, -1, 0, 0)\nend on\n"
local f = assert(io.open(dir .. "pilot.ksp", "w")); f:write(src); f:close()
try("script_name", kt.set_instrument_script_name, i, 0, "probe")
log('{"vol_before":' .. kt.get_instrument_volume(i) .. '}')
try("script_src", kt.set_instrument_script_source, i, 0, src)
log("{\"vol_after_set\":" .. kt.get_instrument_volume(i) .. "}")
try("bypass_on", kt.set_instrument_script_bypassed, i, 0, true)
try("bypass_off", kt.set_instrument_script_bypassed, i, 0, false)
log("{\"vol_after_toggle\":" .. kt.get_instrument_volume(i) .. "}")
save(i, "after")
kt.reset_multi()
local ok, j = try("load_instrument", kt.load_instrument, dir .. "pilot_after.nki", 0)
log('{"slot":' .. tostring(j) .. '}')
log('{"vol_after_reload":' .. tostring(kt.get_instrument_volume(0)) .. '}')
save(0, "reloaded")
log('{"done":true}')
out:close()
