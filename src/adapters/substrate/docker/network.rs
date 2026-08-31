//! Deterministic network links via per-link socat proxy containers.
//!
//! `dstest.net.link(a, b, port)` creates a lightweight Alpine container
//! running `socat` as a TCP forwarder on the same Docker bridge as the
//! subjects. The source subject connects to the proxy container's bridge
//! IP; the proxy forwards to the target subject's bridge IP. Impairments
//! are applied at runtime via `tc netem` (latency, jitter, loss) and
//! `iptables` (partition: blackhole / reset) inside the proxy container.
//!
//! A proxy base image (`dstest-proxy`) with `socat`, `iproute2`, and
//! `iptables` is built once and cached.
//!
//! Limitations:
//! - `tc netem` randomness uses the kernel PRNG, not the experiment seed
//!   (full determinism for impairment *sampling* requires a custom proxy
//!   binary — Phase 1d). Impairment *parameters* (latency ms, loss pct)
//!   are deterministic from the script.
//! - Directional impairments are not supported yet (`tc` on a single
//!   interface is bidirectional). `Direction::AToB` / `BToA` are treated
//!   the same as `Direction::Both`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use bollard::Docker as BollardDocker;
use bollard::models::{ContainerCreateBody, HostConfig};
use bollard::query_parameters::{
    CreateContainerOptions, InspectContainerOptions, RemoveContainerOptions, StartContainerOptions,
};
use futures_util::TryStreamExt;
use tracing::{debug, info, warn};

use crate::domain::subject::Subject;
use crate::ports::components::{Direction, LinkId, NetworkControl, PartitionMode};

const PROXY_IMAGE: &str = "alpine:3.20";

struct LinkInner {
    /// Container name of the proxy container.
    container_name: String,
    /// Bridge IP of the proxy container (the address the source dials).
    addr: String,
    /// Current netem delay (ms), if any.
    delay_ms: Option<u64>,
    /// Current netem jitter (ms), if any.
    jitter_ms: Option<u64>,
    /// Current netem loss fraction (0.0–1.0), if any.
    loss_pct: Option<f64>,
}

pub struct DockerNetwork {
    connection: OnceLock<Result<BollardDocker, String>>,
    seed: Mutex<Option<u64>>,
    links: Mutex<HashMap<LinkId, LinkInner>>,
    link_counter: Mutex<usize>,
    image_built: std::sync::OnceLock<()>,
}

impl DockerNetwork {
    pub fn new() -> Self {
        Self {
            connection: OnceLock::new(),
            seed: Mutex::new(None),
            links: Mutex::new(HashMap::new()),
            link_counter: Mutex::new(0),
            image_built: std::sync::OnceLock::new(),
        }
    }

    /// Lazily connect to the Docker daemon on first use.
    fn connection(&self) -> Result<&BollardDocker, String> {
        let init = self.connection.get_or_init(|| {
            BollardDocker::connect_with_local_defaults()
                .map_err(|e| format!("Failed to connect to Docker: {}", e))
        });
        init.as_ref().map_err(|e| e.clone())
    }

    fn next_link_id(&self) -> LinkId {
        let mut counter = self.link_counter.lock().expect("poisoned counter lock");
        *counter += 1;
        LinkId(format!("link-{}", *counter))
    }

    /// Remove links involving a torn-down subject (called by `Docker::teardown`).
    pub fn unregister_subject(&self, _container_id: &str) {
        // We can't know which links involve this subject from the LinkId
        // alone, so we cancel all links. They'll be recreated if needed.
        let mut links = self.links.lock().expect("poisoned links lock");
        for (_, inner) in links.drain() {
            let name = inner.container_name.clone();
            let conn = match self.connection() {
                Ok(c) => c.clone(),
                Err(e) => {
                    warn!("unregister_subject: cannot connect to Docker: {}", e);
                    continue;
                }
            };
            tokio::spawn(async move {
                let opts = RemoveContainerOptions {
                    v: true,
                    force: true,
                    link: false,
                };
                if let Err(e) = conn.remove_container(&name, Some(opts)).await {
                    warn!("failed to remove proxy container {}: {}", name, e);
                }
            });
        }
    }

