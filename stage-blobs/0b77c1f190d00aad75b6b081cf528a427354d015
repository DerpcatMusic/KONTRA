use sampler_core::ModScaleLaw;
mod support;

// Interpret the captured expression's operators, not a second handwritten law.
// fcinv_* receives an already conditionally inverted signal: its native behavior
// is outside this test. frange_uni selects the proved source-domain argument.
struct Expression<'a> {
    text: &'a str,
    base: f64,
    source: f64,
    depth: f64,
}
impl Expression<'_> {
    fn take(&mut self, token: &str) -> bool {
        self.text = self.text.trim_start();
        if let Some(rest) = self.text.strip_prefix(token) {
            self.text = rest;
            true
        } else {
            false
        }
    }
    fn sum(&mut self) -> f64 {
        let mut v = self.product();
        while self.take("-") {
            v -= self.product();
        }
        v
    }
    fn product(&mut self) -> f64 {
        let mut v = self.atom();
        while self.take("*") {
            v *= self.atom();
        }
        v
    }
    fn atom(&mut self) -> f64 {
        if self.take("(") {
            let v = self.sum();
            assert!(self.take(")"));
            v
        } else if self.take("frange_uni(") {
            let v = self.sum();
            assert!(self.take(")"));
            (v + 1.) * 0.5
        } else if self.take("fcinv_flip(") || self.take("fcinv_neg(") {
            let v = self.sum();
            assert!(self.take(")"));
            v
        } else if self.take("$intensity") {
            self.depth
        } else if self.take("$in") {
            self.base
        } else if self.take("$signal") {
            self.source
        } else {
            assert!(self.take("1.0"), "unknown expression: {}", self.text);
            1.
        }
    }
}

#[test]
fn id25_matches_all_512_original_emitter_cases() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("support/id25-emitter.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 512);
    let mut seen = [[false; 2]; 256];
    let mut checked = 0;
    support::without_heap(|| {
        for case in cases {
            let flags = case[0].as_u64().unwrap() as u8;
            let bipolar = case[1].as_bool().unwrap();
            assert!(!seen[flags as usize][bipolar as usize]);
            seen[flags as usize][bipolar as usize] = true;
            let expression = fixture["expressions"][case[2].as_u64().unwrap() as usize]
                .as_str()
                .unwrap();
            let law = ModScaleLaw::KontaktIntensity {
                depth: 0.,
                flags,
                unit: 1.,
            };
            for base in [-0.5, -0., 0.125, 0.5, 1., 1.25] {
                for unit in [0., 0.125, 0.5, 0.875, 1.] {
                    let source = if bipolar { unit * 2. - 1. } else { unit };
                    for depth in [-0.25, 0., 0.125, 0.5, 1., 1.25] {
                        let law = match law {
                            ModScaleLaw::KontaktIntensity { flags, .. } => {
                                ModScaleLaw::KontaktIntensity {
                                    depth,
                                    flags,
                                    unit: 1.,
                                }
                            }
                            _ => unreachable!(),
                        };
                        let mut oracle = Expression {
                            text: expression,
                            base,
                            source,
                            depth,
                        };
                        let expected = oracle.sum();
                        assert!(oracle.text.trim().is_empty());
                        assert_eq!(
                            law.apply(base, source, bipolar),
                            expected,
                            "flags={flags} bipolar={bipolar} base={base} src={source} depth={depth}"
                        );
                        checked += 1;
                    }
                }
            }
        }
    });
    assert!(seen.iter().all(|domains| *domains == [true, true]));
    assert_eq!(checked, 92_160);
}
