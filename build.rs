//! Cross-compiles `shim/clock.c` (the LD_PRELOAD virtual-clock shim) into a
//! Linux x86-64 ELF shared object using the Zig compiler.
//!
//! Zig bundles compiler-rt and glibc headers, so no container or target
//! toolchain is required regardless of the build host. The produced
//! `dstest_clock.so` is embedded into the binary via
//! `include_bytes!(concat!(env!("OUT_DIR"), "/dstest_clock.so"))` and written
//! to the assets dir at runtime. Compiling here means CI verifies the C every
//! build and a broken shim fails the build, not the first virtual-clock run.

use std::path::Path;
use std::process::Command;

fn main() {
    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR for build scripts");
    let out_so = Path::new(&out_dir).join("dstest_clock.so");

    // Rebuild the shim whenever the C source changes.
    println!("cargo:rerun-if-changed=shim/clock.c");

    // The virtual clock targets Linux x86-64 Docker containers regardless of
    // the build host (e.g. macOS + podman-machine).
    let target = "x86_64-linux-gnu";

    let status = Command::new("zig")
        .args(["cc", "-shared", "-fPIC", "-Werror"])
        .args(["-target", target])
        .arg("-o")
        .arg(&out_so)
        .arg("shim/clock.c")
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!(
            "zig failed to compile the clock shim (target {target}): exit status {}",
            s.code().unwrap_or(-1)
        ),
        Err(e) => panic!(
            "zig is required to build the clock shim but was not found: {e}\n\
             Install it, for example: `nix profile install nixpkgs#zig`"
        ),
    }
}
