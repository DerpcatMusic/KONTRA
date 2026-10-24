//! Builds the Signalsmith C++ shim for the `candidates` feature only. The shipping build runs no
//! compiler here and links nothing extra.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo::rerun-if-changed=candidates/signalsmith-dsp");
    println!("cargo::rerun-if-changed=candidates/signalsmith-stretch");
    if std::env::var_os("CARGO_FEATURE_CANDIDATES").is_none() {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let obj = out.join("signalsmith_shim.o");
    let lib = out.join("libsignalsmith_shim.a");
    let inc = PathBuf::from("candidates/signalsmith-dsp");
    let cxx = std::env::var("CXX").unwrap_or_else(|_| "c++".into());
    let stretch_inc = PathBuf::from("candidates/signalsmith-stretch");
    let stretch_obj = out.join("signalsmith_stretch_shim.o");
    for (src, dst, dir) in [(inc.join("shim.cpp"), obj.clone(), inc.clone()), (stretch_inc.join("stretch_shim.cpp"), stretch_obj.clone(), stretch_inc.clone())] {
        let status = Command::new(&cxx)
            .args(["-O2", "-std=c++17", "-fPIC", "-c"])
            .arg(&src)
            .arg("-I")
            .arg(&dir)
            .arg("-o")
            .arg(&dst)
            .status()
            .expect("c++ for the Signalsmith shim");
        assert!(status.success(), "building the Signalsmith shim failed");
    }
    let status = Command::new("ar").arg("crs").arg(&lib).arg(&obj).arg(&stretch_obj).status().expect("ar");
    assert!(status.success(), "archiving the Signalsmith shim failed");
    println!("cargo::rustc-link-search=native={}", out.display());
    println!("cargo::rustc-link-lib=static=signalsmith_shim");
    println!("cargo::rustc-link-lib=stdc++");
}
