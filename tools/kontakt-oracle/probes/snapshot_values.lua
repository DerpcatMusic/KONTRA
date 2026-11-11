-- Measured probe: the script values Kontakt holds after loading a snapshot.
-- Loads each input (an .nki, or a .nksn: Kontakt loads its base instrument
-- and applies the snapshot), lets the scripts run for 6 s, and saves the
-- instrument; read the saved persistent script values with
-- `kontra-inspect saved <out.nki> <variable>` and compare with KONTRA's load.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local dir = ORACLE_OUT:match("^(.*[\\/])")
for n, input in ipairs(ORACLE_INPUTS) do
  kt.reset_multi()
  local ok, i = pcall(kt.load_instrument, input)
  local t0 = os.time()
  repeat until os.time() - t0 >= 6
  local path = dir .. "snapshot_values_" .. n .. ".nki"
  local saved, serr = false, i
  if ok then saved, serr = pcall(kt.save_instrument, i, path, {}) end
  log(string.format('{"input":%d,"loaded":%s,"saved":%s,"path":"%s","err":"%s"}', n, tostring(ok),
    tostring(saved), path:gsub("\\", "/"), tostring(serr):gsub('"', "'")))
end
log('{"done":true}')
out:close()
