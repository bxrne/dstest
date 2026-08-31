//! Virtual clocks for Docker subjects via a tiny LD_PRELOAD shim.
//!
//! A subject that opts in via `setup({ ..., clock = { virtual = true } })`
//! runs with `LD_PRELOAD=dstest_clock.so` and `DSTEST_CLOCK_CTL=<path>`.
//! The shim intercepts `clock_gettime(CLOCK_REALTIME)`, `time()`, and
//! `gettimeofday()`, reading the current time as nanoseconds-since-epoch from
//! an 8-byte little-endian binary control file on every call — a fully
//! harness-controlled manual clock: the subject's time is frozen until dstest
//! advances it by writing a new value.
//!
//! The shim source lives at `shim/clock.c` and is cross-compiled to a Linux
//! x86-64 ELF by [`build.rs`](build.rs) using the Zig compiler (bundled glibc
//! headers), so no container or target toolchain is required at build time and
//! CI verifies the C compiles on every build. The produced `.so` is embedded
//! into the binary and written to the assets dir on first use. Monotonic
//! clocks are NOT faked — subject timeouts and busy-waits use real elapsed
//! time, which is the correct DST semantics (the virtual clock only moves when
//! dstest says so).
//!
//! Limitations:
//! - Only dynamically linked glibc binaries are affected (musl/static
//!   binaries — e.g. Alpine images, Go — ignore `LD_PRELOAD`).
//! - The manual clock is frozen by construction; `set_rate`/`release` are
//!   unsupported.

use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::io::Read;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::debug;

use crate::domain::subject::Subject;
use crate::ports::components::{ClockControl, ClockState};

/// The compiled clock shim, cross-compiled to a Linux x86-64 ELF by
/// `build.rs` and embedded into the binary. Written to the assets dir on first
/// use; a subject's container preloads it via `LD_PRELOAD`.
const CLOCK_SHIM_SO: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/dstest_clock.so"));

/// Parsed from the `clock` table of `dstest.setup`.
#[derive(Clone, Debug)]
pub struct ClockSpec {
    /// Fixed epoch the clock starts at (unix seconds). Defaults to the real
    /// current time.
    pub start_epoch_secs: Option<u64>,
}

/// Everything [`super::Docker::host`] needs to wire a virtual clock into a
/// container at creation time.
pub struct ClockPrep {
    pub env: Vec<String>,
    pub bind: String,
    pub ctl_path: PathBuf,
}

pub struct DockerClock {
    /// container id -> control file path on the host
    clocks: Mutex<HashMap<String, PathBuf>>,
}

impl DockerClock {
    pub fn new() -> Self {
        Self {
            clocks: Mutex::new(HashMap::new()),
        }
    }

    /// Host directory holding the compiled shim and per-subject control files.
    fn assets_dir() -> PathBuf {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(std::env::temp_dir);
        base.join("dstest").join("clock")
    }

    /// Prepare env + bind mount for a subject that opted into a virtual
    /// clock. Called before container creation; the returned `ctl_path` is
    /// registered once the container id is known.
    pub async fn prepare(&self, subject_name: &str, spec: &ClockSpec) -> Result<ClockPrep, String> {
        let dir = self.ensure_assets()?;

        let start_nanos = spec
            .start_epoch_secs
            .map(|s| (s as i64) * 1_000_000_000)
            .unwrap_or_else(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_nanos() as i64
            });

        let ctl_path = dir.join(format!("{}.ctl", subject_name));
        write_clock_nanos(&ctl_path, start_nanos)?;

