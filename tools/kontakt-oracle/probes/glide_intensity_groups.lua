-- Kontakt 8.13 glide laws, part 2 (run with render.py, trigger note 36).
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
  local t = 0.2
  -- A: (kind, raw) per step. INTMOD_INTENSITY depth (2x-1)^3: 1, .5, .25, 0, -.5, -1;
  -- MOD_TARGET_INTENSITY depth x^2: .5, .25 (mod_par_laws.lua). Last step repeats 100 %.
  local steps = { {2, 1000000}, {2, 896850}, {2, 814980}, {2, 500000}, {2, 103150}, {2, 0}, {3, 707107}, {3, 500000}, {2, 1000000} }
  local a = load()
  local ev = {}
  for _, s in ipairs(steps) do
    ev[#ev + 1] = {t - 0.1, s[1], s[2]}
    t = phrase(ev, t, {60, 72, 60})
  end
  log(string.format('{"A":%s,"steps":%d,"end":%.2f}', tostring(pcall(kt.set_instrument_script_source, a, 0, script(ev))), #steps, t))

  -- B: group 0 saved as a group file and loaded into a new group 1.
  local okg = pcall(kt.save_group, a, 0, NKG, {})
  local function two(name, high)
    local i = load()
    local g = kt.add_group(i)
    local okl, e = pcall(kt.load_group, i, g, NKG, { replace_zones = true })
    sine_zone(i, g, high)
    kt.set_group_pan(i, 0, -100)
    kt.set_group_pan(i, g, 100)
    local zs = {}
    for z = 0, kt.get_num_zones(i) - 1 do zs[#zs + 1] = kt.get_zone_group(i, z) end
    log(string.format('{"%s":{"save":%s,"load":%s,"err":"%s","groups":%d,"zone_groups":"%s"}}', name, tostring(okg),
      tostring(okl), tostring(e):gsub('"', "'"), kt.get_num_groups(i), table.concat(zs, ",")))
    return i
  end
  local b = two("B")
  ev = {}
  -- B1: 60 both, 67 only g0, 72 both  -> g1 from 60 (own) or 67 (instrument)?
  -- B2: 60 both, 67 only g1, 72 only g0 -> g0 from 60 (own) or 67?
  -- B3: 60 both, 67 both, 55 only g1, 72 both -> g0 from 67 (own) or 55?
  t = phrase(ev, t, {60, 67, 72}, {-1, 0, -1})
  t = phrase(ev, t, {60, 67, 72}, {-1, 1, 0})
  t = phrase(ev, t, {60, 67, 55, 72}, {-1, -1, 1, -1})
  log(string.format('{"B":%s,"end":%.2f}', tostring(pcall(kt.set_instrument_script_source, b, 0, script(ev))), t))

  -- B4: group 1's zones only up to key 66. 60 both, 72 (g0 only: no g1 zone), 64 both.
  local c = two("B4", 66)
  ev = {}
  t = phrase(ev, t, {60, 72, 64})
  log(string.format('{"B4":%s,"end":%.2f}', tostring(pcall(kt.set_instrument_script_source, c, 0, script(ev))), t))

end
local ok, err = xpcall(main, debug and debug.traceback or tostring)
if not ok then log('{"error":"' .. tostring(err):gsub('"', "'"):gsub("\n", " | ") .. '"}') end
log('{"ready":true}')
out:close()
