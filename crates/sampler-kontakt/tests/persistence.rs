use sampler_kontakt::{ArrayTail, Chunks, ErrorKind, Limits, SavedEntry, SavedValue, Script};
use sampler_ksp::model::WidgetKind as Widget;
#[path = "support/chunks.rs"]
mod fixture;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
const LIMITS: Limits = Limits {
    bytes: 4096,
    records: 128,
};

#[test]
fn menu_indices_and_array_tail_encoding_require_declaration_context() {
    support::without_heap(|| {
        let entry = |b, d| SavedEntry::parse(b, d, LIMITS).unwrap();
        assert_eq!(entry(b"$curve 2", None).value, SavedValue::Int(2));
        assert_eq!(
            entry(b"$curve 2", Some(Widget::Menu)).value,
            SavedValue::MenuIndex(2)
        );
        // The third menu item can have KSP value 80, even though the file says 2.
        let items = [0, 20, 80];
        let SavedValue::MenuIndex(index) = entry(b"$curve 2", Some(Widget::Menu)).value else {
            panic!()
        };
        assert_eq!(items[index as usize], 80);
        for d in [
            Some(Widget::Button),
            Some(Widget::Switch),
            Some(Widget::Knob),
            Some(Widget::Slider),
            Some(Widget::ValueEdit),
            Some(Widget::Waveform),
            Some(Widget::Wavetable),
        ] {
            assert_eq!(entry(b"$control 1", d).value, SavedValue::Int(1));
        }
        for (d, expected) in [
            (None, ArrayTail::RepeatLast),
            (Some(Widget::Table), ArrayTail::Exact),
        ] {
            let SavedValue::Ints { values, tail } = entry(b"%steps 7 -3 0 ", d).value else {
                panic!()
            };
            assert_eq!(values.len(), 3);
            assert_eq!(tail, expected);
            assert!(values.iter().eq([7, -3, 0]));
        }
        let SavedValue::Reals { values, tail } = entry(b"?xy 0.25 1e-2 ", Some(Widget::Xy)).value
        else {
            panic!()
        };
        assert_eq!(tail, ArrayTail::Exact);
        assert!(values.iter().eq([0.25, 0.01]));
        assert_eq!(entry(b"~gain -0.125", None).value, SavedValue::Real(-0.125));
    });
}

#[test]
fn string_payloads_keep_whitespace_encoding_and_empty_array_cells() {
    support::without_heap(|| {
        for d in [None, Some(Widget::TextEdit)] {
            let v = SavedEntry::parse(b"@text  two\nlines\r\xff ", d, LIMITS)
                .unwrap()
                .value;
            assert_eq!(v, SavedValue::Text(b" two\nlines\r\xff "));
            assert_eq!(
                SavedEntry::parse(b"@text ", d, LIMITS).unwrap().value,
                SavedValue::Text(b"")
            );
        }
        for (wire, expected) in [
            (
                b"!names red blue\n\nlast\n".as_slice(),
                [b"red blue".as_slice(), b"", b"last"],
            ),
            (
                b"!names red blue\n\nlast".as_slice(),
                [b"red blue".as_slice(), b"", b"last"],
            ),
        ] {
            let SavedValue::Texts(values) = SavedEntry::parse(wire, None, LIMITS).unwrap().value
            else {
                panic!()
            };
            assert_eq!(values.len(), 3);
            assert!(values.iter().eq(expected));
        }
        let SavedValue::Texts(values) =
            SavedEntry::parse(b"!names \n", None, LIMITS).unwrap().value
        else {
            panic!()
        };
        assert!(values.iter().eq([b"".as_slice()]));
    });
}

