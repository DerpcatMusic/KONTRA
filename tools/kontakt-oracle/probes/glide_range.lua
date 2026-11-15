-- glide3 (derived from glide2.lua): glide range cap and first-note start. Kontakt 8.13 glide laws, part 2 (run with render.py, trigger note 36).
-- A) Glide intensity below 100 %: glide_time.nki (one group, Glide -> Source tune, Time mode) with
--    coef 750000 (418.7 ms); per step the route intensity is set from KSP, then 60, 72, 60 play.
-- B) Two gliding groups (group 1 = group 0 saved and loaded back), panned L/R, with notes allowed
--    to one group only (EVENT_PAR_ALLOW_GROUP) or a key range: where does each group's glide start?
-- C0 (removed after run 1): KSP set_engine_par($ENGINE_PAR_RELEASE_TRIGGER, 1) reads back 0 and the note
-- still sounds at note-on: release trigger is editor-only. Was: Can KSP switch Release Trigger on? Scratch instrument: readback encoded in group tune, and a
--    key-50 note that sounds at note-off if it did. Every note is 1.5 s long with >= 0.6 s of silence.
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local function log(t) out:write(t .. "\n") out:flush() end
local HERE = "Z:\\data\\dev\\workspace\\BuildScratch\\voices\\probes2\\"
local NKI, NKG, SHORT = HERE .. "glide_time.nki", HERE .. "g0.nkg", HERE .. "short.wav"
local SINE = HERE .. "sine.wav"
-- The saved patch loses its zone (its sample path is gone): every gliding group gets a sine zone.
local function sine_zone(i, g, high)
  local z = kt.add_zone(i, g, SINE)
  kt.set_zone_geometry(i, z, { root_key = 60, low_key = 0, high_key = high or 127 })
end

-- Events {seconds, kind, a, b, only}: kind 1 note-on key a vel b (only: the one allowed group),
-- 0 note-off key a, 2 INTMOD_INTENSITY := a, 3 MOD_TARGET_INTENSITY := a (Glide route, all groups).
local function script(events, init)
  table.sort(events, function(x, y) return x[1] < y[1] end)
  local n = #events
  local d = { "on init", "  declare %ids[128]", "  declare $s", "  declare $g", "  declare $i", "  declare $w",
    "  declare $m := -1", "  declare $n := " .. n, "  declare %t[" .. n .. "]", "  declare %kd[" .. n .. "]",
    "  declare %a[" .. n .. "]", "  declare %b[" .. n .. "]", "  declare %o[" .. n .. "]",
    "  $i := 0", "  while ($i < 16)",
    "    if (get_engine_par($ENGINE_PAR_INTMOD_TYPE, 0, $i, -1) = $INTMOD_TYPE_GLIDE)", "      $m := $i", "    end if",
    "    inc($i)", "  end while",
    "  if ($m >= 0)", "    $g := 0", "    while ($g < $NUM_GROUPS)",
    "      set_engine_par($ENGINE_PAR_GLIDE_COEF, 750000, $g, $m, -1)", "      inc($g)", "    end while", "  end if",
    init or "" }
  for i, e in ipairs(events) do
    d[#d + 1] = string.format("  %%t[%d] := %d", i - 1, math.floor(e[1] * 1e6 + 0.5))
    d[#d + 1] = string.format("  %%kd[%d] := %d", i - 1, e[2])
    d[#d + 1] = string.format("  %%a[%d] := %d", i - 1, e[3])
    d[#d + 1] = string.format("  %%b[%d] := %d", i - 1, e[4] or 0)
    d[#d + 1] = string.format("  %%o[%d] := %d", i - 1, e[5] or -1)
  end
  return table.concat(d, "\n") .. [==[

end on
on note
  ignore_event($EVENT_ID)
  if ($EVENT_NOTE = 36)
    $s := 0
    while ($s < $n)
      if ($s = 0)
        $w := %t[0] + 1
      else
        $w := %t[$s] - %t[$s - 1]
      end if
      while ($w > 1000000)
        wait(1000000)
        $w := $w - 1000000
      end while
      if ($w > 0)
        wait($w)
      end if
      select (%kd[$s])
        case 1
          %ids[%a[$s]] := play_note(%a[$s], %b[$s], 0, 0)
          if (%o[$s] >= 0)
            set_event_par_arr(%ids[%a[$s]], $EVENT_PAR_ALLOW_GROUP, 0, $ALL_GROUPS)
            set_event_par_arr(%ids[%a[$s]], $EVENT_PAR_ALLOW_GROUP, 1, %o[$s])
          end if
        case 0
          note_off(%ids[%a[$s]])
        case 2
          $g := 0
          while ($g < $NUM_GROUPS)
            set_engine_par($ENGINE_PAR_INTMOD_INTENSITY, %a[$s], $g, $m, -1)
            inc($g)
          end while
        case 3
          $g := 0
          while ($g < $NUM_GROUPS)
            set_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY, %a[$s], $g, $m, -1)
            inc($g)
          end while
      end select
      inc($s)
    end while
  end if
end on
]==]
end

local NOTE, GAP = 1.5, 0.6
local function phrase(ev, at, keys, only)
  for j, k in ipairs(keys) do
    local t = at + (j - 1) * (NOTE + GAP)
    ev[#ev + 1] = {t, 1, k, 100, only and only[j]}
    ev[#ev + 1] = {t + NOTE, 0, k}
  end
  return at + #keys * (NOTE + GAP)
end

local function load()
  local i = kt.load_instrument(NKI)
  kt.set_instrument_midi_channel(i, ORACLE_CHANNEL or 1)
  sine_zone(i, 0)
  return i
end

local function main()
  kt.reset_multi()
  -- glide3: is the slide capped at 12 st, and where does the first note start? 100 %, 418.7 ms.
  -- 40 first (36 is the ignored trigger), then jumps of +20, -38, +19, -6, +24, -13, +12.
  local a = load()
  local ev = {}
  phrase(ev, 0.2, {40, 60, 22, 41, 35, 59, 46, 58})
  log(string.format('{"G3":%s}', tostring(pcall(kt.set_instrument_script_source, a, 0, script(ev)))))
end
local ok, err = xpcall(main, debug and debug.traceback or tostring)
if not ok then log('{"error":"' .. tostring(err):gsub('"', "'"):gsub("\n", " | ") .. '"}') end
log('{"ready":true}')
out:close()
