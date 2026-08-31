//! Deterministic network link control between subjects.
//!
//! A substrate implementing `NetworkControl` can establish controllable links
//! between subjects and impair them deterministically: latency, jitter, loss,
//! and asymmetric partitions. All impairment randomness must derive from the
//! experiment seed so that a seed fully determines the network's behaviour.

use std::future::Future;
use std::pin::Pin;

use crate::domain::subject::Subject;

pub const NOT_SUPPORTED: &str = "network control not supported by this substrate";

/// Opaque identifier for an established link, owned by the substrate.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LinkId(pub String);

/// Direction of a directional impairment or partition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Only traffic from the first subject to the second.
    AToB,
    /// Only traffic from the second subject to the first.
    BToA,
    /// Both directions.
    Both,
}

impl std::str::FromStr for Direction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().replace('_', "-").as_str() {
            "a-to-b" | "a->b" | "ab" => Ok(Direction::AToB),
            "b-to-a" | "b->a" | "ba" => Ok(Direction::BToA),
            "both" | "bidirectional" => Ok(Direction::Both),
            other => Err(format!(
                "unknown direction '{}' (expected \"a->b\", \"b->a\", or \"both\")",
                other
            )),
        }
    }
}

/// How a partition manifests on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionMode {
    /// Silently drop traffic (peers see timeouts).
    Blackhole,
    /// Actively reset connections (peers see ECONNREFUSED/RST).
    Reset,
}

impl std::str::FromStr for PartitionMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "blackhole" | "drop" => Ok(PartitionMode::Blackhole),
            "reset" | "rst" | "reject" => Ok(PartitionMode::Reset),
            other => Err(format!(
                "unknown partition mode '{}' (expected \"blackhole\" or \"reset\")",
                other
            )),
        }
    }
}

/// Per-link network impairment control. Implementations must be
/// deterministic given the experiment seed.
pub trait NetworkControl: Send + Sync + 'static {
    /// Set the root seed for impairment randomness (jitter, loss sampling).
    /// Called by the engine when a config with a seed is registered. The
    /// default is a no-op; substrates that support network control should
    /// store it and derive per-link seeds from it.
    fn set_seed(&self, _seed: u64) {}

    /// Establish a controllable link from subject `a` to subject `b` on the
    /// given service port. Returns the link identifier.
    fn link<'a>(
        &'a self,
        _a: &'a Subject,
        _b: &'a Subject,
        _port: u16,
    ) -> Pin<Box<dyn Future<Output = Result<LinkId, String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// The address subject `a` should dial to reach `b` over this link
    /// (e.g. `"host.docker.internal:32768"`). Substrates without network
    /// control return an error.
    fn link_addr<'a>(
        &'a self,
        _link: &'a LinkId,
    ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Impose constant delay plus uniform jitter on a link.
    fn set_latency<'a>(
        &'a self,
        _link: &'a LinkId,
        _delay_ms: u64,
        _jitter_ms: u64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Impose probabilistic traffic loss (0.0–1.0) on a link.
    fn set_loss<'a>(
        &'a self,
        _link: &'a LinkId,
        _pct: f64,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Partition a link in the given direction.
    fn partition<'a>(
        &'a self,
        _link: &'a LinkId,
        _direction: Direction,
        _mode: PartitionMode,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }

    /// Remove all impairments from a link.
    fn heal<'a>(
        &'a self,
        _link: &'a LinkId,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move { Err(NOT_SUPPORTED.to_string()) })
    }
}
