-- Kontakt's Glide modulator law, measured (run with gui_session.py: Glide -> pitch at 100 % is added to
-- the group's Source tune in the editor; no API adds a modulator).
-- One instrument, one group, one zone of a 12 s 261.6256 Hz sine rooted at 60 (C3 in Kontakt's naming).
-- A held trigger note (key 36, held for the whole run: play_note's voices end with it) starts the KSP
-- sequence below.
-- The recording is pitch-tracked (BuildScratch/glide/pitch_track.py) and fitted.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local dir = ORACLE_OUT:match("^(.*[\\/])")
local WAV = dir .. "sine.wav"
local f = assert(io.open(WAV, "wb"))
local rate, n = 48000, 48000 * 12
f:write("RIFF", string.pack("<I4", 36 + n * 4), "WAVEfmt ", string.pack("<I4I2I2I4I4I2I2", 16, 3, 1, rate, rate * 4, 4, 32),
  "data", string.pack("<I4", n * 4))
local w = 2 * math.pi * 261.6255653 / rate
local chunk = {}
for i = 0, n - 1 do
  chunk[#chunk + 1] = string.pack("<f", 0.3 * math.sin(w * i))
  if #chunk == 4800 then f:write(table.concat(chunk)) chunk = {} end
end
f:write(table.concat(chunk))
f:close()

kt.reset_multi()
local inst = kt.add_instrument()
kt.set_instrument_midi_channel(inst, ORACLE_CHANNEL or 1)
local z = kt.add_zone(inst, 0, WAV)
kt.set_zone_geometry(inst, z, { root_key = 60, low_key = 0, high_key = 127 })

-- Steps: coefficient (0..1000000), interval. The mode (Time or Speed) is picked in the editor:
-- writing $ENGINE_PAR_GLIDE_COEF_UNIT changes nothing (it reads back 0, the audio is the same). Each step plays one voice at a time:
-- key 60 for 5 s, 0.5 s of release, key 60 + interval for 5 s, 0.5 s of release, so a step shows the
-- glide up from 60 and (in the next step's first note) the glide down from 60 + interval. The label shows
-- each coefficient's displayed value in both units.
local steps = {}
for _, g in ipairs({0}) do
  for _, c in ipairs({0, 150000, 300000, 450000, 600000, 750000, 900000}) do
    for _, iv in ipairs({4, 12}) do steps[#steps + 1] = {g, c, iv} end
  end
end
local decl = { "on init", "  make_perfview", "  set_ui_height(6)", "  declare $s", "  declare $i", "  declare $m",
  "  declare $a", "  declare $b", "  declare ui_label $show(6, 6)",
  "  declare %grp[" .. #steps .. "]", "  declare %coef[" .. #steps .. "]", "  declare %iv[" .. #steps .. "]" }
for k, s in ipairs(steps) do
  decl[#decl + 1] = string.format("  %%grp[%d] := %d", k - 1, s[1])
  decl[#decl + 1] = string.format("  %%coef[%d] := %d", k - 1, s[2])
  decl[#decl + 1] = string.format("  %%iv[%d] := %d", k - 1, s[3])
end
local body = [==[
end on
on note
  ignore_event($EVENT_ID)
  if ($EVENT_NOTE = 36)
    set_text($show, "")
    $m := -1
    $i := 0
    while ($i < 16)
      if (get_engine_par($ENGINE_PAR_INTMOD_TYPE, 0, $i, -1) = $INTMOD_TYPE_GLIDE)
        $m := $i
      end if
      inc($i)
    end while
    if ($m >= 0)
      $s := 0
      while ($s < num_elements(%grp))
        if (%iv[$s] = 4)
          set_engine_par($ENGINE_PAR_GLIDE_COEF_UNIT, %grp[$s], 0, $m, -1)
          set_engine_par($ENGINE_PAR_GLIDE_COEF, %coef[$s], 0, $m, -1)
          add_text_line($show, "u" & get_engine_par($ENGINE_PAR_GLIDE_COEF_UNIT, 0, $m, -1) & " " & %coef[$s] & " = " & get_engine_par_disp($ENGINE_PAR_GLIDE_COEF, 0, $m, -1))
        end if
        inc($s)
      end while
    else
      set_text($show, "no glide modulator")
    end if
    $s := 0
    while ($s < num_elements(%grp))
      set_engine_par($ENGINE_PAR_GLIDE_COEF_UNIT, %grp[$s], 0, $m, -1)
      set_engine_par($ENGINE_PAR_GLIDE_COEF, %coef[$s], 0, $m, -1)
      $a := play_note(60, 100, 0, 0)
      wait(5000000)
      note_off($a)
      wait(500000)
      $b := play_note(60 + %iv[$s], 100, 0, 0)
      wait(5000000)
      note_off($b)
      wait(500000)
      inc($s)
    end while
  end if
end on
]==]
local ok, err = pcall(kt.set_instrument_script_source, inst, 0, table.concat(decl, "\n") .. "\n" .. body)
log('{"script":' .. tostring(ok) .. ',"err":"' .. tostring(err):gsub('"', "'") .. '","steps":' .. #steps .. '}')
log('{"ready":true}')
out:close()
