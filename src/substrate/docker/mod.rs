use std::collections::HashMap;
use std::fmt::{self, Display};
use std::fs;
use std::sync::Mutex;

use bollard::Docker as BollardDocker;
use bollard::container::LogOutput;
use bollard::errors::Error as BollardError;
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use bollard::models::{
    ContainerCreateBody, ContainerStateStatusEnum, ContainerUpdateBody, HostConfig,
    NetworkConnectRequest, NetworkDisconnectRequest, PortBinding, ThrottleDevice,
};
use bollard::query_parameters::CreateImageOptions;
use bollard::query_parameters::{
    CreateContainerOptions, InspectContainerOptions, LogsOptionsBuilder, RemoveContainerOptions,
    StartContainerOptions,
};
use futures_util::TryStreamExt;
use tracing::{debug, info, warn};

use crate::substrate::{
    ExecResult, Fault, HostedSubject, LogEntry, Stream, Subject, SubjectStatus, Substrate, ToLua,
};

pub mod clock;
pub mod network;
pub mod storage;

use clock::{ClockSpec, DockerClock};
use network::DockerNetwork;
use storage::{DockerStorage, StorageSpec};

pub struct Docker {
    connection: BollardDocker,
    original_limits: Mutex<HashMap<String, OriginalLimits>>,
    clock: DockerClock,
    network: DockerNetwork,
    storage: DockerStorage,
}

/// Resource limits read back from inspect right after container start.
#[derive(Clone, Copy, Debug, Default)]
struct OriginalLimits {
    memory: Option<i64>,
    memory_swap: Option<i64>,
    cpu_period: Option<i64>,
    cpu_quota: Option<i64>,
    blkio_weight: Option<u16>,
}

impl Docker {
    pub fn new() -> Result<Self, String> {
        let connection = BollardDocker::connect_with_local_defaults()
            .map_err(|e| format!("Failed to connect to Docker: {}", e))?;
        Ok(Self {
            clock: DockerClock::new(connection.clone()),
            network: DockerNetwork::new(connection.clone()),
            storage: DockerStorage::new(),
            connection,
            original_limits: Mutex::new(HashMap::new()),
        })
    }

