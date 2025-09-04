//! Synchronous Channel - typically used by a Target.

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use crate::channel::{ChannelActor, ChannelCb, ChannelFlags};
use crate::channel::{check_base_addr, check_channel_size, consumer_only, producer_only};
use crate::{Error, Result};

/// Trait for accessing channel in a shared medium (usually RAM).
///
/// Sync version, typically used for direct RAM access and other synchronous
/// operations.
pub trait ChannelIo {
    /// Atomic read u32 operation
    fn read_u32(&mut self, addr: u32) -> Result<u32>;

    /// Atomic write u32 operation
    fn write_u32(&mut self, addr: u32, value: u32) -> Result<()>;

    /// Bulk read access, no need for atomicity
    fn read_bulk(&mut self, addr: u32, buf: &mut [u32]) -> Result<()>;

    /// Bulk write access, no need for atomicity
    fn write_bulk(&mut self, addr: u32, data: &[u32]) -> Result<()>;
}

/// Synchronous unidirectional communication channel
pub struct Channel<'a, I: ChannelIo> {
    io: &'a mut I,
    actor: ChannelActor,
    base_addr: u32,
}

impl<'a, I: ChannelIo> Channel<'a, I> {
    /// Create new channel with given size.  Used by the Target to initialize
    /// the channel.
    ///
    /// Arguments:
    /// - `io` - Object implementing [`ChannelIo`] trait to access shared
    ///   medium
    /// - `actor` - Whether the user is a Consumer or Producer
    /// - `base_addr` - Base address of the channel on that medium
    /// - `size` - Total size of the channel in bytes, including Control Block
    ///   and data portions.
    pub fn new(io: &'a mut I, actor: ChannelActor, base_addr: u32, size: usize) -> Result<Self> {
        check_base_addr(base_addr)?;
        check_channel_size(size)?;

        let mut channel = Self {
            io,
            base_addr,
            actor,
        };

        // Set channel size to 0 first.  Channel is only valid once size is non-zero.
        channel.write_channel_size(0)?;

        // Initialize control block
        channel.write_producer_seq(0)?;
        channel.write_consumer_seq(0)?;
        channel.write_flags(ChannelFlags::Ok)?;
        channel.write_data_size(0)?;

        // Final step is to set the channel size
        channel.write_channel_size(size)?;

        debug!("Created channel {actor:?} at {base_addr:#010X} size {size} bytes");

        Ok(channel)
    }

    /// Connect to existing channel.  Used by the Host to connect to the
    /// Target's channel.
    ///
    /// Arguments:
    /// - `io` - Object implementing [`ChannelIo`] trait to access shared
    ///   medium
    /// - `actor` - Whether the user is a Consumer or Producer
    /// - `base_addr` - Base address of the channel on that medium
    pub fn from_target(io: &'a mut I, actor: ChannelActor, base_addr: u32) -> Result<Self> {
        check_base_addr(base_addr)?;

        let mut channel = Self {
            io,
            actor,
            base_addr,
        };

        // Validate existing control block
        let channel_size = channel.read_channel_size()?;
        check_channel_size(channel_size)?;
        if channel_size > 0 {
            debug!("Created channel {actor:?} at {base_addr:#010X} size {channel_size} bytes");
            Ok(channel)
        } else {
            Err(Error::Uninit)
        }
    }

    /// Producer: Atomically publish word-aligned data.
    ///
    /// Alternatively use [`Self::publish_bytes()`], which makes no
    /// assumptions about aligment or data length.
    pub fn publish_data(&mut self, data: &[u32]) -> Result<()> {
        producer_only(self.actor)?;

        let byte_len = data.len() * 4;
        if byte_len > self.data_capacity()? {
            return Err(Error::PayloadTooLarge);
        }

        // Check availability
        self.check_idle()?;

        // Write data payload first (bulk)
        let data_addr = self.data_start_addr();
        self.write_bulk(data_addr, data)?;

        // Write metadata before publishing
        self.write_data_size(byte_len)?;
        self.write_flags(ChannelFlags::Ok)?;

        // Atomically publish by incrementing producer_seq last
        self.inc_producer_seq()?;

        Ok(())
    }

