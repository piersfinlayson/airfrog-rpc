//! Channel for RPC communication between Host and Target over SWD and similar protocols
//!
//! See [`crate`] for a description of how to use these objects.

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#[cfg(feature = "async")]
pub mod futures;
pub mod sync;

#[cfg(feature = "async")]
pub use futures::{AsyncChannel, AsyncChannelIo, ReaderWriterChannel, ReaderWriterChannelIo};
pub use sync::{Channel, ChannelIo, RamChannel, RamChannelIo};

use crate::{Error, Result};

/// Whether the user of this Channel is a Producer or Consumer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelActor {
    Producer,
    Consumer,
}

/// Control block for a unidirectional channel.  Used from controller to
/// target, or vice versa.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ChannelCb {
    /// Total size associated with this channel, including this control block
    pub channel_size: u32,

    /// Producer sequence number - incremented when data is written
    pub producer_seq: u32,

    /// Consumer sequence number - incremented when data is consumed
    pub consumer_seq: u32,

    /// Status flags - currently unused
    pub flags: ChannelFlags,

    /// Size of data payload in bytes
    pub data_size: u32,
}

/// ChannelCb offsets
impl ChannelCb {
    #[allow(clippy::new_without_default)]
    pub fn new(size: u32) -> Self {
        Self {
            channel_size: size,
            producer_seq: 0,
            consumer_seq: 0,
            flags: ChannelFlags::default(),
            data_size: 0,
        }
    }

    pub const fn channel_size_offset() -> u32 {
        core::mem::offset_of!(ChannelCb, channel_size) as u32
    }

    pub const fn producer_seq_offset() -> u32 {
        core::mem::offset_of!(ChannelCb, producer_seq) as u32
    }

    pub const fn consumer_seq_offset() -> u32 {
        core::mem::offset_of!(ChannelCb, consumer_seq) as u32
    }

    pub const fn flags_offset() -> u32 {
        core::mem::offset_of!(ChannelCb, flags) as u32
    }

    pub const fn data_size_offset() -> u32 {
        core::mem::offset_of!(ChannelCb, data_size) as u32
    }

    pub const fn data_offset() -> u32 {
        core::mem::size_of::<Self>() as u32
    }

    pub fn data_capacity(&self) -> usize {
        self.channel_size as usize - core::mem::size_of::<ChannelCb>()
    }

    pub fn data_address(&self, base: u32) -> u32 {
        base + Self::data_offset()
    }
}

/// Channel status flags
#[repr(u32)]
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum ChannelFlags {
    #[default]
    Ok = 0,
    Busy = 1,
    Error = 2,
    Timeout = 3,
}

impl From<u32> for ChannelFlags {
    fn from(value: u32) -> Self {
        match value {
            0 => ChannelFlags::Ok,
            1 => ChannelFlags::Busy,
            2 => ChannelFlags::Error,
            3 => ChannelFlags::Timeout,
            _ => ChannelFlags::Error,
        }
    }
}

// Helper functions

const fn min_channel_size() -> usize {
    ChannelCb::data_offset() as usize + 4
}

fn check_base_addr(addr: u32) -> Result<()> {
    if addr % 4 != 0 {
        Err(Error::NotAligned)
    } else {
        Ok(())
    }
}

fn check_channel_size(size: usize) -> Result<()> {
    if size < min_channel_size() {
        Err(Error::BufferTooSmall)
    } else {
        Ok(())
    }
}

fn consumer_only(actor: ChannelActor) -> Result<()> {
    if actor != ChannelActor::Consumer {
        Err(Error::InvalidOperation)
    } else {
        Ok(())
    }
}

fn producer_only(actor: ChannelActor) -> Result<()> {
    if actor != ChannelActor::Producer {
        Err(Error::InvalidOperation)
    } else {
        Ok(())
    }
}
