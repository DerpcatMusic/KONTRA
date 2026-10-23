-- Polyphonic read probe: what does Kontakt return when `$v` (declare polyphonic) is read
-- from on listener, i.e. outside a note callback, after notes have set it?
--
-- Each scenario is one instrument. Its KSP script sets $v in `on note` and, on one chosen
-- listener tick, copies $v to a plain variable and encodes it into the VOLUME engine parameter
-- as 1000 * ($out + 1). Calibration instruments encode known values the same way, so the
-- readback (get_instrument_volume, dB) maps back to the value Kontakt used.
-- Notes are played by the script itself (play_note); the listener ticks every 1000 ms.
-- BLOCKED (2026-10-23): under dump.py Kontakt has no audio thread, so on listener never fires
-- (probes/ksp_polyphonic_init_read.lua shows listener_plain = 0 dB) and play_note never sounds.
-- Needs an audio device that persists (tools/kontakt-oracle/render.py clicks it in each launch).
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local dir = ORACLE_OUT:gsub("[^\\/]+$", "")

local function write_wav(path, samples)
  local f = assert(io.open(path, "wb"))
  local n = #samples
  f:write("RIFF", string.pack("<I4", 36 + n * 4), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 1, 48000, 192000, 4, 32), "data", string.pack("<I4", n * 4))
  for i = 1, n do f:write(string.pack("<f", samples[i])) end
  f:close()
end
local imp = {}
for i = 1, 4800 do imp[i] = 0.0 end
imp[1] = 0.5
write_wav(dir .. "pv_impulse.wav", imp)

local HEAD = [[
on init
  declare polyphonic $v
  declare $tick := 0
  declare $out := -1
  set_listener($NI_SIGNAL_TIMER_MS, 1000)
end on
on note
  $v := $EVENT_NOTE
end on
on listener
  inc($tick)
]]

local function encode(tick_at)
  return "  if $tick = " .. tick_at .. " then\n"
    .. "    $out := $v\n"
    .. "    set_engine_par($ENGINE_PAR_VOLUME, 1000 * ($out + 1), -1, -1, -1)\n"
    .. "  end if\n"
end

-- scenario name, listener tick at which $v is read, extra body (notes), tick the read happens
local SCENARIOS = {
  { "none", 2, "" },
  { "one_released", 3, "  if $tick = 1 then play_note(60, 100, 0, 200) end if\n" },
  { "one_held", 2, "  if $tick = 1 then play_note(60, 100, 0, 4000) end if\n" },
  { "two_held", 2, "  if $tick = 1 then\n    play_note(60, 100, 0, 4000)\n    play_note(64, 100, 200, 4000)\n  end if\n" },
  { "two_released", 3, "  if $tick = 1 then\n    play_note(60, 100, 0, 200)\n    play_note(64, 100, 300, 200)\n  end if\n" },
}
local CAL = { 0, 1, 2, 64, 127 }

kt.reset_multi()
local rows = {}
for _, s in ipairs(SCENARIOS) do
  local i = kt.add_instrument()
  local z = kt.add_zone(i, 0, dir .. "pv_impulse.wav")
  kt.set_zone_geometry(i, z, { root_key = 60, low_key = 0, high_key = 127 })
  kt.set_instrument_volume(i, 0)
  local src = HEAD .. s[3] .. encode(s[2]) .. "end on\n"
  kt.set_instrument_script_name(i, 0, "pv_" .. s[1])
  local ok = pcall(kt.set_instrument_script_source, i, 0, src)
  rows[#rows + 1] = { name = s[1], idx = i, ok = ok }
end
for _, x in ipairs(CAL) do
  local i = kt.add_instrument()
  local src = "on init\n  set_engine_par($ENGINE_PAR_VOLUME, " .. (1000 * (x + 1)) .. ", -1, -1, -1)\nend on\n"
  kt.set_instrument_script_name(i, 0, "pv_cal")
  local ok = pcall(kt.set_instrument_script_source, i, 0, src)
  rows[#rows + 1] = { name = "cal_" .. x, idx = i, ok = ok, x = x }
end

-- Busy-wait ~7 s of wall time so the listener ticks 1..3 have run before the readback.
local t0 = os.time()
repeat until os.time() - t0 >= 7

for _, r in ipairs(rows) do
  log(string.format('{"scenario":"%s","script_ok":%s,"volume_db":%s}', r.name, tostring(r.ok), tostring(kt.get_instrument_volume(r.idx))))
end
log('{"done":true}')
out:close()
