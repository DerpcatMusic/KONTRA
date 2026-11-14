-- Multi script probe: Kontakt saves a multi whose first multi script slot
-- moves channel 1 to channel 2 (`on midi_in`), as ORACLE_OUT .. ".nkm",
-- and reads back the script's name and source length. Ends with {"done":true}.

local kt = Kontakt
local file = assert(io.open(ORACLE_OUT, "w"))
local function line(s) file:write(s .. "\n") file:flush() end
local function try(f, ...)
  local ok, v = pcall(f, ...)
  return ok and v or ("error: " .. tostring(v))
end

local source = [[on midi_in
  if ($MIDI_CHANNEL = 0)
    ignore_midi
    set_midi(1, $MIDI_COMMAND, $MIDI_BYTE_1, $MIDI_BYTE_2)
  end if
end on
]]
for _, i in ipairs(try(kt.get_instrument_indices) or {}) do
  try(kt.remove_instrument, i)
end
try(kt.add_instrument, 0)
try(kt.set_multi_script_source, 0, source)
try(kt.set_multi_script_name, 0, "Channel 1 to 2")
local name = try(kt.get_multi_script_name, 0)
local back = try(kt.get_multi_script_source, 0)
line(string.format('{"name":"%s","source_len":%d}', tostring(name), type(back) == "string" and #back or -1))
local saved = try(kt.save_multi, ORACLE_OUT .. ".nkm", {})
line(string.format('{"save":"%s"}', tostring(saved)))
file:write('{"done":true}\n')
file:close()
