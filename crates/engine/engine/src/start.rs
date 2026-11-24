//! Start voices for the zones selected by a note.
use kontra_behaviour::Step;

use crate::*;

impl Engine {
    pub(crate) fn start_zones(
        &mut self,
        note: NoteRef,
        attack: bool,
        accept: impl Fn(Trigger) -> bool,
    ) {
        let Some(mut n) = self.notes.get(note).copied() else {
            return;
        };
        let plan = self.plan.clone();
        let key = usize::from(n.key);
        // Kontakt's system script restarts the key's release-trigger counter
        // with every ordinary note; a script that plays its releases does not.
        let restarts = attack && self.plan.release_zones == ReleaseZones::AtRelease;
        if restarts && let Some(r) = self.behaviour.rls_reset.get_mut(key) {
            *r = self.now.0;
        }
        let step = if attack { Step::Advance } else { Step::Reuse };
        let since = self.behaviour.rls_reset.get(key).map_or(n.at.0, |&r| r);
        let held_seconds = if n.key_up {
            n.held
        } else {
            self.now.0.saturating_sub(since) as f32 / self.rate as f32
        };
        // Every route reads the counter through the note, shaped or not.
        n.held = held_seconds;
        if let Some(live) = self.notes.get_mut(note) {
            live.held = held_seconds;
        }
        // A key's zones are a few of thousands: walk the key's list, or the
        // packed key ranges when the plan's zones moved, never every zone.
        let index_of = Arc::clone(&self.zone_index);
        let listed = index_of.listed(n.key);
        let count = listed.map_or(plan.zones.len(), <[u32]>::len);
        for at in 0..count {
            let index = match listed.and_then(|l| l.get(at)) {
                Some(&zone) => zone as usize,
                None => at,
            };
            if !index_of.pre_matches(index, n.key, n.velocity_byte, &accept) {
                continue;
            }
            let Some(zone) = plan.zones.get(index) else {
                continue;
            };
            let selected = accept(zone.trigger)
                && self.masks.allows(note.slot(), zone.group)
                && self
                    .groups
                    .get(zone.group as usize)
                    .is_none_or(|g| !g.purged && !g.muted)
                && zone.keys.contains(&n.key)
                && zone.velocities.contains(&n.velocity_byte)
                && (n.zone_slot < 0 || n.zone_slot == i32::from(zone.slot))
                && (self.only_voice_group).is_none_or(|v| zone.voice_group == Some(v))
                && self
                    .buses
                    .get(plan::bus(&self.groups, zone))
                    .is_some_and(|b| b.enabled);
            if !selected
                || !self
                    .behaviour
                    .starts(zone, n.host.port_channel(), n.key, n.draw, step)
            {
                continue;
            }
            // Before the limits steal the note it may slide from.
            let slide = self.slide_for(zone.group, note, n.key);
            if let Some(vg) = zone.voice_group
                && self.mono_line(vg, note, slide)
            {
                continue;
            }
            if let Some(vg) = zone.voice_group {
                self.choke_by(vg, note);
            }
            self.limit_group(zone.group);
            if let Some(vg) = zone.voice_group {
                self.limit_voice_group(vg, note);
            }
            let factor = if zone.trigger == Trigger::Release {
                crate::ir::common::DB_GAIN.eval_unclamped(-zone.release_decay_db * held_seconds)
            } else {
                1.0
            };
            #[cfg(feature = "trace")]
            if let Some(trace) = &self.trace {
                trace.routes.begin_onset(index);
            }
            let mut region = zone.region.clone();
            let span = region.end.saturating_sub(region.start);
            let script = u64::from(n.offset) * u64::from(zone.sample_rate) / 1_000_000;
            let script = if zone.offset_limited {
                if script > zone.start_mod { 0 } else { script }
            } else {
                script.min(span)
            };
            let now_secs = self.now.0 as f32 / self.rate as f32;
            let pre_age = modulators::Age {
                now: now_secs,
                ..modulators::Age::START
            };
            let mut lfo_phase = std::mem::take(&mut self.lfo_phase_scratch);
            self.lfo_phase_offsets(zone, &n, pre_age, &mut lfo_phase);
            let pre_onset = modulators::Age {
                lfo_phase: &lfo_phase,
                ..pre_age
            };
            let frames = self.start_offset(zone, &n, pre_onset);
            if region.reverse {
                // Kontakt reverse offset: subtract from end.
                region.end = region
                    .end
                    .saturating_sub(script)
                    .saturating_add_signed(frames.saturating_neg())
                    .clamp(region.start, zone.region.end);
            } else {
                region.start = region
                    .start
                    .saturating_add(script)
                    .saturating_add_signed(frames)
                    .clamp(zone.region.start, region.end);
            }
            // A reversed region shifts its mirrored loops the other way.
            let loop_shift = match self.loop_shift(zone, &n, pre_onset) {
                [start, length] if region.reverse => [-start, -length],
                shift => shift,
            };
            let start_frame = if region.reverse {
                zone.region.end.saturating_sub(region.end) as f64
            } else {
                region.start as f64
            };
            let timing_age = modulators::Age {
                now: now_secs,
                ..modulators::Age::at_start(start_frame)
            };
            // Read first: the onset targets below see the stretched envelopes
            // and the note-on latched LFO phase offsets.
            let mut stretch = std::mem::take(&mut self.stretch_scratch);
            self.stretches(
                zone,
                &n,
                modulators::Age {
                    lfo_phase: &lfo_phase,
                    ..timing_age
                },
                &mut stretch,
            );
            let onset_base = modulators::Age {
                stretch: &stretch,
                ..timing_age
            };
            self.lfo_phase_offsets(zone, &n, onset_base, &mut lfo_phase);
            let onset = modulators::Age {
                lfo_phase: &lfo_phase,
                ..onset_base
            };
            let mix = self.voice_mix(zone, &n, onset);
            let read = self.voice_read(zone, &n, onset);
            let pan = self.group_pan(zone);
            let (gain, lag) = self.voice_gain(zone, &n, factor, onset, pan);
            let (step, bend) = self.step(zone, &n, onset);
            let release_base = self
                .groups
                .get(zone.group as usize)
                .map_or(0, |g| g.envelope.release);
            let start = VoiceStart {
                region,
                loop_shift,
                mix,
                read,
                oscillator: self
                    .groups
                    .get(zone.group as usize)
                    .and_then(|g| g.oscillator),
                granular: self
                    .groups
                    .get(zone.group as usize)
                    .and_then(|g| g.granular)
                    .map(|spec| kontra_voice::Granular::new(spec, self.rate)),
                step: voice_gain::bent(
                    self.plan.route_pitch,
                    step,
                    bend.instant + bend.lagged() + slide.map_or(0.0, |s| s.offset(self.now.0)),
                ),
                gain,
                lag,
                envelope: self.envelope(zone, &n, onset),
                fade: n.fade.filter(|_| attack),
                speed: (self.groups.get(zone.group as usize))
                    .and_then(|g| g.speed)
                    .map_or(1.0, f64::from),
            };
            let now = self.now;
            let Some(i) = self.free_slot() else {
                #[cfg(feature = "trace")]
                if let Some(trace) = &self.trace {
                    trace.routes.finish_onset(None);
                }
                self.lfo_phase_scratch = lfo_phase;
                self.stretch_scratch = stretch;
                continue;
            };
            #[cfg(feature = "trace")]
            if let Some(trace) = &self.trace {
                trace.routes.finish_onset(Some(i));
            }
            #[cfg(feature = "trace")]
            if self.trace.is_some()
                && let Some(group) = self.groups.get(zone.group as usize)
            {
                use ir::{EnvStage as S, LfoParam as L, ParamAddr as A};
                for m in group.mods.iter().filter(|m| !m.bypass) {
                    let route = m.route;
                    let (onset_timed, lfo_start) = match route.target {
                        A::EnvelopeStage {
                            stage: S::Attack | S::Release,
                            ..
                        } => (true, false),
                        A::LfoParam {
                            param: L::Phase | L::Rate,
                            ..
                        } => (true, true),
                        A::SampleStart | A::LoopShift { .. } | A::EnvelopeStage { .. } => {
                            (false, false)
                        }
                        _ => continue,
                    };
                    let source = route.driver().and_then(|s| match s {
                        ir::Source::Lfo(_) if lfo_start => None,
                        _ if onset_timed => self.source(zone, s, &n, onset),
                        _ => self.source(zone, s, &n, onset),
                    });
                    if let Some(trace) = &mut self.trace {
                        trace.push(TraceEvent::RouteOnset {
                            frame: now.0,
                            voice_slot: i,
                            zone: index,
                            mod_slot: m.slot,
                            target: m.target,
                            route,
                            source,
                        });
                    }
                }
            }
            let Some(slot) = self.voices.get_mut(i) else {
                self.lfo_phase_scratch = lfo_phase;
                self.stretch_scratch = stretch;
                continue;
            };
            #[cfg(feature = "trace")]
            if let Some(trace) = &mut self.trace {
                trace.push(TraceEvent::Voice {
                    frame: now.0,
                    slot: i,
                    zone: index,
                    event: VoiceEvent::Start,
                    position: start.region.start as f64,
                    step: start.step,
                    gain: start.gain.map(|row| {
                        let lagged: f32 = start.lag.iter().map(|l| l.gain).product();
                        row.map(|v| v * lagged)
                    }),
                });
            }
            slot.voice.start(start);
            slot.note = Some(note);
            slot.zone = index;
            slot.started = now;
            slot.factor = factor;
            slot.bend = bend.lagging.map(|(st, _)| [st; 2]);
            slot.pan = [pan; 2];
            slot.warp.fill(0.0);
            slot.chaos.fill(modulators::CHAOS_START);
            slot.release_base = release_base;
            slot.slide = slide;
            slot.start_frame = start_frame;
            #[cfg(feature = "trace")]
            slot.pending_trace_cc.set(0);
            if slide.is_some() {
                self.set_pitch(i, step, bend.instant + bend.lagged());
            }
            #[cfg(feature = "trace")]
            if let Some(trace) = &self.trace {
                trace.routes.begin_onset(index);
            }
            self.start_rack(i, zone, &n, onset);
            if let Some(slot) = self.voices.get_mut(i) {
                std::mem::swap(&mut slot.lfo_phase, &mut lfo_phase);
                std::mem::swap(&mut slot.stretch, &mut stretch);
            }
            self.lfo_phase_scratch = lfo_phase;
            self.stretch_scratch = stretch;
            #[cfg(feature = "trace")]
            if let Some(trace) = &self.trace {
                trace.routes.finish_onset(Some(i));
            }
        }
    }
}
