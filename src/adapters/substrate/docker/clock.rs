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
//! The shim is compiled once on CentOS 7 (glibc 2.17) so the resulting `.so`
//! loads into any glibc-based subject (forward-compatible). Monotonic clocks
//! are NOT faked — subject timeouts and busy-waits use real elapsed time,
//! which is the correct DST semantics (the virtual clock only moves when
//! dstest says so).
//!
//! Limitations:
//! - Only dynamically linked glibc binaries are affected (musl/static
//!   binaries — e.g. Alpine images, Go — ignore `LD_PRELOAD`).
//! - The manual clock is frozen by construction; `set_rate`/`release` are
//!   unsupported.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bollard::Docker as BollardDocker;
use bollard::models::{ContainerCreateBody, HostConfig};
use bollard::query_parameters::{
    CreateContainerOptions, RemoveContainerOptions, StartContainerOptions, WaitContainerOptions,
};
use futures_util::TryStreamExt;
use tracing::{debug, info, warn};

use crate::domain::subject::Subject;
use crate::ports::components::{ClockControl, ClockState};

/// The clock shim C source. Intercepts CLOCK_REALTIME only; reads nanoseconds
/// from a binary control file on each call.
const CLOCK_SHIM_C: &str = r#"
#define _GNU_SOURCE
#include <time.h>
#include <sys/time.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <unistd.h>
#include <stdlib.h>

static int (*real_clock_gettime)(clockid_t, struct timespec *);
static time_t (*real_time)(time_t *);
static const char *ctl_path;

__attribute__((constructor))
static void dstest_clock_init(void) {
    real_clock_gettime = dlsym(RTLD_NEXT, "clock_gettime");
    real_time = dlsym(RTLD_NEXT, "time");
    ctl_path = getenv("DSTEST_CLOCK_CTL");
}

static int read_clock(struct timespec *tp) {
    if (!ctl_path) return -1;
    int fd = open(ctl_path, O_RDONLY);
    if (fd < 0) return -1;
    long long nanos = -1;
    if (read(fd, &nanos, 8) != 8) { close(fd); return -1; }
    close(fd);
    if (nanos < 0) return -1;
    tp->tv_sec = (time_t)(nanos / 1000000000LL);
    tp->tv_nsec = (long)(nanos % 1000000000LL);
    return 0;
}

int clock_gettime(clockid_t clk, struct timespec *tp) {
    if (!real_clock_gettime) dstest_clock_init();
    if (clk == CLOCK_REALTIME
#ifdef CLOCK_REALTIME_COARSE
        || clk == CLOCK_REALTIME_COARSE
#endif
    ) {
        if (read_clock(tp) == 0) return 0;
    }
    return real_clock_gettime(clk, tp);
}

time_t time(time_t *t) {
    if (!real_time) dstest_clock_init();
    struct timespec tp;
    if (clock_gettime(CLOCK_REALTIME, &tp) == 0) {
        if (t) *t = tp.tv_sec;
        return tp.tv_sec;
    }
    return real_time(t);
}
"#;

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
    connection: BollardDocker,
    /// container id -> control file path on the host
    clocks: Mutex<HashMap<String, PathBuf>>,
}

impl DockerClock {
    pub fn new(connection: BollardDocker) -> Self {
        Self {
            connection,
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
        let dir = self.ensure_assets().await?;

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

    /// Ensure the clock shim `.so` exists in the assets dir, compiling it
    /// via a throwaway CentOS 7 container on first use. The C source is
    /// written to the assets dir on the host; the container mounts it,
    /// compiles, and writes the `.so` back via the bind mount.
    async fn ensure_assets(&self) -> Result<PathBuf, String> {
        let dir = Self::assets_dir();
        let so_path = dir.join("dstest_clock.so");
        if so_path.exists() {
            return Ok(dir);
        }

        fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create clock assets dir: {}", e))?;

        // Write the C source for the container to compile.
        let c_path = dir.join("clock.c");
        fs::write(&c_path, CLOCK_SHIM_C).map_err(|e| format!("failed to write clock.c: {}", e))?;

        info!("virtual clock: compiling clock shim (one-time setup)");
        let conn = &self.connection;

        // CentOS 7 (glibc 2.17): the .so loads into any newer-glibc subject.
        super::Docker::pull_image(conn, "centos:7").await?;

        let config = ContainerCreateBody {
            image: Some("centos:7".to_string()),
            cmd: Some(vec![
                "sh".to_string(),
                "-c".to_string(),
                "sed -i 's|mirrorlist=|#mirrorlist=|g; s|#baseurl=http://mirror.centos.org|baseurl=http://vault.centos.org|g' /etc/yum.repos.d/CentOS-*.repo \
                 && yum install -y -q gcc \
                 && cc -shared -fPIC -o /dstest-assets/dstest_clock.so /dstest-assets/clock.c -ldl"
                    .to_string(),
            ]),
            host_config: Some(HostConfig {
                binds: Some(vec![format!("{}:/dstest-assets", dir.display())]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let container = conn
            .create_container(None::<CreateContainerOptions>, config)
            .await
            .map_err(|e| format!("failed to create clock build container: {}", e))?;
        let build_id = container.id.clone();

        let result = async {
            conn.start_container(&build_id, None::<StartContainerOptions>)
                .await
                .map_err(|e| format!("failed to start clock build container: {}", e))?;

            let waits = conn
                .wait_container(&build_id, None::<WaitContainerOptions>)
                .try_collect::<Vec<_>>()
                .await
                .map_err(|e| format!("clock build container wait failed: {}", e))?;
            let exit = waits.first().map(|w| w.status_code).unwrap_or(-1);
            if exit != 0 {
                return Err(format!(
                    "clock build container exited with {} (network access required for yum)",
                    exit
                ));
            }

            if !so_path.exists() {
                return Err("clock shim was not produced (cc failed silently)".to_string());
            }

            info!("virtual clock: shim ready at {}", dir.display());
            Ok(())
        }
        .await;

        // Always clean up the build container.
        let options = RemoveContainerOptions {
            v: true,
            force: true,
            link: false,
        };
        if let Err(e) = conn.remove_container(&build_id, Some(options)).await {
            warn!("failed to remove clock build container: {}", e);
        }

        result?;
        Ok(dir)
    }
}

impl ClockControl for DockerClock {
    async fn now(&self, subject: &Subject) -> Result<i64, String> {
        Ok(self.read_nanos(subject)? / 1_000_000)
    }

    async fn set_offset(&self, subject: &Subject, offset_ms: i64) -> Result<(), String> {
        let real_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_nanos() as i64;
        self.write_nanos(subject, real_now + offset_ms * 1_000_000)
    }

    async fn advance(&self, subject: &Subject, delta_ms: i64) -> Result<(), String> {
        let current = self.read_nanos(subject)?;
        self.write_nanos(subject, current + delta_ms * 1_000_000)
    }

    async fn set_rate(&self, _subject: &Subject, _rate: f64) -> Result<(), String> {
        Err(
            "set_rate is not supported by the manual clock: time only moves when advanced"
                .to_string(),
        )
    }

    async fn freeze(&self, _subject: &Subject) -> Result<(), String> {
        // The manual clock is frozen by construction.
        Ok(())
    }

    async fn release(&self, _subject: &Subject) -> Result<(), String> {
        Err("release is not supported: a virtual-clock subject must be recreated to regain real time".to_string())
    }

    async fn state(&self, subject: &Subject) -> Result<ClockState, String> {
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
