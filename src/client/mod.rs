//! Client for RPC communication between Host and Target using two channels:
//! - Command channel: Host writes commands, Target reads
//! - Response channel: Target writes responses, Host reads
//!
//! See [`AsyncRpcClient`] for async client usage, for example on a Host.

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#[cfg(feature = "async")]
pub mod futures;

#[cfg(feature = "async")]
pub use futures::{AsyncDelay, AsyncRpcClient};

/// Configuration for creating an RPC Client.
/// - `Direct`: Create channels with explicit sizes
/// - `FromTarget`: Create channels by reading sizes from target memory,
///   normally used by Hosts.
#[derive(Debug)]
pub enum RpcClientConfig {
    Direct {
        /// Pointer to command channel in target memory
        cmd_ch_ptr: u32,
        /// Size of command channel in bytes
        cmd_ch_size: usize,
        /// Pointer to response channel in target memory
        rsp_ch_ptr: u32,
        /// Size of response channel in bytes
        rsp_ch_size: usize,
    },
    FromTarget {
        /// Pointer to command channel in target memory
        cmd_ch_ptr: u32,
        /// Pointer to response channel in target memory
        rsp_ch_ptr: u32,
    },
}

#[cfg(feature = "async")]
/// Configuration for how to create a channel
#[derive(Debug, Clone)]
enum ChannelConfig {
    /// Create channel with explicit size
    Direct { ptr: u32, size: usize },
    /// Create channel by reading size from target
    FromTarget { ptr: u32 },
}
