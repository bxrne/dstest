//! Compiles `shim/clock.c` (the LD_PRELOAD virtual-clock shim) into a Linux
//! x86-64 ELF shared object.
//!
//! The shim must be a Linux ELF to load into a Docker subject, regardless of
//! the build host. Compiler selection:
//!
//! - On a Linux x86-64 host the native `cc` matches the target exactly, so it
//!   is used directly (no extra toolchain; this is the CI path).
//! - On any other host (macOS, aarch64 Linux, Windows) the Zig compiler
//!   cross-compiles to `x86_64-linux-gnu`. Zig bundles compiler-rt and glibc
//!   headers, so no container or target toolchain is required.
//!
//! The produced `dstest_clock.so` is embedded into the binary via
//! `include_bytes!(concat!(env!("OUT_DIR"), "/dstest_clock.so"))` and written
//! to the assets dir at runtime. Compiling here means CI verifies the C every
//! build and a broken shim fails the build, not the first virtual-clock run.

use std::path::Path;
use std::process::{Command, Output};

/// Run a command, capturing output. Returns the process `Output` on success
/// or an error string on spawn/exit failure.
fn run(cmd: &mut Command) -> Result<Output, String> {
    let prog = cmd.get_program().to_string_lossy().into_owned();
    let out = cmd
        .output()
        .map_err(|e| format!("{prog} could not be started: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "{prog} failed with exit status {}\n{}",
            out.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }
    Ok(out)
}

fn main() {
    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR for build scripts");
    let out_so = Path::new(&out_dir).join("dstest_clock.so");

    // Rebuild the shim whenever the C source changes.
    println!("cargo:rerun-if-changed=shim/clock.c");

    let host = std::env::var("TARGET").unwrap_or_default();

    let result = if host.starts_with("x86_64-unknown-linux") || host.starts_with("x86_64-linux") {
        // Linux x86-64: the native compiler produces an ELF that matches the
        // target; CI relies on this and needs no extra toolchain.
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
        run(Command::new(&cc)
            .args(["-shared", "-fPIC", "-Werror"])
            .arg("-o")
            .arg(&out_so)
            .arg("shim/clock.c"))
    } else {
        // Any other host: cross-compile to Linux x86-64 with Zig.
        let target = "x86_64-linux-gnu";
        match Command::new("zig")
            .args(["cc", "-shared", "-fPIC", "-Werror"])
            .args(["-target", target])
            .arg("-o")
            .arg(&out_so)
            .arg("shim/clock.c")
            .output()
        {
            Ok(out) if out.status.success() => Ok(out),
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                Err(format!(
                    "zig cc failed to compile the clock shim (target {target})\n{}",
                    stderr.trim()
                ))
            }
            Err(e) => Err(format!(
                "zig is required to cross-compile the clock shim but was not found: {e}"
            )),
        }
    };

    if let Err(e) = result {
        panic!("{e}");
    }
}
