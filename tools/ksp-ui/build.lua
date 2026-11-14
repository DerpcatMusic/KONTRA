-- Builds each probe page into an instrument and loads them all into one multi (shoot.sh writes
-- ORACLE_OUT, ORACLE_NKR and ORACLE_PAGES = {{ksp, nki}, ...} above this). Kontakt's Lua API does not
-- report KSP compile errors: a page that fails shows no performance view.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
for _, page in ipairs(ORACLE_PAGES) do
  kt.reset_multi()
  local src = assert(io.open(page[1], "r")):read("a")
  local i = kt.add_instrument()
  local s1 = pcall(kt.save_instrument, i, page[2], {})
  local rc, re = pcall(kt.create_resource_container, i, ORACLE_NKR)
  local ok, err = pcall(kt.set_instrument_script_source, i, 0, src)
  local s2, e2 = pcall(kt.save_instrument, i, page[2], {})
  out:write(string.format('{"page":"%s","rc":%s,"re":"%s","save1":%s,"ok":%s,"err":"%s","save2":%s,"e2":"%s"}\n',
    page[2]:gsub("\\", "/"), tostring(rc), tostring(re), tostring(s1), tostring(ok), tostring(err), tostring(s2), tostring(e2)))
end
kt.reset_multi()
for _, page in ipairs(ORACLE_PAGES) do
  local l, le = pcall(kt.load_instrument, page[2])
  out:write(string.format('{"load":%s,"le":"%s"}\n', tostring(l), tostring(le)))
end
out:write('{"done":true}\n')
out:close()
