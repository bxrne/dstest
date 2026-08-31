//! Fault-injectable storage via device-mapper `flakey` target.
//!
//! A subject that opts in via
//! `setup({ ..., storage = { flaky = true, mount = "/data", size_mb = 512 } })`
//! gets a loop-backed ext4 filesystem on a dm-flakey device, bind-mounted into
//! the container. At runtime:
//!
//! - `error(true)` — reload the dm table with the `error` target → all I/O
//!   returns EIO.
//! - `drop_writes(true)` — reload with `flakey ... drop_writes` in the always-
//!   down state → writes succeed in userspace but data is discarded.
//! - `corrupt(n)` — flip `n` bytes at seeded-random offsets in the backing
//!   file (bit rot) while the dm device is suspended.
//! - `snapshot()` / `restore(id)` — copy the backing file for crash-consistency
//!   testing (snapshot → kill → restore → verify).
//! - `slow()` — not supported on dm-flakey (returns an error; use
//!   `deprive:disk` for coarse I/O throttling).
//!
//! Requires root + the `dm-flakey` kernel module. Host-side operations use
//! `std::process::Command` (`losetup`, `dmsetup`, `mkfs.ext4`, `mount`).

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Command;
use std::sync::Mutex;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tracing::{debug, info};

use crate::domain::subject::Subject;
use crate::ports::components::{StorageControl, StorageOpts};

/// Parsed from the `storage` table of `dstest.setup`.
#[derive(Clone, Debug)]
pub struct StorageSpec {
    pub size_mb: u64,
    pub mount: String,
}

impl From<StorageSpec> for StorageOpts {
    fn from(s: StorageSpec) -> Self {
        StorageOpts {
            size_mb: s.size_mb,
            mount: s.mount,
        }
    }
}

/// Host-side state for one subject's virtual disk.
#[derive(Clone)]
struct StorageState {
    backing_file: PathBuf,
    loop_dev: String,
    dm_name: String,
    host_mount: PathBuf,
    sectors: u64,
}

/// Result of [`DockerStorage::prepare`] — bind mount + dm name for registration.
pub struct StoragePrep {
    /// `host_path:container_path` bind string for Docker.
    pub bind: String,
    /// Device-mapper name; pass to [`DockerStorage::register`] after start.
    pub dm_name: String,
}

pub struct DockerStorage {
    storages: Mutex<HashMap<String, StorageState>>,
    /// Experiment seed for deterministic `corrupt`. `None` until config sets it.
    seed: Mutex<Option<u64>>,
    /// Monotonic counter mixed into each `corrupt` call so successive calls
    /// with the same seed still produce distinct bit-flips.
    corrupt_counter: Mutex<u64>,
}

impl DockerStorage {
    pub fn new() -> Self {
        Self {
            storages: Mutex::new(HashMap::new()),
            seed: Mutex::new(None),
            corrupt_counter: Mutex::new(0),
        }
    }

    fn assets_dir() -> PathBuf {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(std::env::temp_dir);
        base.join("dstest").join("storage")
    }

    fn dm_name_for(subject_name: &str) -> String {
        format!(
            "dstest-{}",
            subject_name
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
        )
    }