    /// Producer: Atomically publish byte data - handles byte data which is
    /// potentially unaligned and/or not a multiple of word-length.
    ///
    /// This is less efficient than [`Self::publish_data()`] where the data is
    /// guaranteed word aligned.
    pub fn publish_bytes(&mut self, data: &[u8]) -> Result<()> {
        producer_only(self.actor)?;

        if data.len() > self.data_capacity()? {
            return Err(Error::PayloadTooLarge);
        }

        // Check availability
        self.check_idle()?;

        let data_addr = self.data_start_addr();

        // Write aligned portion with individual writes (convert bytes to words)
        let word_count = data.len() / 4;
        for word_idx in 0..word_count {
            let byte_offset = word_idx * 4;
            let word = u32::from_le_bytes([
                data[byte_offset],
                data[byte_offset + 1],
                data[byte_offset + 2],
                data[byte_offset + 3],
            ]);
            self.write_u32(data_addr + (word_idx as u32 * 4), word)?;
        }

        // Handle remaining 1-3 bytes
        let remaining = data.len() % 4;
        if remaining > 0 {
            let mut final_word = 0u32;
            let base_offset = word_count * 4;
            for i in 0..remaining {
                final_word |= (data[base_offset + i] as u32) << (i * 8);
            }
            self.write_u32(data_addr + (base_offset as u32), final_word)?;
        }

        // Write metadata before publishing
        self.write_data_size(data.len())?;
        self.write_flags(ChannelFlags::Ok)?;

        // Atomically publish by incrementing producer_seq last
        self.inc_producer_seq()?;

        Ok(())
    }

    /// Producer: Check if channel is available for publishing.
    pub fn can_publish(&mut self) -> Result<bool> {
        self.idle()
    }

    /// Consumer: Atomically consume data as bytes.
    ///
    /// Less efficient than [`Self::consume_data()`], but handles numbers of
    /// bytes that aren't multiples of word-length.
    ///
    /// Returns the number of bytes consumed.
    pub fn consume_bytes(&mut self, buf: &mut [u8]) -> Result<usize> {
        consumer_only(self.actor)?;

        self.check_busy()?;

        // Read data size
        let data_size = self.read_data_size()?;
        if data_size > buf.len() {
            return Err(Error::BufferTooSmall);
        }
        if data_size > self.data_capacity()? {
            return Err(Error::PayloadTooLarge);
        }

        let data_addr = self.data_start_addr();

        // Read aligned portion with individual u32 reads (convert to bytes)
        let word_count = data_size / 4;
        for word_idx in 0..word_count {
            let word = self.read_u32(data_addr + (word_idx as u32 * 4))?;
            let bytes = word.to_le_bytes();
            let base_offset = word_idx * 4;
            buf[base_offset..base_offset + 4].copy_from_slice(&bytes);
        }

        // Handle remaining 1-3 bytes
        let remaining = data_size % 4;
        if remaining > 0 {
            let final_word = self.read_u32(data_addr + (word_count as u32 * 4))?;
            let bytes = final_word.to_le_bytes();
            let base_offset = word_count * 4;
            buf[base_offset..base_offset + remaining].copy_from_slice(&bytes[..remaining]);
        }

        // Atomically consume by updating consumer_seq last
        self.set_consumer_seq_to_producer()?;

        Ok(data_size)
    }

    /// Consumer: Atomically consume data as words
    ///
    /// More efficient than [`Self::consume_bytes`], but only handles word
    /// length data (although it pads the last u32 read if required).
    ///
    /// Returns the number of full words written.
    pub fn consume_data(&mut self, buf: &mut [u32]) -> Result<usize> {
        consumer_only(self.actor)?;

        self.check_busy()?;

        // Read data size in bytes
        let byte_size = self.read_data_size()?;
        let word_size = byte_size.div_ceil(4); // Round up to words

        if word_size > buf.len() {
            return Err(Error::BufferTooSmall);
        }
        if byte_size > self.data_capacity()? {
            return Err(Error::PayloadTooLarge);
        }

        // Read data payload (bulk read)
        let data_addr = self.data_start_addr();
        self.read_bulk(data_addr, &mut buf[..word_size])?;

        // Atomically consume by updating consumer_seq last
        self.set_consumer_seq_to_producer()?;

        Ok(word_size)
    }

    /// Consumer: Check available data size in bytes.  Use to both check if
    /// there is data available to be read, and also how much.
    pub fn data_available(&mut self) -> Result<Option<usize>> {
        if !self.idle()? {
            let data_size = self.read_data_size()?;
            Ok(Some(data_size))
        } else {
            Ok(None)
        }
    }

    /// Get data capacity for this channel
    pub fn data_capacity(&mut self) -> Result<usize> {
        let channel_size =
            self.io
                .read_u32(self.base_addr + ChannelCb::channel_size_offset())? as usize;
        Ok(channel_size - (ChannelCb::data_offset() as usize))
    }
}