    /// Ensure the proxy base image is pulled. The image is stock alpine;
    /// socat/iproute2/iptables are installed at container start time.
    async fn ensure_image(&self) -> Result<(), String> {
        if self.image_built.get().is_some() {
            return Ok(());
        }

        // Pull alpine if not already present.
        self.connection()?
            .create_image(
                Some(bollard::query_parameters::CreateImageOptions {
                    from_image: Some(PROXY_IMAGE.to_string()),
                    ..Default::default()
                }),
                None,
                None,
            )
            .try_collect::<Vec<_>>()
            .await
            .map_err(|e| format!("failed to pull {}: {}", PROXY_IMAGE, e))?;

        let _ = self.image_built.set(());
        Ok(())
    }

    /// Resolve a subject's bridge IP by inspecting its container.
    async fn bridge_ip(&self, subject: &Subject) -> Result<String, String> {
        let id = subject.id.strip_prefix("docker/").unwrap_or(&subject.id);
        let info = self
            .connection()?
            .inspect_container(id, None::<InspectContainerOptions>)
            .await
            .map_err(|e| format!("inspect failed for {}: {}", id, e))?;
        info.network_settings
            .and_then(|n| n.networks)
            .and_then(|networks| {
                networks
                    .values()
                    .next()
                    .and_then(|ep| ep.ip_address.clone())
            })
            .ok_or_else(|| format!("subject {} has no bridge IP", subject.id))
    }

    /// Run a command inside a proxy container via `docker exec`.
    async fn exec_in_proxy(&self, container_name: &str, cmd: &[&str]) -> Result<(), String> {
        let config = bollard::exec::CreateExecOptions {
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            cmd: Some(cmd.to_vec()),
            ..Default::default()
        };
        let exec = self
            .connection()?
            .create_exec(container_name, config)
            .await
            .map_err(|e| format!("create exec failed: {}", e))?;

        let result = self
            .connection()?
            .start_exec(&exec.id, Some(bollard::exec::StartExecOptions::default()))
            .await
            .map_err(|e| format!("start exec failed: {}", e))?;

        // Drain the output stream so the exec completes.
        if let bollard::exec::StartExecResults::Attached { output, .. } = result {
            use futures_util::TryStreamExt;
            output
                .try_collect::<Vec<_>>()
                .await
                .map_err(|e| format!("exec output failed: {}", e))?;
        }

        let inspect = self
            .connection()?
            .inspect_exec(&exec.id)
            .await
            .map_err(|e| format!("inspect exec failed: {}", e))?;

        let exit = inspect.exit_code.unwrap_or(-1);
        if exit != 0 {
            return Err(format!(
                "exec in {} failed (exit {}): {:?}",
                container_name, exit, cmd
            ));
        }
        Ok(())
    }

    /// Apply the current netem settings stored on a link.
    async fn apply_netem(&self, link: &LinkId) -> Result<(), String> {
        let (container_name, delay_ms, jitter_ms, loss_pct) = {
            let links = self.links.lock().expect("poisoned links lock");
            let inner = links
                .get(link)
                .ok_or_else(|| format!("unknown link {}", link.0))?;
            (
                inner.container_name.clone(),
                inner.delay_ms,
                inner.jitter_ms,
                inner.loss_pct,
            )
        };

        let mut cmd: Vec<&str> = vec!["tc", "qdisc", "replace", "dev", "eth0", "root", "netem"];
        let delay_arg;
        let jitter_arg;
        let loss_arg;

        if let Some(delay) = delay_ms {
            delay_arg = format!("{}ms", delay);
            cmd.push("delay");
            cmd.push(&delay_arg);
            if let Some(jitter) = jitter_ms.filter(|&j| j > 0) {
                jitter_arg = format!("{}ms", jitter);
                cmd.push(&jitter_arg);
            }
        }

        if let Some(pct) = loss_pct {
            loss_arg = format!("{}%", (pct * 100.0) as u32);
            cmd.push("loss");
            cmd.push(&loss_arg);
        }

        if delay_ms.is_none() && loss_pct.is_none() {
            return Ok(());
        }

        self.exec_in_proxy(&container_name, &cmd).await?;
        debug!("link {} netem: {:?}", link.0, cmd);
        Ok(())
    }
}