    /// Create the dm-flakey device and mount it on the host. Called before
    /// container creation; the returned bind mount is added to the container.
    ///
    /// On failure, any partially-created resources are cleaned up.
    pub fn prepare(&self, subject_name: &str, spec: &StorageSpec) -> Result<StoragePrep, String> {
        let opts = StorageOpts::from(spec.clone());
        opts.validate()?;

        let dir = Self::assets_dir();
        fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create storage assets dir: {}", e))?;

        let dm_name = Self::dm_name_for(subject_name);
        let backing_file = dir.join(format!("{}.img", dm_name));
        let host_mount = dir.join(format!("{}.mnt", dm_name));

        // Tear down any leftover state from a previous crashed run.
        self.force_cleanup(&dm_name, &backing_file, &host_mount);

        // Sparse backing file.
        {
            let file = File::create(&backing_file)
                .map_err(|e| format!("failed to create backing file: {}", e))?;
            file.set_len(spec.size_mb.saturating_mul(1024 * 1024))
                .map_err(|e| format!("failed to set backing file size: {}", e))?;
        }

        let result = (|| -> Result<(String, u64), String> {
            let loop_dev = run_cmd(
                "losetup",
                &["--find", "--show", &backing_file.display().to_string()],
            )
            .map_err(|e| format!("losetup failed (need root?): {}", e))?;
            let loop_dev = loop_dev.trim().to_string();

            let sectors_str = run_cmd("blockdev", &["--getsz", &loop_dev])
                .or_else(|_| run_cmd("blockdev", &["--getsize", &loop_dev]))
                .map_err(|e| format!("blockdev failed: {}", e))?;
            let sectors: u64 = sectors_str
                .trim()
                .parse()
                .map_err(|e| format!("invalid sector count: {}", e))?;

            // Always-up flakey table (no down intervals → pass-through).
            let table = Self::pass_through_table(sectors, &loop_dev);
            run_cmd("dmsetup", &["create", &dm_name, "--table", &table])
                .map_err(|e| format!("dmsetup create failed (need root + dm-flakey?): {}", e))?;

            let dm_path = format!("/dev/mapper/{}", dm_name);
            run_cmd("mkfs.ext4", &["-q", &dm_path])
                .map_err(|e| format!("mkfs.ext4 failed: {}", e))?;

            fs::create_dir_all(&host_mount)
                .map_err(|e| format!("failed to create host mount dir: {}", e))?;
            run_cmd("mount", &[&dm_path, &host_mount.display().to_string()])
                .map_err(|e| format!("mount failed: {}", e))?;

            // Container processes may run as non-root.
            let _ = run_cmd("chmod", &["777", &host_mount.display().to_string()]);

            Ok((loop_dev, sectors))
        })();

        let (loop_dev, sectors) = match result {
            Ok(v) => v,
            Err(e) => {
                self.force_cleanup(&dm_name, &backing_file, &host_mount);
                return Err(e);
            }
        };

        info!(
            "storage: prepared {} ({}MB) at {} -> {}",
            dm_name,
            spec.size_mb,
            host_mount.display(),
            spec.mount
        );

        self.storages.lock().expect("poisoned storage lock").insert(
            dm_name.clone(),
            StorageState {
                backing_file,
                loop_dev,
                dm_name: dm_name.clone(),
                host_mount: host_mount.clone(),
                sectors,
            },
        );

        Ok(StoragePrep {
            bind: format!("{}:{}", host_mount.display(), spec.mount),
            dm_name,
        })
    }

    /// Re-key storage state from `dm_name` to `container_id` after start.
    pub fn register(&self, container_id: String, dm_name: &str) {
        let mut storages = self.storages.lock().expect("poisoned storage lock");
        if let Some(state) = storages.remove(dm_name) {
            storages.insert(container_id, state);
        }
    }

    /// Discard a prepared-but-not-registered device (host() failed mid-way).
    pub fn discard(&self, dm_name: &str) {
        self.cleanup_key(dm_name);
    }

    /// Detach and clean up after container teardown.
    pub fn unregister(&self, container_id: &str) {
        self.cleanup_key(container_id);
    }

    fn cleanup_key(&self, key: &str) {
        let state = self
            .storages
            .lock()
            .expect("poisoned storage lock")
            .remove(key);
        if let Some(state) = state {
            self.teardown_state(&state);
            info!("storage: detached {}", state.dm_name);
        }
    }

