//! Compiles `shim/clock.c` (the LD_PRELOAD virtual-clock shim) into a Linux
//! x86-64 ELF shared object.
//!
//! The shim must be a Linux ELF to load into a Docker subject, and it must be
//! built against an old glibc baseline so it loads into any glibc-based
//! subject (forward compatibility). The Zig compiler can target a specific
//! glibc version with bundled headers regardless of the build host, so the
//! shim is always cross-compiled with
//! `zig cc -target x86_64-linux-gnu.2.17`. A native compiler cannot do this:
//! it would build against the host's (modern) glibc and only load into
//! subjects running that glibc or newer, which is why the shim must go through
//! Zig on every platform, including Linux CI.
//!
//! The produced `dstest_clock.so` is embedded into the binary via
//! `include_bytes!(concat!(env!("OUT_DIR"), "/dstest_clock.so"))` and written
//! to the assets dir at runtime. Compiling here means CI verifies the C every
//! build and a broken shim fails the build, not the first virtual-clock run.

use std::path::Path;
use std::process::Command;

/// glibc baseline the shim targets. `README.md` documents that subjects must
/// run glibc >= this version for the virtual clock to preload.
const GLIBC_BASELINE: &str = "2.17";

fn main() {
    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR for build scripts");
    let out_so = Path::new(&out_dir).join("dstest_clock.so");

    // Rebuild the shim whenever the C source changes.
    println!("cargo:rerun-if-changed=shim/clock.c");

    let target = format!("x86_64-linux-gnu.{GLIBC_BASELINE}");
    let out = Command::new("zig")
        .args(["cc", "-shared", "-fPIC", "-Werror"])
        .args(["-target", &target])
        .arg("-o")
        .arg(&out_so)
        .arg("shim/clock.c")
        .output();

    match out {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            panic!(
                "zig cc failed to compile the clock shim (target {target})\n{}",
                stderr.trim()
            );
        }
        Err(e) => panic!(
            "zig is required to build the clock shim but was not found: {e}\n\
             Install it, for example: `nix profile install nixpkgs#zig`"
        ),
    }
}
