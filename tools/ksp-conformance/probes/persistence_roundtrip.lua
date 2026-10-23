-- Persistence round trip on real Kontakt 8. Each variant builds instrument A from a KSP
-- script, saves it with kt.save_instrument, loads the file as instrument B, waits, then
-- reads B's group volumes. Bit b of an encoded value sits in group base+b and reads back
-- as a set bit when below -100 dB (see docs/re/KSP_CONFORMANCE.md).
--
-- Encoding points (8 bits each, group base):
--   A  0   $p after declaration, before any read_persistent_var
--   B  8   $p after read_persistent_var (if the variant has one)
--   C 16   $p at the end of on init
--   D 24   $p in on persistence_changed
--   S 32   $s before its read_persistent_var (1 means restored before init)
-- Set rule: `if ($s = 0)` writes the value and sets $s := 1. The writer runs it. A load
-- that restores $s sees 1 and skips it, so the value shown is what the file carried.
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

-- $s is saved first and restored with read_persistent_var, so a loaded run sees 1 at the
-- set rule. Its value before the read (S, base 32) shows whether Kontakt restores before init.
local HEADER = "on init\ndeclare $cv\ndeclare $cb\ndeclare $s\nmake_persistent($s)\n"
  .. enc("$s", 32) .. "read_persistent_var($s)\n"

-- Each variant: declarations (with make_persistent when the variant saves the value),
-- the value expression, the set statement the writer runs, and whether to read it in
-- init with read_persistent_var. `$s` is read first, so the loaded run skips the set.
local VARIANTS = {
  { name = "int_pers_readvar", decl = "declare $p\nmake_persistent($p)\n", val = "$p",
    set = "$p := 7", read = "read_persistent_var($p)\n" },
  { name = "int_pers_noread", decl = "declare $p\nmake_persistent($p)\n", val = "$p",
    set = "$p := 7", read = "" },
  { name = "int_nonpers", decl = "declare $p\n", val = "$p", set = "$p := 7", read = "" },
  { name = "real_pers", decl = "declare ~r\nmake_persistent(~r)\n", val = "int(~r)",
    set = "~r := 3.7", read = "" },
  { name = "array_pers", decl = "declare %a[3]\nmake_persistent(%a)\n", val = "%a[1]",
    set = "%a[1] := 5", read = "" },
  -- make_persistent inside an untaken if: Kontakt should not mark $z (no save), ours marks it.
  { name = "untaken_mp", decl = "declare $z\nif (0 = 1)\n  make_persistent($z)\nend if\n", val = "$z",
    set = "$z := 4", read = "" },
  -- make_instr_persistent: saved with the instrument.
  { name = "instr_pers", decl = "declare $ii\nmake_instr_persistent($ii)\n", val = "$ii",
    set = "$ii := 6", read = "" },
}

-- Body: the value before any read (A), the read (if any), value after the read (B), the
-- set when $s = 0, and the value at the end of init (C). persistence_changed shows D.
local function script(v)
  local e = v.val
  local src = HEADER .. v.decl
  src = src .. enc(e, 0)
  if v.read ~= "" then src = src .. v.read .. enc(e, 8) end
  src = src .. "if ($s = 0)\n  " .. v.set .. "\n  $s := 1\nend if\n"
  src = src .. enc(e, 16) .. "end on\n"
  src = src .. "on persistence_changed\n" .. enc(e, 24) .. "end on\n"
  return src
end

local function json_str(s)
  return '"' .. s:gsub('[%c"\\]', function(c) return string.format("\\u%04x", c:byte()) end) .. '"'
end

local function instrument(name, src)
  log(string.format('{"case":"%s","source":%s}', name, json_str(src)))
  local i = kt.add_instrument()
  kt.set_instrument_volume(i, 0)
  for _ = 1, 40 do kt.add_group(i) end
  local ok, err = pcall(kt.set_instrument_script_source, i, 0, src)
  log(string.format('{"%s_compile":%s,"err":"%s"}', name, tostring(ok), tostring(err or ""):gsub('"', "'"):sub(1, 160)))
  return i
end

local pending = {}
for _, v in ipairs(VARIANTS) do
  local a = instrument(v.name .. "_A", script(v))
  local path = DIR .. v.name .. ".nki"
  local ok, err = pcall(kt.save_instrument, a, path, {})
  log(string.format('{"%s_save":%s,"err":"%s"}', v.name, tostring(ok), tostring(err or ""):gsub('"', "'"):sub(1, 160)))
  local ok2, b = pcall(kt.load_instrument, path, 0)
  log(string.format('{"%s_load":%s,"index":"%s"}', v.name, tostring(ok2), tostring(b)))
  if ok2 and math.type(b) == "integer" then pending[#pending + 1] = { name = v.name, idx = b } end
end

-- Init and persistence_changed run on load; wait before reading back.
local t0 = os.time()
repeat until os.time() - t0 >= 4

for _, p in ipairs(pending) do
  local groups = {}
  for g = 0, 39 do
    local ok, db = pcall(kt.get_group_volume, p.idx, g)
    groups[#groups + 1] = string.format("%.9g", ok and db or -999)
  end
  log(string.format('{"case":"%s","group_volume_db":[%s]}', p.name, table.concat(groups, ",")))
end
log('{"done":true}')
out:close()