    /// Best-effort cleanup of leftover devices/files from a previous run.
    fn force_cleanup(&self, dm_name: &str, backing_file: &PathBuf, host_mount: &PathBuf) {
        let _ = run_cmd("umount", &[&host_mount.display().to_string()]);
        let _ = run_cmd("dmsetup", &["remove", "--force", dm_name]);
        // Detach any loop device still pointing at this backing file.
        if backing_file.exists()
            && let Ok(out) = run_cmd("losetup", &["-j", &backing_file.display().to_string()])
        {
            for line in out.lines() {
                if let Some(dev) = line.split(':').next() {
                    let _ = run_cmd("losetup", &["-d", dev.trim()]);
                }
            }
        }
        let _ = fs::remove_file(backing_file);
        let _ = fs::remove_dir(host_mount);
    }

    fn teardown_state(&self, state: &StorageState) {
        let _ = run_cmd("umount", &[&state.host_mount.display().to_string()]);
        let _ = run_cmd("dmsetup", &["remove", "--force", &state.dm_name]);
        let _ = run_cmd("losetup", &["-d", &state.loop_dev]);
        // Snapshots live next to the backing file.
        if let Some(parent) = state.backing_file.parent()
            && let Ok(entries) = fs::read_dir(parent)
        {
            let prefix = format!("{}.", state.dm_name);
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(&prefix) && name.ends_with(".snap") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        let _ = fs::remove_file(&state.backing_file);
        let _ = fs::remove_dir(&state.host_mount);
    }

    fn state_for(&self, subject: &Subject) -> Result<StorageState, String> {
        let id = subject.id.strip_prefix("docker/").unwrap_or(&subject.id);
        self.storages
            .lock()
            .expect("poisoned storage lock")
            .get(id)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "subject {} has no virtual disk (set storage = {{ flaky = true, ... }} in setup)",
                    subject.id
                )
            })
    }

    fn reload_table(&self, state: &StorageState, table: &str) -> Result<(), String> {
        run_cmd("dmsetup", &["suspend", &state.dm_name])
            .map_err(|e| format!("dmsetup suspend failed: {}", e))?;
        let result = run_cmd("dmsetup", &["reload", &state.dm_name, "--table", table]);
        // Always resume, even if reload failed, so the device is not left hung.
        let _ = run_cmd("dmsetup", &["resume", &state.dm_name]);
        result.map_err(|e| format!("dmsetup reload failed: {}", e))?;
        Ok(())
    }

    /// Always-up flakey (pass-through, no impairments).
    fn pass_through_table(sectors: u64, loop_dev: &str) -> String {
        // up=large, down=0 → always up (kernel docs).
        format!("0 {} flakey {} 0 999999 0", sectors, loop_dev)
    }

    /// Always-down flakey with optional feature args (`drop_writes`, …).
    fn down_table(sectors: u64, loop_dev: &str, features: &[&str]) -> String {
        // up=0, down=1 → always down; then `<num_features> <feature…>`.
        if features.is_empty() {
            format!("0 {} flakey {} 0 0 1", sectors, loop_dev)
        } else {
            format!(
                "0 {} flakey {} 0 0 1 {} {}",
                sectors,
                loop_dev,
                features.len(),
                features.join(" ")
            )
        }
    }

    fn snap_path(state: &StorageState, snap_id: &str) -> PathBuf {
        state
            .backing_file
            .with_file_name(format!("{}.snap", snap_id))
    }
}

impl StorageControl for DockerStorage {
    fn set_seed(&self, seed: u64) {
        let mut guard = self.seed.lock().expect("poisoned seed lock");
        if guard.is_none() {
            *guard = Some(seed);
            *self.corrupt_counter.lock().expect("poisoned counter lock") = 0;
            debug!("storage seed set to {}", seed);
        }
    }