impl NetworkControl for DockerNetwork {
    fn set_seed(&self, seed: u64) {
        let mut guard = self.seed.lock().expect("poisoned seed lock");
        if guard.is_none() {
            *guard = Some(seed);
            debug!("network seed set to {}", seed);
        }
    }

    fn link<'a>(
        &'a self,
        a: &'a Subject,
        b: &'a Subject,
        port: u16,
    ) -> Pin<Box<dyn Future<Output = Result<LinkId, String>> + Send + 'a>> {
        Box::pin(async move {
            self.ensure_image().await?;

            let target_ip = self.bridge_ip(b).await?;
            let target = format!("{}:{}", target_ip, port);

            let id = self.next_link_id();
            let container_name = format!("dstest-proxy-{}", id.0);

            let cmd = vec![
                "sh".to_string(),
                "-c".to_string(),
                format!(
                    "apk add --no-cache socat iproute2 iptables && exec socat TCP-LISTEN:{},fork,reuseaddr TCP:{}",
                    port, target
                ),
            ];

            let config = ContainerCreateBody {
                image: Some(PROXY_IMAGE.to_string()),
                cmd: Some(cmd),
                host_config: Some(HostConfig {
                    cap_add: Some(vec!["NET_ADMIN".to_string()]),
                    ..Default::default()
                }),
                ..Default::default()
            };

            let container = self
                .connection()?
                .create_container(
                    Some(CreateContainerOptions {
                        name: Some(container_name.clone()),
                        ..Default::default()
                    }),
                    config,
                )
                .await
                .map_err(|e| format!("failed to create proxy container: {}", e))?;

            self.connection()?
                .start_container(&container.id, None::<StartContainerOptions>)
                .await
                .map_err(|e| format!("failed to start proxy container: {}", e))?;

            // Get the proxy container's bridge IP.
            let info = self
                .connection()?
                .inspect_container(&container.id, None::<InspectContainerOptions>)
                .await
                .map_err(|e| format!("failed to inspect proxy container: {}", e))?;

            let proxy_ip = info
                .network_settings
                .and_then(|n| n.networks)
                .and_then(|networks| {
                    networks
                        .values()
                        .next()
                        .and_then(|ep| ep.ip_address.clone())
                })
                .ok_or_else(|| "proxy container has no bridge IP".to_string())?;

            let addr = format!("{}:{}", proxy_ip, port);
            info!(
                "network link {} created: {} -> {} via {}",
                id.0, a.id, target, addr
            );

            // Wait for socat to be ready (it installs packages on startup). Bound
            // each connection attempt so an unreachable proxy bridge IP (e.g. on a
            // podman/docker machine VM where the host can't dial container IPs)
            // cannot hang the whole experiment.
            let ready_addr = format!("{}:{}", proxy_ip, port);
            let mut ready = false;
            for attempt in 0..10 {
                let connected = tokio::time::timeout(
                    Duration::from_millis(500),
                    tokio::net::TcpStream::connect(&ready_addr),
                )
                .await
                .is_ok_and(|r| r.is_ok());
                if connected {
                    debug!("proxy ready after {} attempts", attempt + 1);
                    ready = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            if !ready {
                return Err(format!(
                    "proxy container {} never became reachable at {} (socat setup failed)",
                    container_name, ready_addr
                ));
            }

            self.links.lock().expect("poisoned links lock").insert(
                id.clone(),
                LinkInner {
                    container_name,
                    addr,
                    delay_ms: None,
                    jitter_ms: None,
                    loss_pct: None,
                },
            );

            Ok(id)
        })
    }

    fn link_addr<'a>(
        &'a self,
        link: &'a LinkId,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(async move {
            let links = self.links.lock().expect("poisoned links lock");
            let inner = links
                .get(link)
                .ok_or_else(|| format!("unknown link {}", link.0))?;
            Ok(inner.addr.clone())
        })
    }

    fn set_latency<'a>(
        &'a self,
        link: &'a LinkId,
        delay_ms: u64,
        jitter_ms: u64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            {
                let mut links = self.links.lock().expect("poisoned links lock");
                let inner = links
                    .get_mut(link)
                    .ok_or_else(|| format!("unknown link {}", link.0))?;
                inner.delay_ms = Some(delay_ms);
                inner.jitter_ms = if jitter_ms > 0 { Some(jitter_ms) } else { None };
            }
            self.apply_netem(link).await
        })
    }

    fn set_loss<'a>(
        &'a self,
        link: &'a LinkId,
        pct: f64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            if !(0.0..=1.0).contains(&pct) {
                return Err(format!("loss must be 0.0–1.0, got {}", pct));
            }
            {
                let mut links = self.links.lock().expect("poisoned links lock");
                let inner = links
                    .get_mut(link)
                    .ok_or_else(|| format!("unknown link {}", link.0))?;
                inner.loss_pct = Some(pct);
            }
            self.apply_netem(link).await
        })
    }

    fn partition<'a>(
        &'a self,
        link: &'a LinkId,
        _direction: Direction,
        mode: PartitionMode,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let container_name = {
                let links = self.links.lock().expect("poisoned links lock");
                links
                    .get(link)
                    .ok_or_else(|| format!("unknown link {}", link.0))?
                    .container_name
                    .clone()
            };

            match mode {
                PartitionMode::Blackhole => {
                    // Drop all outbound traffic (peers see timeouts).
                    self.exec_in_proxy(
                        &container_name,
                        &["iptables", "-A", "OUTPUT", "-j", "DROP"],
                    )
                    .await?;
                }
                PartitionMode::Reset => {
                    // Reject all outbound TCP with RST.
                    self.exec_in_proxy(
                        &container_name,
                        &[
                            "iptables",
                            "-A",
                            "OUTPUT",
                            "-p",
                            "tcp",
                            "-j",
                            "REJECT",
                            "--reject-with",
                            "tcp-reset",
                        ],
                    )
                    .await?;
                }
            }
            info!("link {} partitioned: {:?}", link.0, mode);
            Ok(())
        })
    }

    fn heal<'a>(
        &'a self,
        link: &'a LinkId,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            let container_name = {
                let links = self.links.lock().expect("poisoned links lock");
                links
                    .get(link)
                    .ok_or_else(|| format!("unknown link {}", link.0))?
                    .container_name
                    .clone()
            };

            // Remove iptables OUTPUT DROP (blackhole) — errors if absent.
            self.exec_in_proxy(&container_name, &["iptables", "-D", "OUTPUT", "-j", "DROP"])
                .await
                .ok();

            // Remove iptables OUTPUT tcp-reset (reset partition) — errors if absent.
            self.exec_in_proxy(
                &container_name,
                &[
                    "iptables",
                    "-D",
                    "OUTPUT",
                    "-p",
                    "tcp",
                    "-j",
                    "REJECT",
                    "--reject-with",
                    "tcp-reset",
                ],
            )
            .await
            .ok();

            // Remove tc qdisc (latency/loss) — errors if absent.
            self.exec_in_proxy(
                &container_name,
                &["tc", "qdisc", "del", "dev", "eth0", "root"],
            )
            .await
            .ok();

            {
                let mut links = self.links.lock().expect("poisoned links lock");
                if let Some(inner) = links.get_mut(link) {
                    inner.delay_ms = None;
                    inner.jitter_ms = None;
                    inner.loss_pct = None;
                }
            }

            info!("link {} healed", link.0);
            Ok(())
        })
    }
}
