-- Whether KSP's `and`/`or` skip their right side, and what an out-of-range array read does.
-- Slot 1's init sets a marker tune on group 460+k after each case; a case that aborts the
-- callback leaves the later markers unset. Group 470+k marks a taken branch; 472 is the read.
-- Kontakt 8.13.1, 2026-10-28: every marker set (no abort); 470 unset, 471 set, read 0, 474
-- set: an out-of-range read answers 0 and a store is dropped. KONTRA's VM answers the same.
-- ANALOG STRINGS' 6684 out_of_range faults per render are all such reads, under guards like
-- `(%x[$i] # -1) and (... %arr[%x[$i]] ...)`, so its state matches Kontakt's.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
kt.reset_multi()
local i = kt.load_instrument(ORACLE_INPUTS[1])
local src = [[
on init
  declare %a[4] := (7, 7, 7, 7)
  declare $z := 0
  declare $v
  declare $n := -1
  set_engine_par($ENGINE_PAR_TUNE, 505000, 460, -1, -1)
  if ($z = 1 and (%a[$n] = 0))
    set_engine_par($ENGINE_PAR_TUNE, 510000, 470, -1, -1)
  end if
  set_engine_par($ENGINE_PAR_TUNE, 505000, 461, -1, -1)
  if ($z = 0 or (%a[$n] = 0))
    set_engine_par($ENGINE_PAR_TUNE, 510000, 471, -1, -1)
  end if
  set_engine_par($ENGINE_PAR_TUNE, 505000, 462, -1, -1)
  $v := %a[$n]
  set_engine_par($ENGINE_PAR_TUNE, 500000 + 5000 * ($v + 1), 472, -1, -1)
  set_engine_par($ENGINE_PAR_TUNE, 505000, 463, -1, -1)
  %a[$n] := 3
  set_engine_par($ENGINE_PAR_TUNE, 505000, 464, -1, -1)
  if ($z = 0 and (%a[$n] = 0))
    set_engine_par($ENGINE_PAR_TUNE, 510000, 474, -1, -1)
  end if
  set_engine_par($ENGINE_PAR_TUNE, 505000, 465, -1, -1)
end on
]]
local ok, err = pcall(kt.set_instrument_script_source, i, 1, src)
log(string.format('{"ok":%s,"err":"%s"}', tostring(ok), tostring(err):gsub('"', "'")))
local t0 = os.time() repeat until os.time() - t0 >= 6
for g = 460, 475 do
  log(string.format('{"g":%d,"u":%.2f}', g, kt.get_group_tune(i, g) / 0.36))
end
out:close()