    fn attach<'a>(
        &'a self,
        _subject: &'a Subject,
        _opts: StorageOpts,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            Err(
                "storage must be configured at setup time via setup({ storage = { flaky = true, mount = \"/data\", size_mb = 512 } })"
                    .to_string(),
            )
        })
    }

    fn error<'a>(
        &'a self,
        subject: &'a Subject,
        on: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let state = self.state_for(subject)?;
            if on {
                // Constant EIO for all I/O.
                let table = format!("0 {} error", state.sectors);
                self.reload_table(&state, &table)?;
                debug!("storage: error mode ON for {}", state.dm_name);
            } else {
                let table = Self::pass_through_table(state.sectors, &state.loop_dev);
                self.reload_table(&state, &table)?;
                debug!("storage: error mode OFF for {}", state.dm_name);
            }
            Ok(())
        })
    }

    fn drop_writes<'a>(
        &'a self,
        subject: &'a Subject,
        on: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let state = self.state_for(subject)?;
            let table = if on {
                // Always-down + drop_writes: writes ACK but never hit the backing store.
                Self::down_table(state.sectors, &state.loop_dev, &["drop_writes"])
            } else {
                Self::pass_through_table(state.sectors, &state.loop_dev)
            };
            self.reload_table(&state, &table)?;
            debug!("storage: drop_writes={} for {}", on, state.dm_name);
            Ok(())
        })
    }

    fn slow<'a>(
        &'a self,
        _subject: &'a Subject,
        _delay_ms: u64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            Err(
                "slow is not supported on dm-flakey (use deprive:disk for coarse I/O throttling)"
                    .to_string(),
            )
        })
    }

    fn corrupt<'a>(
        &'a self,
        subject: &'a Subject,
        n: u64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let state = self.state_for(subject)?;
            let seed = self.seed.lock().expect("poisoned seed lock").unwrap_or(0);
            let counter = {
                let mut c = self.corrupt_counter.lock().expect("poisoned counter lock");
                let v = *c;
                *c = c.wrapping_add(1);
                v
            };
            // Mix subject id + counter so concurrent subjects and successive calls
            // draw independent streams from the same experiment seed.
            let mixed = seed
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add(counter)
                .wrapping_add(state.dm_name.len() as u64);
            let mut rng = StdRng::seed_from_u64(mixed);

            run_cmd("dmsetup", &["suspend", &state.dm_name])
                .map_err(|e| format!("dmsetup suspend failed: {}", e))?;

            let corrupt_result = (|| -> Result<(), String> {
                let file_size = fs::metadata(&state.backing_file)
                    .map_err(|e| format!("failed to stat backing file: {}", e))?
                    .len();
                if file_size == 0 {
                    return Err("backing file is empty".to_string());
                }
                let mut file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&state.backing_file)
                    .map_err(|e| format!("failed to open backing file: {}", e))?;
                for _ in 0..n {
                    let offset = rng.gen_range(0..file_size);
                    file.seek(SeekFrom::Start(offset))
                        .map_err(|e| format!("seek failed: {}", e))?;
                    let mut buf = [0u8; 1];
                    // Sparse holes read as zeros; still flip them so the file is dirtied.
                    let _ = file.read_exact(&mut buf);
                    file.seek(SeekFrom::Start(offset))
                        .map_err(|e| format!("seek failed: {}", e))?;
                    buf[0] ^= 0xff;
                    file.write_all(&buf)
                        .map_err(|e| format!("corrupt write failed: {}", e))?;
                }
                file.sync_all()
                    .map_err(|e| format!("fsync after corrupt failed: {}", e))?;
                Ok(())
            })();

            let _ = run_cmd("dmsetup", &["resume", &state.dm_name]);
            corrupt_result?;
            info!("storage: corrupted {} bytes in {}", n, state.dm_name);
            Ok(())
        })
    }

    fn snapshot<'a>(
        &'a self,
        subject: &'a Subject,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(async move {
            let state = self.state_for(subject)?;
            let counter = {
                let mut c = self.corrupt_counter.lock().expect("poisoned counter lock");
                // Reuse the counter so snapshot ids are deterministic under a seed.
                let v = *c;
                *c = c.wrapping_add(1);
                v
            };
            let snap_id = format!("{}-snap-{}", state.dm_name, counter);
            let snap_path = Self::snap_path(&state, &snap_id);

            // Suspend so in-flight I/O settles before we copy the backing store.
            run_cmd("dmsetup", &["suspend", &state.dm_name])
                .map_err(|e| format!("dmsetup suspend failed: {}", e))?;
            // Flush dirty pages from the mounted filesystem to the dm device first.
            let _ = run_cmd("sync", &[]);
            let copy = fs::copy(&state.backing_file, &snap_path);
            let _ = run_cmd("dmsetup", &["resume", &state.dm_name]);
            copy.map_err(|e| format!("snapshot failed: {}", e))?;

            info!("storage: snapshot {} -> {}", state.dm_name, snap_id);
            Ok(snap_id)
        })
    }

    fn restore<'a>(
        &'a self,
        subject: &'a Subject,
        snap_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let state = self.state_for(subject)?;
            let snap_path = Self::snap_path(&state, snap_id);
            if !snap_path.exists() {
                return Err(format!("snapshot {} not found", snap_id));
            }

            // Unmount so the page cache cannot serve stale data after we rewrite
            // the backing file under the device.
            run_cmd("umount", &[&state.host_mount.display().to_string()])
                .map_err(|e| format!("umount before restore failed: {}", e))?;

            run_cmd("dmsetup", &["suspend", &state.dm_name])
                .map_err(|e| format!("dmsetup suspend failed: {}", e))?;
            let copy = fs::copy(&snap_path, &state.backing_file);
            let _ = run_cmd("dmsetup", &["resume", &state.dm_name]);
            if let Err(e) = copy {
                // Best-effort remount so the subject is not left without a disk.
                let dm_path = format!("/dev/mapper/{}", state.dm_name);
                let _ = run_cmd(
                    "mount",
                    &[&dm_path, &state.host_mount.display().to_string()],
                );
                return Err(format!("restore copy failed: {}", e));
            }

            let _ = run_cmd(
                "blockdev",
                &["--flushbufs", &format!("/dev/mapper/{}", state.dm_name)],
            );
            let dm_path = format!("/dev/mapper/{}", state.dm_name);
            run_cmd(
                "mount",
                &[&dm_path, &state.host_mount.display().to_string()],
            )
            .map_err(|e| format!("remount after restore failed: {}", e))?;
            let _ = run_cmd("chmod", &["777", &state.host_mount.display().to_string()]);

            info!("storage: restored {} from {}", state.dm_name, snap_id);
            Ok(())
        })
    }
}

