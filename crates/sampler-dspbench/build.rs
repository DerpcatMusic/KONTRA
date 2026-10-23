//! Builds the Signalsmith C++ shim for the `candidates` feature only. The shipping build runs no
//! compiler here and links nothing extra.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo::rerun-if-changed=candidates/signalsmith-dsp");
    if std::env::var_os("CARGO_FEATURE_CANDIDATES").is_none() {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let obj = out.join("signalsmith_shim.o");
    let lib = out.join("libsignalsmith_shim.a");
    let inc = PathBuf::from("candidates/signalsmith-dsp");
    let cxx = std::env::var("CXX").unwrap_or_else(|_| "c++".into());
    let status = Command::new(&cxx)
        .args(["-O2", "-std=c++17", "-fPIC", "-c"])
        .arg(inc.join("shim.cpp"))
        .arg("-I")
        .arg(&inc)
        .arg("-o")
        .arg(&obj)
        .status()
        .expect("c++ for the Signalsmith shim");
    assert!(status.success(), "building the Signalsmith shim failed");
    let status = Command::new("ar").arg("crs").arg(&lib).arg(&obj).status().expect("ar");
    assert!(status.success(), "archiving the Signalsmith shim failed");
    println!("cargo::rustc-link-search=native={}", out.display());
    println!("cargo::rustc-link-lib=static=signalsmith_shim");
    println!("cargo::rustc-link-lib=stdc++");
}
