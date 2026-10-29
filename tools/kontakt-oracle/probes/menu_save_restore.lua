-- Measured probe: what Kontakt saves for a persistent ui_menu and what it restores.
-- Input: ANALOG STRINGS.nki (slot 0 empty; slots 1-4 bypassed). Fresh run selects $m := 30
-- (entries 10/20/30), $n := 1 (entries -1/0/1) and $p := 30, saves, reloads the save.
-- Result (Kontakt 8.13.1, 2026-10-29): the save holds "$m 2", "$n 2", "$p 2" (the selected
-- entry's index, read with KONTRA's parser); read_persistent_var restores $m = 30, $n = 1,
-- SELECTED_ITEM_IDX 2. A new menu holds its first entry's value ($p reads 10 before restore).
-- Group k's tune is case k: st = 0.72 * (written - 500000) / 10000.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local dir = ORACLE_OUT:match("^(.*[\\/])")
local src = [[
on init
  declare ui_menu $m
  add_menu_item($m, "a", 10)
  add_menu_item($m, "b", 20)
  add_menu_item($m, "c", 30)
  declare ui_menu $n
  add_menu_item($n, "Off", -1)
  add_menu_item($n, "G0", 0)
  add_menu_item($n, "G1", 1)
  declare ui_menu $p
  add_menu_item($p, "x", 10)
  add_menu_item($p, "y", 20)
  add_menu_item($p, "z", 30)
  declare $runs
  make_persistent($m)
  make_persistent($n)
  make_persistent($p)
  make_persistent($runs)
  read_persistent_var($runs)
  read_persistent_var($m)
  read_persistent_var($n)
  if ($runs = 0)
    $m := 30
    $n := 1
    $p := 30
  else
    set_engine_par($ENGINE_PAR_TUNE, 500000 + 2000 * $m, 0, -1, -1)
    set_engine_par($ENGINE_PAR_TUNE, 500000 + 20000 * get_control_par(get_ui_id($m), $CONTROL_PAR_SELECTED_ITEM_IDX), 1, -1, -1)
    set_engine_par($ENGINE_PAR_TUNE, 500000 + 20000 * $n, 2, -1, -1)
    set_engine_par($ENGINE_PAR_TUNE, 500000 + 2000 * $p, 3, -1, -1)
  end if
  inc($runs)
end on
on persistence_changed
  set_engine_par($ENGINE_PAR_TUNE, 500000 + 2000 * $p, 4, -1, -1)
  set_engine_par($ENGINE_PAR_TUNE, 500000 + 20000 * get_control_par(get_ui_id($p), $CONTROL_PAR_SELECTED_ITEM_IDX), 5, -1, -1)
  set_engine_par($ENGINE_PAR_TUNE, 500000 + 2000 * $m, 6, -1, -1)
  set_engine_par($ENGINE_PAR_TUNE, 500000 + 20000 * $runs, 7, -1, -1)
end on
]]
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
for s = 1, 4 do pcall(kt.set_instrument_script_bypassed, i, s, true) end
local ok, err = pcall(kt.set_instrument_script_source, i, 0, src)
log(string.format('{"compile":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'")))
local t0 = os.time() repeat until os.time() - t0 >= 2
local path = dir .. "menu_save.nki"
local saved, serr = pcall(kt.save_instrument, i, path, {})
log(string.format('{"saved":%s,"path":"%s","err":"%s"}', tostring(saved), path:gsub("\\", "/"), tostring(serr):gsub('"', "'")))
kt.reset_multi()
local j = kt.load_instrument(path)
local t1 = os.time() repeat until os.time() - t1 >= 3
for g = 0, 7 do log(string.format('{"g":%d,"st":%.4f}', g, kt.get_group_tune(j, g))) end
log('{"done":true}')
out:close()
