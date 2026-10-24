-- Two script slots in one instrument, on real Kontakt 8. Probes four things:
--   order   both slots' on init write group 40 with different levels; the final level
--           names the slot that ran last (readback group 40).
--   scope   slot 0 declares $x := 5, slot 1 reads its own $x (group bits 0..7). A
--           shared variable would read 5.
--   pers    each slot has a persistent $p with the same name and a sentinel $s. Slot 0
--           stores 3, slot 1 stores 9. After save and load the two must not swap or mix.
--           Groups 8..15 are slot 0's $p and 16..23 slot 1's $p, read in
--           on persistence_changed.
-- Instrument M is the writer; instrument L is loaded from the file that M saved.
-- Bits are written as set_engine_par(ENGINE_PAR_VOLUME, 1000, group) and read back as
-- group volume below -100 dB, as in persistence_roundtrip.lua.
--
-- Run: flock /tmp/kontra-oracle.lock env ORACLE_LUA=<this file> \
--        python3 tools/kontakt-oracle/dump.py OUT.jsonl
local kt = Kontakt
local DIR = "Z:\\data\\dev\\workspace\\Research\\KONTRA\\scratch-pers\\"
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end

local function enc(expr, base)
  return string.format([[
  $cv := %s
  $cb := 0
  while ($cb < 8)
    if ((sh_right($cv, $cb) mod 2) # 0)
      set_engine_par($ENGINE_PAR_VOLUME, 1000, %d + $cb, -1, -1)
    end if
    inc($cb)
  end while
]], expr, base)
end

local SLOT0 = "on init\ndeclare $cv\ndeclare $cb\ndeclare $s\ndeclare $p\nmake_persistent($s)\nmake_persistent($p)\nread_persistent_var($s)\nread_persistent_var($p)\n"
  .. "declare $x := 5\n"
  .. "set_engine_par($ENGINE_PAR_VOLUME, 2000, 40, -1, -1)\n"
  .. "if ($s = 0)\n  $p := 3\n  $s := 1\nend if\n"
  .. "end on\n"
  .. "on persistence_changed\n" .. enc("$p", 8) .. "end on\n"

local SLOT1 = "on init\ndeclare $cv\ndeclare $cb\ndeclare $s\ndeclare $p\nmake_persistent($s)\nmake_persistent($p)\nread_persistent_var($s)\nread_persistent_var($p)\n"
  .. "declare $x\n"
  .. enc("$x", 0)
  .. "set_engine_par($ENGINE_PAR_VOLUME, 4000, 40, -1, -1)\n"
  .. "if ($s = 0)\n  $p := 9\n  $s := 1\nend if\n"
  .. "end on\n"
  .. "on persistence_changed\n" .. enc("$p", 16) .. "end on\n"

local function instrument(name)
  local i = kt.add_instrument()
  kt.set_instrument_volume(i, 0)
  for _ = 1, 41 do kt.add_group(i) end
  for slot, src in ipairs({ SLOT0, SLOT1 }) do
    local ok, err = pcall(kt.set_instrument_script_source, i, slot - 1, src)
    log(string.format('{"%s_slot%d_compile":%s,"err":"%s"}', name, slot - 1, tostring(ok),
      tostring(err or ""):gsub('"', "'"):sub(1, 160)))
  end
  return i
end

local m = instrument("M")
local path = DIR .. "multi_slot.nki"
local ok, err = pcall(kt.save_instrument, m, path, {})
log(string.format('{"save":%s,"err":"%s"}', tostring(ok), tostring(err or ""):gsub('"', "'"):sub(1, 160)))
local ok2, l = pcall(kt.load_instrument, path, 0)
log(string.format('{"load":%s,"index":"%s"}', tostring(ok2), tostring(l)))

local t0 = os.time()
repeat until os.time() - t0 >= 4

for _, p in ipairs({ { "M", m }, { "L", ok2 and l or nil } }) do
  if p[2] then
    local groups = {}
    for g = 0, 40 do
      local ok3, db = pcall(kt.get_group_volume, p[2], g)
      groups[#groups + 1] = string.format("%.9g", ok3 and db or -999)
    end
    log(string.format('{"case":"%s","group_volume_db":[%s]}', p[1], table.concat(groups, ",")))
  end
end
log('{"done":true}')
out:close()
