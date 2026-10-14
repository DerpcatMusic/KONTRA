//! Load one instrument through the full production path (translate, decode only
//! the zones on one key, lower into a plan) and report whether it prepares.
//!
//!     cargo run -p sampler-kontakt --example prepare_check -- A.nki [KEY]

fn main() {
    let mut args = std::env::args_os().skip(1);
    let path = std::path::PathBuf::from(args.next().expect("usage: prepare_check A.nki [KEY]"));
    let key: u8 = args.next().and_then(|k| k.to_str()?.parse().ok()).unwrap_or(60);
    let options = sampler_kontakt::Options { rate: 48_000, keys: key..=key, ..Default::default() };
    let started = std::time::Instant::now();
    match sampler_kontakt::load(&path, &options, |_| {}) {
        Ok(loaded) => {
            let unsupported = loaded.instrument.unsupported.len();
            println!(
                "ok {:.1}s zones {} routes {} unsupported {}",
                started.elapsed().as_secs_f64(),
                loaded.instrument.zones.len(),
                loaded.instrument.routes.len(),
                unsupported
            );
            let mut by_feature = std::collections::BTreeMap::<&str, usize>::new();
            for u in &loaded.instrument.unsupported {
                *by_feature.entry(&u.feature).or_default() += 1;
            }
            let mut top: Vec<_> = by_feature.into_iter().collect();
            top.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
            for u in loaded.instrument.unsupported.iter().filter(|u| u.feature == "script") {
                println!("  script: {}: {}", u.location, u.value);
            }
            for (feature, n) in top.iter().take(12) {
                println!("  {n:6} {feature}");
            }
        }
        Err(e) => {
            println!("ERROR {e}");
            std::process::exit(1);
        }
    }
}
