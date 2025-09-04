//! Asynchronous Client - typically used by a Host.

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use alloc::vec;
use alloc::vec::Vec;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use crate::channel::{ChannelActor, ReaderWriterChannel, ReaderWriterChannelIo};
use crate::client::{ChannelConfig, RpcClientConfig};
use crate::io::{Reader, Writer};

/// Yield delay for async polling loops.
///
/// Application must provide an implementation of this trait in order for the
/// async client to be able to yield, waiting for a response from the other
/// side of the channel.
///
/// This trait keeps `airfrog-rpc` free of any specific async runtime.
///
/// Example:
///
/// ```rust
/// use embassy_time::{Duration, Timer};
/// struct Delay;
/// impl AsyncDelay for Delay {
///     async fn delay() {
///         Timer::after(Duration::from_millis(50)).await;
///     }
/// }
/// ```
pub trait AsyncDelay {
    fn delay() -> impl Future<Output = ()>;
}

/// Async RPC Client for dual-channel command/response communication.
///
/// See [`AsyncDelay`] for required delay trait.
///
/// Example usage:
///
/// ```rust
/// use airfrog_rpc::client::{AsyncDelay, AsyncRpcClient, RpcClientConfig};
/// use airfrog_rpc::io::{Reader, Writer};
///
/// let config = RpcClientConfig::FromTarget {
///     cmd_ch_ptr: 0x2000_0000,
///     rsp_ch_ptr: 0x2000_1000,
/// };
/// let mut reader = ...; // implement Reader trait
/// let mut writer = ...; // implement Writer trait
/// let mut client = AsyncRpcClient::<_, _, Delay>::new(&mut reader, &mut writer, config);
/// let command = [0x01, 0x02, 0x03, 0x04];
/// let response = client.request(&command).await?;
/// // Process response...
/// ```
pub struct AsyncRpcClient<'a, R: Reader, W: Writer, D: AsyncDelay> {
    io: ReaderWriterChannelIo<'a, R, W>,
    cmd_ch_config: ChannelConfig,
    rsp_ch_config: ChannelConfig,
    _delay: core::marker::PhantomData<D>,
}

impl<'a, R: Reader, W: Writer, D: AsyncDelay> AsyncRpcClient<'a, R, W, D> {
    /// Create a new AsyncRpcClient
    ///
    /// Arguments:
    /// - `reader`: Reader object to read from target
    /// - `writer`: Writer object to write to target
    /// - `config`: Configuration for creating the client
    pub fn new(reader: &'a mut R, writer: &'a mut W, config: RpcClientConfig) -> Self {
        let (cmd_ch_config, rsp_ch_config) = Self::get_channel_configs(config);

        Self {
            io: ReaderWriterChannelIo::new(reader, writer),
            cmd_ch_config,
            rsp_ch_config,
            _delay: core::marker::PhantomData,
        }
    }

    /// Perform an RPC request by sending a command and waiting for a response
    ///
    /// The format of the command and response data is application-specific.
    ///
    /// Arguments:
    /// - `command`: Command data to send to target
    ///
    /// Returns:
    /// - `Ok(response_data)`: Response data received from target
    /// - `Err(error)`: Error occurred during request
    pub async fn request(&mut self, command: &[u8]) -> Result<Vec<u8>, crate::Error> {
        debug!("Starting RPC request ({} bytes)", command.len());

        // Send command phase - create channel, send, drop channel
        let mut cmd_ch = self.cmd_channel().await?;
        cmd_ch.publish_bytes(command).await?;
        debug!("Command sent to target");

        // Receive response phase - create channel, wait, read, drop channel
        let mut rsp_ch = self.rsp_channel().await?;

        // Wait for response with polling
        let response_size = loop {
            if let Some(size) = rsp_ch.data_available().await? {
                debug!("Response available ({} bytes)", size);
                break size;
            }

            // Yield with reasonable delay to avoid spinning too fast
            D::delay().await;
        };

        // Read the response data
        let mut response_buf = vec![0u8; response_size];
        let received_size = rsp_ch.consume_bytes(&mut response_buf).await?;

        if received_size != response_size {
            warn!(
                "Expected {} bytes, received {} bytes",
                response_size, received_size
            );
            response_buf.truncate(received_size);
        }

        debug!("RPC request completed ({} bytes received)", received_size);
        Ok(response_buf)
    }

    fn get_channel_configs(config: RpcClientConfig) -> (ChannelConfig, ChannelConfig) {
        match config {
            RpcClientConfig::Direct {
                cmd_ch_ptr,
                cmd_ch_size,
                rsp_ch_ptr,
                rsp_ch_size,
            } => (
                ChannelConfig::Direct {
                    ptr: cmd_ch_ptr,
                    size: cmd_ch_size,
                },
                ChannelConfig::Direct {
                    ptr: rsp_ch_ptr,
                    size: rsp_ch_size,
                },
            ),
            RpcClientConfig::FromTarget {
                cmd_ch_ptr,
                rsp_ch_ptr,
            } => (
                ChannelConfig::FromTarget { ptr: cmd_ch_ptr },
                ChannelConfig::FromTarget { ptr: rsp_ch_ptr },
            ),
        }
    }

    async fn cmd_channel<'method>(
        &'method mut self,
    ) -> Result<ReaderWriterChannel<'method, 'a, R, W>, crate::Error> {
        match self.cmd_ch_config {
            ChannelConfig::Direct { ptr, size } => {
                ReaderWriterChannel::new(&mut self.io, ChannelActor::Producer, ptr, size).await
            }
            ChannelConfig::FromTarget { ptr } => {
                ReaderWriterChannel::from_target(&mut self.io, ChannelActor::Producer, ptr).await
            }
        }
    }

    async fn rsp_channel<'method>(
        &'method mut self,
    ) -> Result<ReaderWriterChannel<'method, 'a, R, W>, crate::Error> {
        match self.rsp_ch_config {
            ChannelConfig::Direct { ptr, size } => {
                ReaderWriterChannel::new(&mut self.io, ChannelActor::Consumer, ptr, size).await
            }
            ChannelConfig::FromTarget { ptr } => {
                ReaderWriterChannel::from_target(&mut self.io, ChannelActor::Consumer, ptr).await
            }
        }
    }
}