/// Run a command and return its stdout, or an error with stderr.
fn run_cmd(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("failed to execute {}: {}", program, e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(format!(
            "{} {} failed (exit {}): {} {}",
            program,
            args.join(" "),
            output.status.code().unwrap_or(-1),
            stdout.trim(),
            stderr.trim()
        ))
    } else {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_cmd_success() {
        assert!(run_cmd("true", &[]).is_ok());
    }

    #[test]
    fn test_run_cmd_failure() {
        assert!(run_cmd("false", &[]).is_err());
    }

    #[test]
    fn test_pass_through_table() {
        let t = DockerStorage::pass_through_table(2048, "/dev/loop0");
        assert_eq!(t, "0 2048 flakey /dev/loop0 0 999999 0");
    }

    #[test]
    fn test_down_table_drop_writes() {
        let t = DockerStorage::down_table(2048, "/dev/loop0", &["drop_writes"]);
        assert_eq!(t, "0 2048 flakey /dev/loop0 0 0 1 1 drop_writes");
    }

    #[test]
    fn test_dm_name_sanitises() {
        assert_eq!(
            DockerStorage::dm_name_for("dstest-config_1-1"),
            "dstest-dstest-config-1-1"
        );
    }

    #[test]
    fn test_storage_spec_validate_via_opts() {
        let bad = StorageOpts {
            size_mb: 0,
            mount: "/data".into(),
        };
        assert!(bad.validate().is_err());
        let bad_mount = StorageOpts {
            size_mb: 64,
            mount: "relative".into(),
        };
        assert!(bad_mount.validate().is_err());
        let ok = StorageOpts {
            size_mb: 64,
            mount: "/data".into(),
        };
        assert!(ok.validate().is_ok());
    }
}
