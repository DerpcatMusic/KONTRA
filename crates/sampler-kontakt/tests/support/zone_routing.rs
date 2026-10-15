//! Independently authored NKS/WAV fixture, no commercial source or binary payload.
use std::path::{Path, PathBuf};
#[path = "snapshot_reuse.rs"]
mod snapshot;
pub struct Fixture {
    pub directory: tempfile::TempDir,
    pub base: PathBuf,
    pub a: PathBuf,
    pub b: PathBuf,
}
fn sized(b: &[u8]) -> Vec<u8> {
    let mut v = (b.len() as u32).to_le_bytes().to_vec();
    v.extend(b);
    v
}
fn wide(s: &str) -> Vec<u8> {
    let mut v = (s.encode_utf16().count() as u32).to_le_bytes().to_vec();
    v.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
    v
}
fn object(version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Vec<u8> {
    let mut v = vec![1];
    v.extend(version.to_le_bytes());
    for b in [private, public, children] {
        v.extend(sized(b));
    }
    v
}
fn chunk(id: u16, b: &[u8]) -> Vec<u8> {
    let mut v = id.to_le_bytes().to_vec();
    v.extend(sized(b));
    v
}
pub const SCRIPT: &str = "on init
 set_snapshot_type(3)
 declare $choice := 0
 make_persistent($choice)
 declare $current := 0
 declare $job
 declare $before
 declare $after
 declare $completions
 declare $count := get_num_zones()
 declare $zero := get_zone_id(0)
 declare $sparse := get_zone_id(2)
end on
on persistence_changed
 $completions := 0
 set_zone_par($current,$ZONE_PAR_GROUP,1)
 set_zone_par($current,$ZONE_PAR_LOW_KEY,0)
 set_zone_par($current,$ZONE_PAR_HIGH_KEY,0)
 $job := set_zone_par($choice,$ZONE_PAR_GROUP,0)
 set_zone_par($choice,$ZONE_PAR_LOW_KEY,60)
 set_zone_par($choice,$ZONE_PAR_HIGH_KEY,60)
 $before := get_zone_par($choice,$ZONE_PAR_GROUP)
 $current := $choice
 wait_async($job)
 $after := get_zone_par($choice,$ZONE_PAR_GROUP)
end on
on async_complete
 inc($completions)
end on";
fn group(wave: bool) -> Vec<u8> {
    let mut public = wide(if wave { "Owned WT" } else { "Owned storage" });
    for x in [1f32, 0., 1.] {
        public.extend(x.to_le_bytes());
    }
    public.extend([1, 0, 0, 0]);
    public.extend(0i32.to_le_bytes());
    public.extend((-1i16).to_le_bytes());
    public.extend([0; 10]);
    public.extend(0i32.to_le_bytes());
    let mut private = Vec::new();
    for _ in 0..136 {
        private.extend(8u32.to_le_bytes());
        private.extend([0; 8]);
    }
    private.extend([0; 24]);
    private.extend([0, 0x13, 0]);
    private.extend(8u32.to_le_bytes());
    private.extend([0; 8]);
    private.push(1);
    if wave {
        let mut common = [0u8; 30];
        common[..7].copy_from_slice(&[0, 6, 1, 9, 0, 0, 0]);
        ni_file::kontakt::objects::WavetableSource {
            common,
            position: 0.,
            form1: 0.,
            phase: 0.,
            phase_random: 0.,
            form_type: 1,
            quality: 1,
            inharmonic_enabled: false,
            inharmonic: 0.,
            form2: 0.,
            form2_type: 1,
            mod_wave: 0,
            mod_type: 0,
            mod_amount: 0.,
            mod_tune: 0.,
            unknown_tail: [0; 16],
        }
        .write(&mut private)
        .unwrap();
    } else {
        private.extend([0, 6, 1]);
        private.extend(0u32.to_le_bytes());
        private.extend([0; 23]);
    }
    private.extend([255; 22]);
    let children = chunk(0x38, &0u32.to_le_bytes());
    object(0x95, &private, &public, &children)
}
pub fn zone_record(id: Option<i32>, group: u32, low: i16, high: i16, file: i32) -> Vec<u8> {
    let mut public = vec![0; 12];
    for x in [1i16, 127, low, high, 0, 0, 0, 0, 60] {
        public.extend(x.to_le_bytes());
    }
    for x in [1f32, 0., 1.] {
        public.extend(x.to_le_bytes());
    }
    public.extend([0, 1, 255, 255, 255, 255]);
    for x in [file, 0, 48000] {
        public.extend(x.to_le_bytes());
    }
    public.push(1);
    for x in [4096i32, 0, 60] {
        public.extend(x.to_le_bytes());
    }
    public.extend(0f32.to_le_bytes());
    public.push(0);
    public.extend(0i32.to_le_bytes());
    let mut private = vec![0; 87];
    private[83..87].copy_from_slice(&id.unwrap_or(-1).to_le_bytes());
    let mut v = group.to_le_bytes().to_vec();
    v.extend(object(0x9c, &private, &public, &[]));
    v
}
pub fn base(script: Option<&str>, selected: i32, ids: [Option<i32>; 3]) -> Vec<u8> {
    let mut public = wide("Owned zone routing");
    public.extend(0f64.to_le_bytes());
    public.push(0);
    for x in [1f32, 0., 1.] {
        public.extend(x.to_le_bytes());
    }
    public.extend([1, 127, 0, 127]);
    public.extend((-1i16).to_le_bytes());
    public.extend([0; 16]);
    public.push(0);
    public.extend(0i32.to_le_bytes());
    for _ in 0..3 {
        public.extend(wide(""));
    }
    public.extend([0; 6]);
    let mut groups = 2u32.to_le_bytes().to_vec();
    groups.extend(group(true));
    groups.extend(group(false));
    let mut zones = 3u32.to_le_bytes().to_vec();
    for (index, id) in ids.into_iter().enumerate() {
        let selected = id == Some(selected);
        zones.extend(zone_record(
            id,
            if selected { 0 } else { 1 },
            if selected { 60 } else { 0 },
            if selected { 60 } else { 0 },
            index as i32,
        ));
    }
    let mut children = chunk(0x33, &groups);
    children.extend(chunk(0x34, &zones));
    if let Some(s) = script {
        let mut p = sized(s.as_bytes());
        p.extend([0, 0, 0]);
        p.extend(sized(&[]));
        p.extend(sized(&[]));
        p.extend(u32::MAX.to_le_bytes());
        p.extend(0u32.to_le_bytes());
        children.extend(chunk(6, &object(0x60, &[], &p, &[])));
    }
    let mut payload = chunk(0x28, &object(0x90, &[], &public, &children));
    let mut files = vec![0; 4];
    files.extend(3u32.to_le_bytes());
    for name in ["zero.wav", "seven.wav", "tail.wav"] {
        files.extend(1i32.to_le_bytes());
        files.push(4);
        files.extend(wide(name));
    }
    // Legacy tables store one u64 timestamp per sample, then the other-file
    // count. Three names therefore need 24 timestamp bytes, not one timestamp.
    files.extend([0; 3 * 8]);
    files.extend(0u32.to_le_bytes());
    payload.extend(chunk(0x3d, &files));
    snapshot::container(&payload)
}
fn wav(kind: usize) -> Vec<u8> {
    let mut v = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0".to_vec();
    v.extend(1u16.to_le_bytes());
    v.extend(1u16.to_le_bytes());
    v.extend(48000u32.to_le_bytes());
    v.extend(96000u32.to_le_bytes());
    v.extend(2u16.to_le_bytes());
    v.extend(16u16.to_le_bytes());
    v.extend(b"data");
    v.extend(8192u32.to_le_bytes());
    for i in 0..4096 {
        let phase = (i % 2048) as f64 / 2048.;
        let value = match kind {
            0 => (phase * std::f64::consts::TAU).sin(),
            1 => 2. * phase - 1.,
            _ => {
                if phase < 0.5 {
                    1.
                } else {
                    -1.
                }
            }
        };
        v.extend(((value * 12000.) as i16).to_le_bytes());
    }
    let n = v.len() as u32 - 8;
    v[4..8].copy_from_slice(&n.to_le_bytes());
    v
}
pub fn fixture() -> Fixture {
    let directory = snapshot::directory();
    let root = directory.path();
    for (i, name) in ["zero.wav", "seven.wav", "tail.wav"].iter().enumerate() {
        std::fs::write(root.join(name), wav(i)).unwrap();
    }
    let base = root.join("owned.nki");
    std::fs::write(
        &base,
        self::base(Some(SCRIPT), 0, [Some(0), Some(7), Some(2018)]),
    )
    .unwrap();
    let a = root.join("A.nksn");
    let b = root.join("B.nksn");
    for (path, row) in [(&a, "$choice 0"), (&b, "$choice 7")] {
        let rows = [row];
        std::fs::write(
            path,
            snapshot::script_only_snapshot("owned", "", &[&rows, &[], &[], &[], &[]]),
        )
        .unwrap();
    }
    Fixture {
        directory,
        base,
        a,
        b,
    }
}
pub fn static_base(root: &Path, selected: i32) -> PathBuf {
    let p = root.join(format!("static-{selected}.nki"));
    std::fs::write(&p, base(None, selected, [Some(0), Some(7), Some(2018)])).unwrap();
    p
}