        Ok(ClockPrep {
            env: vec![
                "LD_PRELOAD=/dstest-clock/dstest_clock.so".to_string(),
                format!("DSTEST_CLOCK_CTL=/dstest-clock/{}.ctl", subject_name),
            ],
            bind: format!("{}:/dstest-clock:ro", dir.display()),
            ctl_path,
        })
    }

    /// Associate a running container with its control file.
    pub fn register(&self, container_id: String, ctl_path: PathBuf) {
        self.clocks
            .lock()
            .expect("poisoned clock lock")
            .insert(container_id, ctl_path);
    }

    /// Drop bookkeeping and delete the control file.
    pub fn unregister(&self, container_id: &str) {
        let path = self
            .clocks
            .lock()
            .expect("poisoned clock lock")
            .remove(container_id);
        if let Some(path) = path
            && let Err(e) = fs::remove_file(&path)
        {
            debug!("failed to remove clock control file {:?}: {}", path, e);
        }
    }

    fn ctl_for(&self, subject: &Subject) -> Result<PathBuf, String> {
        let id = subject.id.strip_prefix("docker/").unwrap_or(&subject.id);
        self.clocks
            .lock()
            .expect("poisoned clock lock")
            .get(id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "subject {} has no virtual clock (set clock = {{ virtual = true }} in setup)",
                    subject.id
                )
            })
    }

    fn read_nanos(&self, subject: &Subject) -> Result<i64, String> {
        let ctl = self.ctl_for(subject)?;
        let mut buf = [0u8; 8];
        let mut file = fs::File::open(&ctl)
            .map_err(|e| format!("failed to open clock control file: {}", e))?;
        file.read_exact(&mut buf)
            .map_err(|e| format!("failed to read clock control file: {}", e))?;
        Ok(i64::from_le_bytes(buf))
    }

    fn write_nanos(&self, subject: &Subject, nanos: i64) -> Result<(), String> {
        let ctl = self.ctl_for(subject)?;
        fs::write(&ctl, nanos.to_le_bytes())
            .map_err(|e| format!("failed to write clock control file: {}", e))?;
        debug!("virtual clock for {} set to {} nanos", subject.id, nanos);
        Ok(())
    }

    /// Ensure the embedded clock shim `.so` exists in the assets dir, writing
    /// it out from the bytes compiled by `build.rs` on first use. No container
    /// or runtime toolchain is involved; the shim ships inside the binary.
    fn ensure_assets(&self) -> Result<PathBuf, String> {
        let dir = Self::assets_dir();
        let so_path = dir.join("dstest_clock.so");
        if so_path.exists() {
            return Ok(dir);
        }

        fs::create_dir_all(&dir).map_err(|e| format!("failed to create clock assets dir: {}", e))?;
        let tmp = dir.join("dstest_clock.so.tmp");
        fs::write(&tmp, CLOCK_SHIM_SO).map_err(|e| format!("failed to write clock shim: {}", e))?;
        fs::rename(&tmp, &so_path).map_err(|e| format!("failed to finalize clock shim: {}", e))?;
        Ok(dir)
    }
}

impl ClockControl for DockerClock {
    fn now<'a>(
        &'a self,
        subject: &'a Subject,
    ) -> Pin<Box<dyn Future<Output = Result<i64, String>> + Send + 'a>> {
        Box::pin(async move { Ok(self.read_nanos(subject)? / 1_000_000) })
    }

    fn set_offset<'a>(
        &'a self,
        subject: &'a Subject,
        offset_ms: i64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let real_now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_nanos() as i64;
            self.write_nanos(subject, real_now + offset_ms * 1_000_000)
        })
    }

    fn advance<'a>(
        &'a self,
        subject: &'a Subject,
        delta_ms: i64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let current = self.read_nanos(subject)?;
            self.write_nanos(subject, current + delta_ms * 1_000_000)
        })
    }

    fn set_rate<'a>(
        &'a self,
        _subject: &'a Subject,
        _rate: f64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            Err(
                "set_rate is not supported by the manual clock: time only moves when advanced"
                    .to_string(),
            )
        })
    }

    fn freeze<'a>(
        &'a self,
        _subject: &'a Subject,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        // The manual clock is frozen by construction.
        Box::pin(async move { Ok(()) })
    }

    fn release<'a>(
        &'a self,
        _subject: &'a Subject,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            Err(
                "release is not supported: a virtual-clock subject must be recreated to regain real time"
                    .to_string(),
            )
        })
    }

    fn state<'a>(
        &'a self,
        subject: &'a Subject,
    ) -> Pin<Box<dyn Future<Output = Result<ClockState, String>> + Send + 'a>> {
        Box::pin(async move {
            let nanos = self.read_nanos(subject)?;
            let real_now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_nanos() as i64;
            Ok(ClockState {
                virtualised: true,
                epoch_millis: nanos / 1_000_000,
                offset_millis: (nanos - real_now) / 1_000_000,
                rate: 1.0,
                frozen: true,
            })
        })
    }
}

/// Write nanoseconds-since-epoch as 8-byte little-endian to the control file.
fn write_clock_nanos(path: &PathBuf, nanos: i64) -> Result<(), String> {
    fs::write(path, nanos.to_le_bytes())
        .map_err(|e| format!("failed to write clock control file: {}", e))?;
    debug!("clock control file {:?} set to {} nanos", path, nanos);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_read_nanos() {
        let dir = std::env::temp_dir().join("dstest_clock_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.ctl");
        write_clock_nanos(&path, 1_600_000_000_000_000_000).unwrap();
        let mut buf = [0u8; 8];
        let mut file = fs::File::open(&path).unwrap();
        file.read_exact(&mut buf).unwrap();
        assert_eq!(i64::from_le_bytes(buf), 1_600_000_000_000_000_000);
        fs::remove_file(&path).ok();
        fs::remove_dir(&dir).ok();
    }
}
