//! Build script: compiles the vendored cubiomes C sources (MIT) needed for biome
//! queries, plus a thin `bridge.c` that gives Rust a stable, layout-free API.
//!
//! We deliberately compile only the minimal set required by `getBiomeAt`
//! (`noise`, `biomes`, `layers`, `biomenoise`, `generator`, `util`) and **not**
//! `finders.c`/`quadbase.c`, which pull in `pthread.h`/`windows.h` portability
//! code that does not build cleanly with MSVC and is not needed here.
//!
//! MSVC is discovered automatically by the `cc` crate via vswhere; `cl.exe` need
//! not be on PATH.

use std::path::PathBuf;

fn main() {
    let cubiomes = PathBuf::from("cubiomes");

    // Emit rebuild triggers for the vendored tree.
    println!("cargo:rerun-if-changed={}", cubiomes.display());
    println!("cargo:rerun-if-changed=src/bridge.c");

    let sources = [
        "noise.c",
        "biomes.c",
        "layers.c",
        "biomenoise.c",
        "generator.c",
        "util.c",
    ];

    let mut build = cc::Build::new();
    for src in &sources {
        build.file(cubiomes.join(src));
    }
    build.file("src/bridge.c");
    build.include(&cubiomes);
    build.opt_level(2);
    build.warnings(false);

    build.compile("cubiomes");
}
