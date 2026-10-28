-- Measured probe: what a script reads back from a ui_menu and a control's text.
-- Encoding as in find_mod_index.lua: group k's tune is case k's value in 0.36 st units.
-- Input: ANALOG STRINGS.nki (slot 0 replaced; any instrument with enough groups).
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local cases = {
  { "item 1 text length (bb: 2)", "length(get_menu_item_str($id, 1))" },
  { "item 2 value", "get_menu_item_value($id, 2)" },
  { "NUM_ITEMS", "get_control_par($id, $CONTROL_PAR_NUM_ITEMS)" },
  { "SELECTED_ITEM_IDX after $m := 20", "$sel" },
  { "visibility of hidden item 0", "get_menu_item_visibility($id, 0)" },
  { "visibility of item 1", "get_menu_item_visibility($id, 1)" },
  { "item 0 text length after set_menu_item_str zzzz (4)", "length(get_menu_item_str($id, 0))" },
  { "item 1 value after set_menu_item_value 77", "get_menu_item_value($id, 1)" },
  { "label TEXT length after hello (5)", "" },
  { "menu value after $m := 20", "$m" },
  { "SELECTED_ITEM_IDX of a value no item has", "$none" },
  { "NUM_ITEMS of a hidden-item menu counts all", "get_control_par($id, $CONTROL_PAR_NUM_ITEMS)" },
}
local lines = {
  "on init", "  declare ui_menu $m", "  declare $id",
  "  declare $sel", "  declare $none",
  '  add_menu_item($m, "a", 10)', '  add_menu_item($m, "bb", 20)', '  add_menu_item($m, "ccc", 30)',
  "  $id := get_ui_id($m)",
  "  $m := 20", "  $sel := get_control_par($id, $CONTROL_PAR_SELECTED_ITEM_IDX)",
  "  $m := 25", "  $none := get_control_par($id, $CONTROL_PAR_SELECTED_ITEM_IDX)", "  $m := 20",
  "  set_menu_item_visibility($id, 0, 0)",
}
-- The label case runs alone in slot 1, so an error there cannot stop the menu cases.
local label = table.concat({
  "on init", "  declare ui_label $l(1, 1)", "  declare $lid", "  $lid := get_ui_id($l)",
  '  set_control_par_str($lid, $CONTROL_PAR_TEXT, "hello")',
  "  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * length(get_control_par_str($lid, $CONTROL_PAR_TEXT)), 8, -1, -1)",
  "end on", "",
}, "\n")
for k, c in ipairs(cases) do
  if k == 9 then goto continue end
  if k == 7 then lines[#lines + 1] = '  set_menu_item_str($id, 0, "zzzz")' end
  if k == 8 then lines[#lines + 1] = "  set_menu_item_value($id, 1, 77)" end
  local e = c[2]
  if e:find(" = ") then
    lines[#lines + 1] = "  if (" .. e .. ")"
    lines[#lines + 1] = string.format("    set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000, %d, -1, -1)", k - 1)
    lines[#lines + 1] = "  else"
    lines[#lines + 1] = string.format("    set_engine_par($ENGINE_PAR_TUNE, 500000, %d, -1, -1)", k - 1)
    lines[#lines + 1] = "  end if"
  else
    lines[#lines + 1] = string.format("  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * (%s), %d, -1, -1)", e, k - 1)
  end
  ::continue::
end
lines[#lines + 1] = "end on"
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
for s = 2, 4 do pcall(kt.set_instrument_script_source, i, s, "on init\nend on\n") end
local ok, err = pcall(kt.set_instrument_script_source, i, 0, table.concat(lines, "\n") .. "\n")
log(string.format('{"compile":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'")))
ok, err = pcall(kt.set_instrument_script_source, i, 1, label)
log(string.format('{"label compile":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'")))
local t0 = os.time()
repeat until os.time() - t0 >= 6
for k, c in ipairs(cases) do
  log(string.format('{"case":"%s","units":%.2f}', c[1], kt.get_group_tune(i, k - 1) / 0.36))
end
log('{"done":true}')
out:close()