#[test]
fn damaged_unknown_and_mismatched_records_are_errors_without_allocating() {
    support::without_heap(|| {
        for b in [
            b"$x".as_slice(),
            b"$ 1",
            b"$x 2147483648",
            b"$x 1 2",
            b"~x rubbish",
            b"%x ",
            b"%x 1 broken",
            b"$x 1\0",
            b"$bad-name 1",
        ] {
            assert_eq!(
                SavedEntry::parse(b, None, LIMITS).unwrap_err().kind,
                ErrorKind::InvalidSavedValue,
                "{b:?}"
            );
        }
        assert_eq!(
            SavedEntry::parse(b"#x 1", None, LIMITS).unwrap_err().kind,
            ErrorKind::UnsupportedLayout
        );
        for d in [
            Some(Widget::Label),
            Some(Widget::FileSelector),
            Some(Widget::LevelMeter),
        ] {
            assert_eq!(
                SavedEntry::parse(b"$x 1", d, LIMITS).unwrap_err().kind,
                ErrorKind::UnsupportedLayout
            );
        }
        assert!(SavedEntry::parse(b"@menu 1", Some(Widget::Menu), LIMITS).is_err());
        let limits = Limits {
            records: 1,
            ..LIMITS
        };
        for b in [b"%x 1 2".as_slice(), b"!x a\nb\n"] {
            assert_eq!(
                SavedEntry::parse(b, None, limits).unwrap_err().kind,
                ErrorKind::Limit
            );
        }
        assert_eq!(
            SavedEntry::parse(b"$x 1", None, Limits { bytes: 2, ..LIMITS })
                .unwrap_err()
                .kind,
            ErrorKind::Limit
        );
    });
}

#[test]
fn typed_script_entries_retain_their_absolute_wire_offsets() {
    let mut public = fixture::sized(b"on init end on");
    public.extend([0, 0, 0]);
    public.extend(fixture::sized(b""));
    public.extend(u32::MAX.to_le_bytes()); // description and linked script absent
    public.extend(u32::MAX.to_le_bytes());
    public.extend(2u32.to_le_bytes());
    public.extend(fixture::sized(b"$menu 2"));
    public.extend(fixture::sized(b"%steps 4 0 "));
    let wire = fixture::chunk(6, &fixture::object(0x60, &[], &public, &[]));
    support::without_heap(|| {
        let chunks = Chunks::parse(&wire, LIMITS).unwrap();
        let script = Script::parse(chunks.iter().next().unwrap(), LIMITS).unwrap();
        let mut entries = script.persistent.unwrap().iter();
        let bytes = entries.next().unwrap();
        let menu = SavedEntry::from_bytes(bytes, Some(Widget::Menu), LIMITS).unwrap();
        assert_eq!(menu.value, SavedValue::MenuIndex(2));
        assert_eq!(
            &wire[menu.raw.offset()..menu.raw.offset() + menu.raw.data().len()],
            b"$menu 2"
        );
        let bytes = entries.next().unwrap();
        assert_eq!(
            SavedEntry::from_bytes(bytes, Some(Widget::Menu), LIMITS)
                .unwrap_err()
                .offset,
            bytes.offset()
        );
    });
}

#[test]
fn legacy_script_password_string_and_saved_table_follow_the_v50_layout() {
    for password in [None, Some(b"legacy".as_slice()), Some(b"".as_slice())] {
        let mut body = vec![0, 0x50, 0];
        body.extend(fixture::sized(b"on init end on"));
        body.extend([0, 0, 0]);
        body.extend(password.map_or_else(|| u32::MAX.to_le_bytes().to_vec(), fixture::sized));
        body.extend(u32::MAX.to_le_bytes());
        body.extend(u32::MAX.to_le_bytes());
        body.extend(1u32.to_le_bytes());
        body.extend(fixture::sized(b"$menu 1"));
        let wire = fixture::chunk(6, &body);
        support::without_heap(|| {
            let chunks = Chunks::parse(&wire, LIMITS).unwrap();
            let script = Script::parse(chunks.iter().next().unwrap(), LIMITS).unwrap();
            assert_eq!(script.password_hash.data(), password.unwrap_or(&[]));
            let raw = script.persistent.unwrap().iter().next().unwrap();
            assert_eq!(
                SavedEntry::from_bytes(raw, Some(Widget::Menu), LIMITS)
                    .unwrap()
                    .value,
                SavedValue::MenuIndex(1)
            );
        });
    }
}
