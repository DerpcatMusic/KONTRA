//! Effect parameter bindings measured on real Kontakt 8.13.1 (tools/kontakt-oracle/probe_fx.py).
//! Each fixture is an instrument Kontakt itself saved after a KSP `set_engine_par(PAR, VALUE, ..)`
//! on a freshly loaded factory effect preset. The test decodes the effect with Kontakt's own
//! reader programs and checks the stored field against the law recorded in
//! docs/re/kontakt-bindings/*.tsv (field index, normalized value -> stored value).

use ni_file::kontakt::{Chunk, StructuredObject, native_schema, objects::BParamArrayBParFX8};
use std::io::Cursor;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// (fixture, effect preset folder, ENGINE_PAR suffix, set value)
const CASES: &[(&str, &str, &str, f64)] = &[
    ("Effects-ACBox_AC_BASS_900000", "Effects/ACBox", "AC_BASS", 900000.),
    ("Filters-EQ_FREQ1_700000", "Filters/EQ", "FREQ1", 700000.),
    ("Filters-EQ_GAIN1_700000", "Filters/EQ", "GAIN1", 700000.),
    ("Filters-Solid-G-EQ_SEQ_LF_FREQ_700000", "Filters/Solid G-EQ", "SEQ_LF_FREQ", 700000.),
    ("Filters-Solid-G-EQ_SEQ_HP_1", "Filters/Solid G-EQ", "SEQ_HP", 1.),
    ("Effects-Compressor_RATIO_500000", "Effects/Compressor", "RATIO", 500000.),
    ("Effects-Skreamer_SK_DRIVE_700000", "Effects/Skreamer", "SK_DRIVE", 700000.),
    ("Effects-Delay_DL_TIME_700000", "Effects/Delay", "DL_TIME", 700000.),
    ("Effects-Reverb_RV_SIZE_700000", "Effects/Reverb", "RV_SIZE", 700000.),
];

/// Evaluate a law string from the binding tables at normalized `n`.
fn law(text: &str, n: f64, raw: f64) -> Option<f64> {
    let num = |key: &str| -> f64 {
        let at = text.find(key).unwrap() + key.len();
        text[at..].split_whitespace().next().unwrap().parse().unwrap()
    };
    Some(match text.split_whitespace().next()? {
        "identity" => n,
        "linear" => num("a=") + num("b=") * n,
        "exp" => num("a=") * num("r=").powf(n),
        "pow" => num("a=") * n.powf(num("p=")),
        "bool:" => (raw >= num("value>=")) as u8 as f64,
        _ => return None,
    })
}

/// Curve laws without a closed form are tabulated every 10000 units in curves_dense.tsv.
fn dense(effect: &str, par: &str, value: f64) -> f64 {
    let text = std::fs::read_to_string(root().join("../../docs/re/kontakt-bindings/curves_dense.tsv")).unwrap();
    text.lines()
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .find(|c| c[0] == effect && c[1] == par && c[2].parse::<f64>().unwrap() == value)
        .unwrap_or_else(|| panic!("{effect} {par}={value}: neither a closed-form law nor a dense row"))[3]
        .parse()
        .unwrap()
}

fn binding(effect: &str, par: &str) -> (usize, String) {
    let dir = root().join("../../docs/re/kontakt-bindings");
    for entry in std::fs::read_dir(dir).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        for line in text.lines() {
            let c: Vec<&str> = line.split('\t').collect();
            if c.len() > 8 && c[0] == effect && c[1] == format!("$ENGINE_PAR_{par}") && c[2] != "BParFX" {
                return (c[4].parse().unwrap(), c[7].to_string());
            }
        }
    }
    panic!("no binding row for {effect} {par}");
}

/// Field `idx` of the effect object in the first occupied insert/send slot of `file`.
fn stored(file: &str, idx: usize) -> f64 {
    let path = root().join(format!("tests/fixtures/bindings/{file}.nki"));
    let chunks = sampler_kontakt::read_chunks(&path).unwrap();
    let program = chunks.find_first(0x28).unwrap();
    let program = StructuredObject::read(Cursor::new(&program.data)).unwrap();
    for rack in program.children.iter().filter(|c| c.id == 0x3a) {
        let array = BParamArrayBParFX8::read(Cursor::new(&rack.data), 8).unwrap();
        for slot in array.items.iter().flatten() {
            let wrapper = StructuredObject::read(Cursor::new(&slot.data)).unwrap();
            let Some(Chunk { id, data }) = wrapper.children.first() else { continue };
            let effect = StructuredObject::read(Cursor::new(data)).unwrap();
            let d = native_schema::decode(*id, effect.version, &effect.public_data).unwrap();
            let f = d.fields.iter().filter(|f| f.depth == 0).nth(idx).unwrap();
            return f.as_f32().map(f64::from).unwrap_or_else(|| f.value.unwrap() as f64);
        }
    }
    panic!("{file}: no effect in any rack");
}

#[test]
fn stored_effect_fields_follow_the_measured_binding_laws() {
    for &(fixture, effect, par, value) in CASES {
        let (field, law_text) = binding(effect, par);
        let want = law(&law_text, value / 1e6, value).unwrap_or_else(|| dense(effect, par, value));
        let got = stored(fixture, field);
        assert!(
            (got - want).abs() <= 1e-5 * want.abs().max(1.0),
            "{effect} {par}={value}: field {field} stored {got}, law `{law_text}` gives {want}"
        );
    }
}
