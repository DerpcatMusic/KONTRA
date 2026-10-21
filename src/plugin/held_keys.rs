//! Physical host key state. Sounding gates may outlive key-up (pedals,
//! one-shots and script children), or end while a physical key is still down.
use super::{CoreEvent, Event, EventBody, HostNote, HostPattern, Ordering, Shared};

pub(super) struct HeldKeys {
    presses: Vec<(HostNote, u8)>,
    levels: [u8; 128],
}

impl Default for HeldKeys {
    fn default() -> Self {
        Self {
            presses: Vec::with_capacity(8192),
            levels: [0; 128],
        }
    }
}

impl HeldKeys {
    pub fn clear(&mut self) {
        self.presses.clear();
        self.levels.fill(0);
    }

    fn press(&mut self, note: HostNote, velocity: u8) {
        if let Some((_, v)) = self.presses.iter_mut().find(|(n, _)| *n == note) {
            *v = velocity;
        } else if self.presses.len() < self.presses.capacity() {
            self.presses.push((note, velocity));
        }
        self.refresh();
    }

    fn release(&mut self, pattern: HostPattern) {
        self.presses.retain(|(n, _)| !pattern.matches(*n));
        self.refresh();
    }

    fn refresh(&mut self) {
        self.levels.fill(0);
        for (note, velocity) in &self.presses {
            let level = &mut self.levels[usize::from(note.key & 127)];
            *level = (*level).max(*velocity);
        }
    }

    pub fn exact(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::NoteOn { note, velocity, .. } => {
                self.press(note, ((velocity * 127.).round() as u8).clamp(1, 127))
            }
            CoreEvent::NoteOff(p) | CoreEvent::NoteOffVelocity(p, _) | CoreEvent::Choke(p) => {
                self.release(p)
            }
            _ => {}
        }
    }

    pub fn typed(&mut self, event: &Event) {
        let note = |channel, key| HostNote {
            port: event.port,
            channel,
            key,
            id: -1,
            clap: false,
        };
        let off = |channel, key| HostPattern {
            port: i32::from(event.port),
            channel: i32::from(channel),
            key: i32::from(key),
            id: -1,
            clap: false,
        };
        match event.body {
            EventBody::NoteOn {
                channel,
                note: key,
                velocity: 0,
                ..
            }
            | EventBody::NoteOff {
                channel, note: key, ..
            }
            | EventBody::NoteOff2 {
                channel, note: key, ..
            } => self.release(off(channel, key)),
            EventBody::NoteOn {
                channel,
                note: key,
                velocity,
                ..
            } => self.press(note(channel, key), velocity),
            EventBody::NoteOn2 {
                channel,
                note: key,
                velocity,
                ..
            } => self.press(note(channel, key), ((velocity >> 9) as u8).clamp(1, 127)),
            EventBody::ControlChange {
                channel,
                cc: 120 | 123,
                ..
            } => {
                self.presses
                    .retain(|(n, _)| n.port != event.port || n.channel != channel);
                self.refresh();
            }
            _ => {}
        }
    }

    pub fn publish(&self, shared: &Shared) {
        for (cell, value) in shared.heard.iter().zip(self.levels) {
            cell.store(value, Ordering::Relaxed);
        }
    }
}
