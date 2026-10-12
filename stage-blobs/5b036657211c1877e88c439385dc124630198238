use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=shim/vst3_shim.cpp");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let crt_static = std::env::var("CARGO_CFG_TARGET_FEATURE")
        .unwrap_or_default()
        .split(',')
        .any(|f| f == "crt-static");

    let mut build = cc::Build::new();
    build.cpp(true).file("shim/vst3_shim.cpp");

    // windows-gnu ignores `crt-static` (it links libgcc and winpthread
    // statically already); only MSVC needs the flag.
    if target_env == "msvc" && !crt_static {
        println!(
            "cargo:warning=moose-vst3: building for Windows MSVC without `+crt-static`; the \
             VST3 DLL will import VCRUNTIME140, which some hosts cannot find. \
             `cargo moose` adds it; for plain cargo builds add \
             `rustflags = [\"-C\", \"target-feature=+crt-static\"]` under \
             `[target.<triple>-windows-msvc]` in .cargo/config.toml."
        );
    }

    let mingw = target_os == "windows" && target_env == "gnu";
    if build.get_compiler().is_like_msvc() {
        build.flag("/std:c++17");
        // The shim's `strncpy` calls are bounded by the destination size;
        // silence the MSVC CRT's "use strncpy_s" deprecation noise.
        build.define("_CRT_SECURE_NO_WARNINGS", None);
        // /MT when the Rust side links the CRT statically, /MD
        // otherwise. A /MD shim inside a `+crt-static` cdylib imports
        // VCRUNTIME140 and can abort the host on process attach; a /MT
        // shim inside a /MD cdylib is a CRT mismatch. Always match.
        build.static_crt(crt_static);
    } else {
        build.flag("-std=c++17");
        // `cfg!` in a build script reads the *host*, so cross-compiling
        // from macOS handed this macOS-only flag to e.g. the mingw g++.
        // Gate on the target via `CARGO_CFG_TARGET_OS` instead.
        if target_os == "macos" {
            // Match the workspace's Apple deployment floor. 10.13 was
            // honored by Xcode <= 14 but newer Xcode SDKs reject it,
            // breaking with `cstdint not found` since no matching
            // headers ship.
            build.flag("-mmacosx-version-min=11.0");
        }
        if mingw {
            // Link libstdc++ ourselves (below) instead of cc's `-lstdc++`,
            // which resolves to libstdc++-6.dll.
            build.cpp_link_stdlib(None);
        }
    }

    build.compile("vst3_shim");

    // The shim's Windows idle timer calls SetTimer/KillTimer. Link user32
    // ourselves rather than relying on some other dep in the graph to.
    if target_os == "windows" {
        println!("cargo:rustc-link-lib=user32");
    }

    // MinGW: link libstdc++ statically, after the shim so a
    // single-pass linker resolves the shim's references. Hosts that
    // LoadLibrary a VST3 without LOAD_WITH_ALTERED_SEARCH_PATH (FL
    // Studio's scanner) never look beside the plugin for
    // libstdc++-6.dll, so the module fails to load. rustc already
    // links libgcc and winpthread statically on windows-gnu.
    if mingw {
        if let Some(dir) = static_libstdcxx_dir(&build) {
            println!("cargo:rustc-link-search=native={}", dir.display());
            println!("cargo:rustc-link-lib=static=stdc++");
        } else {
            println!(
                "cargo:warning=moose-vst3: no libstdc++.a next to the MinGW compiler; \
                 linking libstdc++ dynamically"
            );
            println!("cargo:rustc-link-lib=stdc++");
        }
    }
}

/// Directory of the MinGW compiler's `libstdc++.a`, if it has one.
/// `-print-file-name` echoes the bare name back when the file is
/// missing (e.g. an llvm-mingw toolchain), hence the absolute check.
fn static_libstdcxx_dir(build: &cc::Build) -> Option<PathBuf> {
    let out = build
        .get_compiler()
        .to_command()
        .arg("-print-file-name=libstdc++.a")
        .output()
        .ok()?;
    let archive = PathBuf::from(String::from_utf8(out.stdout).ok()?.trim());
    if !archive.is_absolute() || !archive.is_file() {
        return None;
    }
    archive.parent().map(PathBuf::from)
}
