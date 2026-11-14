-- Snapshot restore probe: what Kontakt's own instrument holds after a
-- snapshot is applied over it, saved for KONTRA's reader to compare.
-- ORACLE_INPUTS: pairs of (instrument .nki, snapshot .nksn of it). Each
-- pair: load the instrument, let its scripts settle, load the snapshot,
-- let them settle again, save as <ORACLE_OUT>.<n>.nki (and the bare
-- instrument as <ORACLE_OUT>.<n>.base.nki, before the snapshot).
-- Ends with {"done":true}. Writes no library content.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local function q(s) return '"' .. tostring(s):gsub("\\", "/"):gsub('"', "'") .. '"' end
local function settle(s) local t0 = os.time() repeat until os.time() - t0 >= s end

for n = 1, #ORACLE_INPUTS, 2 do
  local nki, snap = ORACLE_INPUTS[n], ORACLE_INPUTS[n + 1]
  local k = math.floor((n + 1) / 2)
  kt.reset_multi()
  local ok, i = pcall(kt.load_instrument, nki)
  log('{"pair":' .. k .. ',"load":' .. tostring(ok) .. ',"idx":' .. q(i) .. '}')
  if ok then
    settle(4)
    local b, berr = pcall(kt.save_instrument, i, ORACLE_OUT .. "." .. k .. ".base.nki", {})
    local s, serr = pcall(kt.load_snapshot, i, snap)
    settle(6)
    local v, verr = pcall(kt.save_instrument, i, ORACLE_OUT .. "." .. k .. ".nki", {})
    log('{"pair":' .. k .. ',"base_saved":' .. tostring(b) .. ',"snapshot":' .. tostring(s)
      .. ',"saved":' .. tostring(v) .. ',"err":' .. q(tostring(berr) .. " " .. tostring(serr) .. " " .. tostring(verr))
      .. ',"snap":' .. q(snap) .. '}')
  end
end
log('{"done":true}')
out:close()