    /// Pull an image (shared by subject hosting and clock asset builds).
    pub(crate) async fn pull_image(conn: &BollardDocker, image: &str) -> Result<(), String> {
        conn.create_image(
            Some(CreateImageOptions {
                from_image: Some(image.to_string()),
                ..Default::default()
            }),
            None,
            None,
        )
        .try_collect::<Vec<_>>()
        .await
        .map_err(|e| format!("Failed to pull image: {}", e))?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockerState {
    Running,
    Paused,
    Exited,
    Dead,
}

impl Display for DockerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DockerState::Running => write!(f, "running"),
            DockerState::Paused => write!(f, "paused"),
            DockerState::Exited => write!(f, "exited"),
            DockerState::Dead => write!(f, "dead"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DockerInspect {
    pub state: DockerState,
    pub pid: Option<u32>,
    pub ip: Option<String>,
    pub memory_limit: Option<u64>,
    pub cpu_quota: Option<f64>,
}

impl ToLua for DockerInspect {
    fn to_lua(self, lua: &mlua::Lua) -> mlua::Result<mlua::Value> {
        let t = lua.create_table()?;
        t.set("state", self.state.to_string())?;
        t.set("pid", self.pid)?;
        t.set("ip", self.ip)?;
        t.set("memory_limit", self.memory_limit)?;
        t.set("cpu_quota", self.cpu_quota)?;
        Ok(mlua::Value::Table(t))
    }
}

#[derive(Clone, Debug)]
pub struct DockerLogOpts {
    pub stdout: bool,
    pub stderr: bool,
    pub tail: Option<String>,
    pub since: Option<i32>,
    pub timestamps: bool,
}

impl Default for DockerLogOpts {
    fn default() -> Self {
        Self {
            stdout: true,
            stderr: true,
            tail: None,
            since: None,
            timestamps: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DockerSubjectData {
    pub image: String,
    pub runtime: Option<String>,
    pub cmd: Option<Vec<String>>,
    pub ports: Option<Vec<u16>>,
    pub volumes: Option<Vec<String>>,
    pub env: Option<Vec<String>>,
    pub clock: Option<ClockSpec>,
    pub network_proxied: bool,
    pub storage: Option<StorageSpec>,
}

impl Substrate for Docker {
    const NAME: &'static str = "docker";

    type SubjectData = DockerSubjectData;
    type Inspect = DockerInspect;
    type LogOpts = DockerLogOpts;
    type Clock = DockerClock;
    type Network = DockerNetwork;
    type Storage = DockerStorage;

    fn parse_subject(&self, table: &mlua::Table) -> Result<Self::SubjectData, String> {
        let image: String = table
            .get("image")
            .map_err(|_| "setup requires `image` field".to_string())?;
        let runtime: Option<String> = table.get("runtime").ok();
        let ports: Option<Vec<u16>> = table.get("ports").ok();
        let cmd: Option<Vec<String>> = table.get("cmd").ok();
        let volumes: Option<Vec<String>> = table.get("volumes").ok();
        let env: Option<HashMap<String, String>> = table.get("env").ok();
        let env = env.map(|e| e.into_iter().map(|(k, v)| format!("{}={}", k, v)).collect());

        // Optional virtual clock: clock = { virtual = true, start_epoch = <unix secs> }
        let clock = match table.get::<mlua::Table>("clock") {
            Ok(t) if t.get::<bool>("virtual").unwrap_or(false) => Some(ClockSpec {
                start_epoch_secs: t.get("start_epoch").ok(),
            }),
            _ => None,
        };

        // Optional proxied networking: network = { proxied = true }
        let network_proxied = table
            .get::<mlua::Table>("network")
            .ok()
            .and_then(|t| t.get::<bool>("proxied").ok())
            .unwrap_or(false);

        // Optional virtual disk: storage = { flaky = true, mount = "/data", size_mb = 512 }
        let storage = match table.get::<mlua::Table>("storage") {
            Ok(t) if t.get::<bool>("flaky").unwrap_or(false) => {
                let spec = StorageSpec {
                    size_mb: t.get("size_mb").unwrap_or(512),
                    mount: t
                        .get("mount")
                        .map_err(|_| "storage requires a `mount` field".to_string())?,
                };
                crate::components::StorageOpts::from(spec.clone()).validate()?;
                Some(spec)
            }
            _ => None,
        };

        Ok(DockerSubjectData {
            image,
            runtime,
            cmd,
            ports,
            volumes,
            env,
            clock,
            network_proxied,
            storage,
        })
    }

    fn parse_log_opts(&self, table: Option<&mlua::Table>) -> Result<Self::LogOpts, String> {
        let Some(table) = table else {
            return Ok(DockerLogOpts::default());
        };
        Ok(DockerLogOpts {
            stdout: table.get("stdout").unwrap_or(true),
            stderr: table.get("stderr").unwrap_or(true),
            tail: table.get("tail").ok(),
            since: table.get("since").ok(),
            timestamps: table.get("timestamps").unwrap_or(false),
        })
    }

    async fn host(&self, name: &str, data: &Self::SubjectData) -> Result<HostedSubject, String> {
        let conn = &self.connection;

        Self::pull_image(conn, &data.image).await?;

        // Virtual clock opt-in: prepare libfaketime env + control-file mount.
        let clock_prep = match &data.clock {
            Some(spec) => Some(self.clock.prepare(name, spec).await?),
            None => None,
        };

        let mut env = data.env.clone().unwrap_or_default();
        let mut binds = data.volumes.clone().unwrap_or_default();
        if let Some(prep) = &clock_prep {
            env.extend(prep.env.iter().cloned());
            binds.push(prep.bind.clone());
        }

        // Virtual disk opt-in: prepare dm-flakey device + bind mount.
        // If later host steps fail, discard the pending device so we don't
        // leak loop/dm resources.
        let storage_prep = match &data.storage {
            Some(spec) => Some(self.storage.prepare(name, spec)?),
            None => None,
        };
        if let Some(prep) = &storage_prep {
            binds.push(prep.bind.clone());
        }

        let host_result = async {
            let mut labels = HashMap::new();
            labels.insert("dstest.managed".to_string(), "true".to_string());

            let container_config = ContainerCreateBody {
                image: Some(data.image.clone()),
                cmd: data.cmd.clone(),
                labels: Some(labels),
                exposed_ports: data
                    .ports
                    .as_ref()
                    .map(|ports| ports.iter().map(|p| format!("{}/tcp", p)).collect()),
                host_config: Some(HostConfig {
                    runtime: data.runtime.clone(),
                    // Host port "0" => Docker assigns an ephemeral port, so
                    // multiple subjects (and parallel experiments) never collide.
                    port_bindings: data.ports.as_ref().map(|ports| {
                        let mut map: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();
                        for p in ports {
                            map.insert(
                                format!("{}/tcp", p),
                                Some(vec![PortBinding {
                                    host_ip: None,
                                    host_port: Some("0".to_string()),
                                }]),
                            );
                        }
                        map
                    }),
                    binds: if binds.is_empty() { None } else { Some(binds) },
                    // Proxied subjects get host.docker.internal so they can dial
                    // proxy listeners on the host.
                    extra_hosts: if data.network_proxied {
                        Some(vec!["host.docker.internal:host-gateway".to_string()])
                    } else {
                        None
                    },
                    ..Default::default()
                }),
                env: if env.is_empty() { None } else { Some(env) },
                ..Default::default()
            };

            let options = CreateContainerOptions {
                name: Some(name.to_string()),
                ..Default::default()
            };

            let container = match conn
                .create_container(Some(options.clone()), container_config.clone())
                .await
            {
                Ok(c) => c,
                Err(BollardError::DockerResponseServerError {
                    status_code: 409, ..
                }) => {
                    // Name collision: remove the stale dstest-managed container
                    // holding this name, then retry once.
                    self.remove_stale(name).await?;
                    conn.create_container(Some(options), container_config)
                        .await
                        .map_err(|e| format!("Failed to create container: {}", e))?
                }
                Err(e) => return Err(format!("Failed to create container: {}", e)),
            };

            let container_id = container.id.clone();
            conn.start_container(&container_id, None::<StartContainerOptions>)
                .await
                .map_err(|e| format!("Failed to start container: {}", e))?;

            let info = conn
                .inspect_container(&container_id, None::<InspectContainerOptions>)
                .await
                .map_err(|e| format!("Inspect after start failed: {}", e))?;

            // Discover the ephemeral host ports Docker assigned.
            let addr = data.ports.as_ref().and_then(|ports| {
                ports.first().and_then(|p| {
                    Self::host_port_for(&info, *p).map(|hp| format!("localhost:{}", hp))
                })
            });

            let originals = info
                .host_config
                .as_ref()
                .map(|hc| OriginalLimits {
                    memory: hc.memory,
                    memory_swap: hc.memory_swap,
                    cpu_period: hc.cpu_period,
                    cpu_quota: hc.cpu_quota,
                    blkio_weight: hc.blkio_weight,
                })
                .unwrap_or_default();
            self.original_limits
                .lock()
                .expect("poisoned limits lock")
                .insert(container_id.clone(), originals);

            if let Some(prep) = clock_prep {
                self.clock.register(container_id.clone(), prep.ctl_path);
            }

            if let Some(prep) = &storage_prep {
                self.storage.register(container_id.clone(), &prep.dm_name);
            }

            Ok((container_id, addr))
        }
        .await;

        let (container_id, addr) = match host_result {
            Ok(v) => v,
            Err(e) => {
                if let Some(prep) = &storage_prep {
                    self.storage.discard(&prep.dm_name);
                }
                return Err(e);
            }
        };

        // Register the host-mapped address so the network proxy can resolve it
        // for link forwarding.
        if let Some(ref addr) = addr {
            self.network
                .register_host(container_id.clone(), addr.clone());
        }

        info!("Started container name={} id={}", name, container_id);
        Ok(HostedSubject {
            id: container_id,
            addr,
        })
    }

    async fn affect(&self, subject: &Subject, fault: &Fault) -> Result<(), String> {
        let id = Self::container_id(subject).to_string();

        match fault {
            Fault::Pause => match self.connection.pause_container(&id).await {
                Ok(_) => info!("Paused container id={}", id),
                Err(BollardError::DockerResponseServerError {
                    status_code: 409, ..
                }) => debug!("Container id={} already paused", id),
                Err(e) => return Err(format!("Failed to pause container {}: {}", id, e)),
            },
            Fault::Kill => match self.connection.kill_container(&id, None).await {
                Ok(_) => info!("Killed container id={}", id),
                Err(BollardError::DockerResponseServerError {
                    status_code: 409, ..
                }) => debug!("Container id={} not running", id),
                Err(e) => return Err(format!("Failed to kill container {}: {}", id, e)),
            },
            Fault::Deprive(tier) => {
                info!("Depriving container id={} tier={}", id, tier);
                self.deprive_resource(subject, tier).await?;
            }
        }
        Ok(())
    }

    async fn clear_faults(&self, subject: &Subject) -> Result<(), String> {
        let id = Self::container_id(subject).to_string();
        info!("Clearing faults id={}", id);

        match self.connection.unpause_container(&id).await {
            Ok(_) => debug!("Unpaused container id={}", id),
            Err(BollardError::DockerResponseServerError {
                status_code: 409, ..
            }) => {}
            Err(BollardError::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(e) => debug!("Failed to unpause container id={} error=\"{}\"", id, e),
        }

        self.restart_if_killed(subject).await?;
        self.reconnect_network(subject).await?;
        self.restore_resource_limits(subject).await?;

        Ok(())
    }

    async fn teardown(&self, subject: Subject) -> Result<(), String> {
        let id = Self::container_id(&subject).to_string();
        info!("Tearing down container id={}", id);

        self.connection
            .stop_container(&id, None)
            .await
            .map_err(|e| format!("Failed to stop container: {}", e))?;

        let options = RemoveContainerOptions {
            v: true,
            force: true,
            link: false,
        };
        self.connection
            .remove_container(&id, Some(options))
            .await
            .map_err(|e| format!("Failed to remove container: {}", e))?;

        self.original_limits
            .lock()
            .expect("poisoned limits lock")
            .remove(&id);

        self.clock.unregister(&id);
        self.network.unregister_subject(&id);
        self.storage.unregister(&id);

        Ok(())
    }

    async fn logs(&self, subject: &Subject, opts: DockerLogOpts) -> Result<Vec<LogEntry>, String> {
        let id = Self::container_id(subject).to_string();

        let mut builder = LogsOptionsBuilder::new()
            .stdout(opts.stdout)
            .stderr(opts.stderr)
            .timestamps(opts.timestamps);

        if let Some(tail) = opts.tail {
            builder = builder.tail(&tail);
        }
        if let Some(since) = opts.since {
            builder = builder.since(since);
        }

        let options = builder.build();

        let stream = self
            .connection
            .logs(&id, Some(options))
            .try_collect::<Vec<_>>()
            .await
            .map_err(|e| format!("Failed to get logs: {}", e))?;

        stream
            .into_iter()
            .filter_map(|entry| match entry {
                LogOutput::StdOut { message } => Some(LogEntry {
                    stream: Stream::StdOut,
                    message: String::from_utf8_lossy(&message).to_string(),
                }),
                LogOutput::StdErr { message } => Some(LogEntry {
                    stream: Stream::StdErr,
                    message: String::from_utf8_lossy(&message).to_string(),
                }),
                _ => None,
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(Ok)
            .collect()
    }

    async fn inspect(&self, subject: &Subject) -> Result<DockerInspect, String> {
        let id = Self::container_id(subject).to_string();

        let info = self
            .connection
            .inspect_container(&id, None::<InspectContainerOptions>)
            .await
            .map_err(|e| format!("Inspect failed: {}", e))?;

        let state = match info.state.as_ref().and_then(|s| s.status) {
            Some(ContainerStateStatusEnum::RUNNING) => DockerState::Running,
            Some(ContainerStateStatusEnum::PAUSED) => DockerState::Paused,
            Some(ContainerStateStatusEnum::EXITED) => DockerState::Exited,
            Some(ContainerStateStatusEnum::DEAD) => DockerState::Dead,
            _ => DockerState::Dead,
        };

        Ok(DockerInspect {
            state,
            pid: info.state.as_ref().and_then(|s| s.pid.map(|p| p as u32)),
            ip: info
                .network_settings
                .and_then(|n| n.networks)
                .and_then(|networks| {
                    networks
                        .values()
                        .next()
                        .and_then(|endpoint| endpoint.ip_address.clone())
                }),
            memory_limit: info
                .host_config
                .as_ref()
                .and_then(|h| h.memory.map(|m| m as u64)),
            cpu_quota: info.host_config.as_ref().and_then(|h| {
                h.cpu_quota
                    .zip(h.cpu_period)
                    .filter(|(q, p)| *q > 0 && *p > 0)
                    .map(|(q, p)| q as f64 / p as f64)
            }),
        })
    }

    async fn exec(&self, subject: &Subject, cmd: &[String]) -> Result<ExecResult, String> {
        let id = Self::container_id(subject).to_string();

        let config = CreateExecOptions {
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            cmd: Some(cmd.iter().map(|s| s.as_str()).collect()),
            ..Default::default()
        };
        let exec = self
            .connection
            .create_exec(&id, config)
            .await
            .map_err(|e| format!("Create exec failed: {}", e))?;

        let result = self
            .connection
            .start_exec(&exec.id, Some(StartExecOptions::default()))
            .await
            .map_err(|e| format!("Start exec failed: {}", e))?;

        let (stdout, stderr) = match result {
            StartExecResults::Attached { output, .. } => {
                let entries = output
                    .try_collect::<Vec<_>>()
                    .await
                    .map_err(|e| format!("Exec output failed: {}", e))?;

                let mut stdout = String::new();
                let mut stderr = String::new();

                for entry in entries {
                    match entry {
                        LogOutput::StdOut { message } => {
                            stdout.push_str(&String::from_utf8_lossy(&message));
                        }
                        LogOutput::StdErr { message } => {
                            stderr.push_str(&String::from_utf8_lossy(&message));
                        }
                        _ => {}
                    }
                }
                (stdout, stderr)
            }
            StartExecResults::Detached => (String::new(), String::new()),
        };

        let inspect = self
            .connection
            .inspect_exec(&exec.id)
            .await
            .map_err(|e| format!("Inspect exec failed: {}", e))?;

        Ok(ExecResult {
            exit_code: inspect.exit_code.unwrap_or(-1) as i32,
            stdout,
            stderr,
        })
    }

    async fn status(&self, subject: &Subject) -> Result<SubjectStatus, String> {
        let id = Self::container_id(subject).to_string();

        let info = self
            .connection
            .inspect_container(&id, None::<InspectContainerOptions>)
            .await
            .map_err(|e| format!("Inspect failed for status: {}", e))?;

        let state = info.state.and_then(|s| s.status);
        match state {
            Some(ContainerStateStatusEnum::RUNNING) => Ok(SubjectStatus::Running),
            Some(ContainerStateStatusEnum::PAUSED) => Ok(SubjectStatus::Pending),
            Some(ContainerStateStatusEnum::CREATED) => Ok(SubjectStatus::Pending),
            Some(ContainerStateStatusEnum::RESTARTING) => Ok(SubjectStatus::Pending),
            Some(ContainerStateStatusEnum::REMOVING) => Ok(SubjectStatus::Pending),
            Some(ContainerStateStatusEnum::EXITED) => Ok(SubjectStatus::Terminated),
            Some(ContainerStateStatusEnum::DEAD) => Ok(SubjectStatus::Terminated),
            _ => Ok(SubjectStatus::Terminated),
        }
    }

    fn clock(&self) -> &Self::Clock {
        &self.clock
    }

    fn network(&self) -> &Self::Network {
        &self.network
    }

    fn storage(&self) -> &Self::Storage {
        &self.storage
    }
}

impl Docker {
    fn container_id(subject: &Subject) -> &str {
        subject.id.strip_prefix("docker/").unwrap_or(&subject.id)
    }

    /// Find the ephemeral host port Docker assigned to a container port.
    fn host_port_for(
        info: &bollard::models::ContainerInspectResponse,
        container_port: u16,
    ) -> Option<u16> {
        let ports = info.network_settings.as_ref()?.ports.as_ref()?;
        ports
            .get(&format!("{}/tcp", container_port))?
            .as_ref()?
            .first()?
            .host_port
            .as_ref()?
            .parse()
            .ok()
    }

    /// Remove a stale dstest-managed container occupying `name`. Refuses to
    /// touch containers not labelled as dstest-managed.
    async fn remove_stale(&self, name: &str) -> Result<(), String> {
        let info = self
            .connection
            .inspect_container(name, None::<InspectContainerOptions>)
            .await
            .map_err(|e| format!("Failed to inspect stale container {}: {}", name, e))?;

        let managed = info
            .config
            .and_then(|c| c.labels)
            .and_then(|l| l.get("dstest.managed").cloned())
            .is_some_and(|v| v == "true");

        if !managed {
            return Err(format!(
                "container name '{}' is in use by a container not managed by dstest",
                name
            ));
        }

        warn!("Removing stale dstest container name={}", name);
        let options = RemoveContainerOptions {
            v: true,
            force: true,
            link: false,
        };
        self.connection
            .remove_container(name, Some(options))
            .await
            .map_err(|e| format!("Failed to remove stale container {}: {}", name, e))?;
        Ok(())
    }

    /// Resolve the host's root filesystem to the physical whole block device
    /// backing it, walking through dm-crypt/LVM/btrfs layers and partitions.
    fn root_block_device() -> Option<String> {
        let source = Self::root_mount_source()?;
        let canonical = fs::canonicalize(&source).ok()?;
        let name = canonical.file_name()?.to_string_lossy().to_string();
        Self::resolve_to_physical(&name)
    }

    /// Extract the source device of the root mount from `/proc/self/mountinfo`.
    /// The mount source sits two fields past the optional-fields separator `-`.
    fn root_mount_source() -> Option<String> {
        let mountinfo = fs::read_to_string("/proc/self/mountinfo").ok()?;
        for line in mountinfo.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() > 4 && fields[4] == "/" {
                let dash = fields.iter().position(|f| *f == "-")?;
                return fields.get(dash + 2).map(|s| s.to_string());
            }
        }
        None
    }

    /// Walk a block device name through slaves (dm-crypt/LVM/RAID) and
    /// partition parents to the underlying physical whole device, returning
    /// its `/dev/<name>` path. Verifies the result is throttleable on cgroup
    /// v2 via `io.stat`, since `io.max` only accepts whole-device major:minor
    /// pairs that appear there.
    fn resolve_to_physical(name: &str) -> Option<String> {
        let mut current = name.to_string();
        loop {
            // If this device is backed by slaves (e.g. dm-0 backed by
            // nvme0n1p2), recurse into the first slave.
            let slaves_dir = format!("/sys/class/block/{}/slaves", current);
            if let Ok(slaves) = fs::read_dir(&slaves_dir)
                && let Some(slave) = slaves.flatten().next()
            {
                current = slave.file_name().to_string_lossy().to_string();
                continue;
            }
            // If this device is a partition, walk to its parent whole device.
            let partition_file = format!("/sys/class/block/{}/partition", current);
            if fs::metadata(&partition_file).is_ok() {
                let parent = fs::canonicalize(format!("/sys/class/block/{}", current))
                    .ok()?
                    .parent()?
                    .file_name()?
                    .to_string_lossy()
                    .to_string();
                current = parent;
                continue;
            }
            break;
        }
        let dev_file = format!("/sys/class/block/{}/dev", current);
        let majmin = fs::read_to_string(dev_file).ok()?.trim().to_string();
        if !Self::in_io_stat(&majmin) {
            debug!(
                "resolved device /dev/{} ({} not in io.stat; not throttleable on cgroup v2)",
                current, majmin
            );
            return None;
        }
        Some(format!("/dev/{}", current))
    }

    /// Check whether a `major:minor` pair appears in `/sys/fs/cgroup/io.stat`.
    fn in_io_stat(majmin: &str) -> bool {
        let Ok(stat) = fs::read_to_string("/sys/fs/cgroup/io.stat") else {
            return false;
        };
        stat.lines()
            .filter_map(|l| l.split_whitespace().next())
            .any(|m| m == majmin)
    }

    /// Fallback: scan `/dev` for the first whole block device matching common
    /// NVMe/SCSI/virtio prefixes, skipping char devices (e.g. `nvme0`) and
    /// partitions. Prefers whole devices, falls back to partitions.
    fn fallback_block_device() -> Option<String> {
        let entries = fs::read_dir("/dev").ok()?;
        let mut whole: Option<String> = None;
        let mut partition: Option<String> = None;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !(name.starts_with("nvme") || name.starts_with("sd") || name.starts_with("vd")) {
                continue;
            }
            // Only real block devices have a sysfs entry under /sys/class/block.
            if fs::metadata(format!("/sys/class/block/{}", name)).is_err() {
                continue;
            }
            if fs::metadata(format!("/sys/class/block/{}/partition", name)).is_ok() {
                partition.get_or_insert(format!("/dev/{}", name));
            } else {
                whole.get_or_insert(format!("/dev/{}", name));
            }
        }
        whole.or(partition)
    }

    async fn deprive_resource(
        &self,
        subject: &Subject,
        tier: &crate::fault::Tier,
    ) -> Result<(), String> {
        let id = Self::container_id(subject).to_string();

        match tier {
            crate::fault::Tier::Disk => {
                info!("Applying OCI storage I/O fault for container id={}", id);

                // `blkio_weight` is a device-agnostic OCI block-IO fault: it
                // throttles the container's I/O scheduling weight relative to
                // its peers. It needs no host block-device resolution, so it
                // works on any substrate — including a podman/docker machine VM
                // where the orchestrator runs on macOS and the container's
                // backing device is not visible from the host.
                let mut update_config = ContainerUpdateBody {
                    blkio_weight: Some(50),
                    ..Default::default()
                };

                // Best-effort absolute bandwidth cap (1MB/s). cgroup v2
                // `io.max` only accepts whole-device major:minor pairs, so this
                // requires a resolvable host block device. Skip it silently when
                // one can't be found instead of failing the whole experiment.
                if let Some(device) = Self::root_block_device().or_else(Self::fallback_block_device)
                {
                    info!(
                        "Capping disk I/O for container id={} on {} to 1MB/s",
                        id, device
                    );
                    update_config.blkio_device_read_bps = Some(vec![ThrottleDevice {
                        path: Some(device.clone()),
                        rate: Some(1024 * 1024),
                    }]);
                    update_config.blkio_device_write_bps = Some(vec![ThrottleDevice {
                        path: Some(device),
                        rate: Some(1024 * 1024),
                    }]);
                } else {
                    debug!(
                        "no throttleable host block device resolved; applying weight-only disk fault"
                    );
                }

                self.connection
                    .update_container(&id, update_config)
                    .await
                    .map_err(|e| format!("Failed to throttle disk: {}", e))?;
            }
            crate::fault::Tier::Network => {
                info!("Disconnecting network for container id={}", id);
                let disconnect = NetworkDisconnectRequest {
                    container: id.clone(),
                    force: Some(true),
                };
                match self
                    .connection
                    .disconnect_network("bridge", disconnect)
                    .await
                {
                    Ok(_) => info!("Container disconnected from bridge network"),
                    Err(e) => {
                        warn!(
                            "Failed to disconnect network (may already be disconnected): {}",
                            e
                        );
                    }
                }
            }
            crate::fault::Tier::Memory => {
                let container_info = self
                    .connection
                    .inspect_container(&id, None::<InspectContainerOptions>)
                    .await
                    .map_err(|e| format!("Failed to inspect container: {}", e))?;

                let current_limit = container_info
                    .host_config
                    .and_then(|hc| hc.memory)
                    .unwrap_or(0);

                let new_limit = if current_limit > 0 {
                    (current_limit / 2).max(64 * 1024 * 1024)
                } else {
                    64 * 1024 * 1024
                };

                info!(
                    "Limiting memory for container id={} to {}MB (was {}MB)",
                    id,
                    new_limit / (1024 * 1024),
                    current_limit / (1024 * 1024)
                );

                let update_config = ContainerUpdateBody {
                    memory: Some(new_limit),
                    memory_swap: Some(new_limit),
                    ..Default::default()
                };
                self.connection
                    .update_container(&id, update_config)
                    .await
                    .map_err(|e| format!("Failed to limit memory: {}", e))?;
            }
            crate::fault::Tier::Cpu => {
                info!("Throttling CPU for container id={}", id);
                let update_config = ContainerUpdateBody {
                    cpu_period: Some(100000),
                    cpu_quota: Some(20000),
                    ..Default::default()
                };
                self.connection
                    .update_container(&id, update_config)
                    .await
                    .map_err(|e| format!("Failed to throttle CPU: {}", e))?;
            }
        }

        Ok(())
    }

    async fn restart_if_killed(&self, subject: &Subject) -> Result<(), String> {
        let id = Self::container_id(subject).to_string();

        match self
            .connection
            .inspect_container(&id, None::<InspectContainerOptions>)
            .await
        {
            Ok(container) => {
                if let Some(state) = container.state
                    && state.status == Some(ContainerStateStatusEnum::EXITED)
                {
                    info!("Restarting killed container id={}", id);
                    self.connection
                        .restart_container(&id, None)
                        .await
                        .map_err(|e| format!("Failed to restart container: {}", e))?;
                }
            }
            Err(e) => {
                debug!("Could not inspect container: {}", e);
            }
        }

        Ok(())
    }

    async fn reconnect_network(&self, subject: &Subject) -> Result<(), String> {
        let id = Self::container_id(subject).to_string();

        let connect = NetworkConnectRequest {
            container: id.clone(),
            endpoint_config: None,
        };

        match self.connection.connect_network("bridge", connect).await {
            Ok(_) => info!("Reconnected container to bridge network"),
            Err(e) => {
                debug!(
                    "Network reconnect skipped (may already be connected): {}",
                    e
                );
            }
        }

        Ok(())
    }

    /// Restore the resource limits captured at `host()` time. Docker's update
    /// API treats absent fields as "no change", so restoring means writing
    /// values back explicitly. Caveat: the daemon also treats **0 as "no
    /// change"** and rejects -1 (verified empirically), so an originally
    /// *unlimited* resource cannot be reset to unlimited in place — we
    /// approximate it instead: host-total memory, all-cores CPU quota, and
    /// the cgroup-default blkio weight. Practically equivalent for a test
    /// harness; exact reset would require recreating the container.
    async fn restore_resource_limits(&self, subject: &Subject) -> Result<(), String> {
        let id = Self::container_id(subject).to_string();

        let orig = self
            .original_limits
            .lock()
            .expect("poisoned limits lock")
            .get(&id)
            .copied()
            .unwrap_or_default();

        let memory = orig.memory.filter(|m| *m > 0).unwrap_or_else(|| {
            debug!("original memory unlimited; approximating with host total");
            Self::host_mem_total().unwrap_or(i64::MAX)
        });
        let memory_swap = orig
            .memory_swap
            .filter(|s| *s > 0)
            .unwrap_or(memory.saturating_mul(2));
        let cpu_period = orig.cpu_period.filter(|p| *p > 0).unwrap_or(100_000);
        let cpu_quota = orig.cpu_quota.filter(|q| *q > 0).unwrap_or_else(|| {
            let nproc = std::thread::available_parallelism()
                .map(|n| n.get() as i64)
                .unwrap_or(1);
            debug!("original cpu quota unlimited; approximating with all cores");
            cpu_period.saturating_mul(nproc)
        });
        let blkio_weight = orig
            .blkio_weight
            .filter(|w| *w > 0)
            .unwrap_or_else(Self::default_blkio_weight);

        let update_config = ContainerUpdateBody {
            memory: Some(memory),
            memory_swap: Some(memory_swap),
            cpu_period: Some(cpu_period),
            cpu_quota: Some(cpu_quota),
            blkio_weight: Some(blkio_weight),
            // Empty lists clear per-device throttle rules.
            blkio_device_read_bps: Some(vec![]),
            blkio_device_write_bps: Some(vec![]),
            ..Default::default()
        };

        match self.connection.update_container(&id, update_config).await {
            Ok(_) => debug!("Restored resource limits for container id={}", id),
            Err(BollardError::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(e) => {
                debug!(
                    "Failed to restore resource limits for container id={} error=\"{}\"",
                    id, e
                )
            }
        }

        Ok(())
    }

    /// Host total memory in bytes, from /proc/meminfo.
    fn host_mem_total() -> Option<i64> {
        let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
        for line in meminfo.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                let kb: i64 = rest.trim().strip_suffix(" kB")?.trim().parse().ok()?;
                return Some(kb * 1024);
            }
        }
        None
    }

    /// The cgroup-default blkio weight: 100 on cgroup v2 (io.weight),
    /// 500 on cgroup v1 (blkio.weight).
    fn default_blkio_weight() -> u16 {
        if fs::metadata("/sys/fs/cgroup/cgroup.controllers").is_ok() {
            100
        } else {
            500
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_docker_new() {
        // Skips gracefully on hosts without a Docker daemon (e.g. CI).
        match Docker::new() {
            Ok(_) => {}
            Err(e) => eprintln!("skipping Docker::new test, no daemon: {e}"),
        }
    }

    /// The root block device must resolve to a real, throttleable whole
    /// device: it must exist, be a block device, and its `major:minor` must
    /// appear in `/sys/fs/cgroup/io.stat` (the cgroup v2 throttle list).
    #[test]
    fn test_root_block_device_resolves_to_throttleable_whole_device() {
        let Some(device) = Docker::root_block_device() else {
            return; // host without a resolvable root device (e.g. CI)
        };
        let meta = fs::metadata(&device)
            .unwrap_or_else(|e| panic!("resolved device {device} missing: {e}"));
        use std::os::unix::fs::FileTypeExt;
        assert!(
            meta.file_type().is_block_device(),
            "{device} is not a block device",
        );
        let name = device.strip_prefix("/dev/").unwrap_or(&device);
        assert!(
            fs::metadata(format!("/sys/class/block/{name}/partition")).is_err(),
            "{device} is a partition, not a whole device",
        );
        let majmin = fs::read_to_string(format!("/sys/class/block/{name}/dev"))
            .expect("sysfs dev file")
            .trim()
            .to_string();
        assert!(
            Docker::in_io_stat(&majmin),
            "{device} ({majmin}) not in io.stat — not throttleable on cgroup v2",
        );
    }

    /// `root_mount_source` must find a non-empty source path for the root mount.
    #[test]
    fn test_root_mount_source_present() {
        // `/proc/self/mountinfo` only exists on Linux; skip elsewhere (e.g. a
        // macOS podman/docker machine host).
        if fs::read_to_string("/proc/self/mountinfo").is_err() {
            return;
        }
        let source = Docker::root_mount_source();
        assert!(source.is_some(), "no root mount source found in mountinfo");
        let source = source.unwrap();
        assert!(!source.is_empty());
    }

    #[test]
    fn test_parse_subject_runtime() {
        let lua = mlua::Lua::new();
        let table = lua.create_table().unwrap();
        table.set("image", "alpine").unwrap();
        table.set("runtime", "dtrun").unwrap();

        let docker = Docker {
            connection: BollardDocker::connect_with_local_defaults().unwrap_or_else(|_| {
                // Dummy client fallback for non-docker test environments
                BollardDocker::connect_with_http_defaults().unwrap()
            }),
            original_limits: Mutex::new(HashMap::new()),
            clock: DockerClock::new(BollardDocker::connect_with_http_defaults().unwrap()),
            network: DockerNetwork::new(BollardDocker::connect_with_http_defaults().unwrap()),
            storage: DockerStorage::new(),
        };

        let parsed = docker.parse_subject(&table).unwrap();
        assert_eq!(parsed.runtime.as_deref(), Some("dtrun"));

        let table_default = lua.create_table().unwrap();
        table_default.set("image", "alpine").unwrap();
        let parsed_default = docker.parse_subject(&table_default).unwrap();
        assert_eq!(parsed_default.runtime, None);
    }
}
