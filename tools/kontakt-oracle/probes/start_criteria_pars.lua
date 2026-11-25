-- Groups with start options and voice groups whose $ENGINE_PAR_START_CRITERIA_*
-- and $ENGINE_PAR_VOICE_GROUP reads were measured (disp_sweep.py --setup, GET/<g>/<c>/-1),
-- saved as crates/format/kontakt/tests/data/start-criteria.nki.
local g1, g2, g3, g4 = kt.add_group(i), kt.add_group(i), kt.add_group(i), kt.add_group(i)
kt.set_group_start_options(i, 0, { { mode = "round_robin", position = 2 } })
kt.set_group_start_options(i, g1, { { mode = "key", key_min = 36, key_max = 40, next = "and" }, { mode = "round_robin", position = 3 } })
kt.set_group_start_options(i, g2, { { mode = "controller", controller = 11, cc_min = 10, cc_max = 90, next = "or" }, { mode = "key", key_min = 50, key_max = 52, next = "and_not" }, { mode = "random" } })
kt.set_group_start_options(i, g3, { { mode = "random" } })
kt.set_voice_groups(i, { { mode = "oldest", voices = 4, fade_time = 10, prefer_released = false }, { mode = "oldest", voices = 2, fade_time = 10, prefer_released = false } })
kt.set_voice_group(i, g1, 1)
kt.set_voice_group(i, g2, 2)
