-- One group per source mode with non-default source controls (only those the
-- file saves), set by an `on init` script and saved as
-- crates/format/kontakt/tests/data/source-pars.nki; disp_sweep.py --setup this,
-- then GET/<g>/-1/-1 reads what Kontakt holds for each.
local modes = { "sampler", "dfd", "tone_machine", "time_machine_1", "time_machine_2", "time_machine_pro", "beat_machine", "wavetable" }
for n, m in ipairs(modes) do
  local g = n == 1 and 0 or kt.add_group(i)
  pcall(kt.set_group_playback_mode, i, g, m)
end
local sets = {
  { 0, "HQI_MODE", 1 }, { 0, "TRACKING", 0 },
  { 2, "SMOOTH", 250000 }, { 2, "FORMANT", 750000 },
  { 3, "SMOOTH", 333333 }, { 3, "GRAIN_LENGTH", 250000 }, { 3, "TM_LEGATO", 1 }, { 3, "HQI_MODE", 2 },
  { 5, "ENVELOPE_ORDER", 600000 }, { 5, "FORMANT_SHIFT", 900000 }, { 5, "TMPRO_KEEP_FORMANTS", 0 },
  { 6, "SLICE_ATTACK", 100000 }, { 6, "SLICE_RELEASE", 900000 },
}
local src = "on init\n"
for _, s in ipairs(sets) do
  src = src .. string.format(" set_engine_par($ENGINE_PAR_%s, %d, %d, -1, -1)\n", s[2], s[3], s[1])
end
pcall(kt.set_instrument_script_source, i, 0, src .. "end on\n")
kt.save_instrument(i, [[Z:/data/dev/workspace/BuildScratch/oracle-probe/sweeps/source-pars.nki]], {})