// Internal functions
impl<I: ChannelIo> Channel<'_, I> {
    fn write_channel_size(&mut self, size: usize) -> Result<()> {
        self.io.write_u32(
            self.base_addr + ChannelCb::channel_size_offset(),
            size as u32,
        )
    }

    fn write_producer_seq(&mut self, seq: u32) -> Result<()> {
        self.io
            .write_u32(self.base_addr + ChannelCb::producer_seq_offset(), seq)
    }

    fn write_consumer_seq(&mut self, seq: u32) -> Result<()> {
        self.io
            .write_u32(self.base_addr + ChannelCb::consumer_seq_offset(), seq)
    }

    fn write_flags(&mut self, flags: ChannelFlags) -> Result<()> {
        self.io
            .write_u32(self.base_addr + ChannelCb::flags_offset(), flags as u32)
    }

    fn write_data_size(&mut self, size: usize) -> Result<()> {
        self.io
            .write_u32(self.base_addr + ChannelCb::data_size_offset(), size as u32)
    }

    fn read_channel_size(&mut self) -> Result<usize> {
        let channel_size =
            self.io
                .read_u32(self.base_addr + ChannelCb::channel_size_offset())? as usize;
        Ok(channel_size)
    }

    fn read_producer_seq(&mut self) -> Result<u32> {
        self.io
            .read_u32(self.base_addr + ChannelCb::producer_seq_offset())
    }

    fn read_consumer_seq(&mut self) -> Result<u32> {
        self.io
            .read_u32(self.base_addr + ChannelCb::consumer_seq_offset())
    }

    #[allow(dead_code)]
    fn read_flags(&mut self) -> Result<ChannelFlags> {
        let flags = self
            .io
            .read_u32(self.base_addr + ChannelCb::flags_offset())?;
        Ok(ChannelFlags::from(flags))
    }

    fn read_data_size(&mut self) -> Result<usize> {
        let data_size =
            self.io
                .read_u32(self.base_addr + ChannelCb::data_size_offset())? as usize;
        Ok(data_size)
    }

    fn data_start_addr(&mut self) -> u32 {
        self.base_addr + ChannelCb::data_offset()
    }

    fn write_bulk(&mut self, addr: u32, data: &[u32]) -> Result<()> {
        self.io.write_bulk(addr, data)
    }

    fn read_bulk(&mut self, addr: u32, buf: &mut [u32]) -> Result<()> {
        self.io.read_bulk(addr, buf)
    }

    fn write_u32(&mut self, addr: u32, value: u32) -> Result<()> {
        self.io.write_u32(addr, value)
    }

    fn read_u32(&mut self, addr: u32) -> Result<u32> {
        self.io.read_u32(addr)
    }

    fn idle(&mut self) -> Result<bool> {
        let producer_seq = self.read_producer_seq()?;
        let consumer_seq = self.read_consumer_seq()?;
        Ok(producer_seq == consumer_seq)
    }

    fn check_idle(&mut self) -> Result<()> {
        if self.idle()? {
            Ok(())
        } else {
            Err(Error::Busy)
        }
    }

    fn check_busy(&mut self) -> Result<()> {
        if !self.idle()? {
            Ok(())
        } else {
            Err(Error::NoData)
        }
    }

    fn inc_producer_seq(&mut self) -> Result<()> {
        let producer_seq = self.read_producer_seq()?;
        self.write_producer_seq(producer_seq.wrapping_add(1))
    }

    fn set_consumer_seq_to_producer(&mut self) -> Result<()> {
        let producer_seq = self.read_producer_seq()?;
        self.write_consumer_seq(producer_seq)
    }
}

/// RAM channel type.  Typically used by a Target.
pub type RamChannel = Channel<'static, RamChannelIo>;

/// Channel I/O implementation using direct RAM access
#[derive(Clone, Copy)]
pub struct RamChannelIo;

impl RamChannelIo {
    /// Create a new RamChannelIo instances.
    ///
    /// ```rust
    /// static mut RAM_CHANNEL_IO: RamChannelIo = RamChannelIo::new();
    /// // Now use it in RamChannel::new()
    /// ```
    // We need a new() rather than a default() as it must be const.
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self {}
    }
}

impl ChannelIo for RamChannelIo {
    fn read_u32(&mut self, addr: u32) -> Result<u32> {
        Ok(unsafe { core::ptr::read_volatile(addr as *const u32) })
    }

    fn write_u32(&mut self, addr: u32, value: u32) -> Result<()> {
        unsafe { core::ptr::write_volatile(addr as *mut u32, value) };
        Ok(())
    }

    fn read_bulk(&mut self, addr: u32, buf: &mut [u32]) -> Result<()> {
        for (i, word) in buf.iter_mut().enumerate() {
            *word = self.read_u32(addr + (i as u32 * 4))?;
        }
        Ok(())
    }

    fn write_bulk(&mut self, addr: u32, data: &[u32]) -> Result<()> {
        for (i, word) in data.iter().enumerate() {
            self.write_u32(addr + (i as u32 * 4), *word)?;
        }
        Ok(())
    }
}
