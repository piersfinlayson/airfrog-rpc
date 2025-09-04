//! Asynchronous Channel - typically used by a Host.

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

use alloc::boxed::Box;
use async_trait::async_trait;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

use crate::channel::{ChannelActor, ChannelCb, ChannelFlags};
use crate::channel::{check_base_addr, check_channel_size, consumer_only, producer_only};
use crate::io::{Reader, Writer};
use crate::{Error, Result};

/// Trait for accessing the channel in a shared medium (usually RAM).
///
/// Async version, typically used for accessing the medium over SWD or
/// other asynchronous protocols.
#[async_trait(?Send)]
pub trait AsyncChannelIo {
    ///  Atomic read u32 operation
    async fn read_u32(&mut self, addr: u32) -> Result<u32>;

    /// Aotmic write u32 operation
    async fn write_u32(&mut self, addr: u32, value: u32) -> Result<()>;

    /// Bulk read access, no need for atomicity
    async fn read_bulk(&mut self, addr: u32, buf: &mut [u32]) -> Result<()>;

    /// Bulk write access, no need for atomicity
    async fn write_bulk(&mut self, addr: u32, data: &[u32]) -> Result<()>;
}

/// Asynchronous unidirectional communication channel
pub struct AsyncChannel<'a, I: AsyncChannelIo> {
    io: &'a mut I,
    actor: ChannelActor,
    base_addr: u32,
}

impl<'a, I: AsyncChannelIo> AsyncChannel<'a, I> {
    /// Create new channel with given size.  Used by the Target to initialize
    /// the channel.
    ///
    /// Arguments:
    /// - `io` - Object implementing [`AsyncChannelIo`] trait to access shared
    ///   medium
    /// - `actor` - Whether the user is a Consumer or Producer
    /// - `base_addr` - Base address of the channel on that medium
    /// - `size` - Total size of the channel in bytes, including Control Block
    ///   and data portions.
    pub async fn new(
        io: &'a mut I,
        actor: ChannelActor,
        base_addr: u32,
        size: usize,
    ) -> Result<Self> {
        check_base_addr(base_addr)?;
        check_channel_size(size)?;

        let mut channel = Self {
            io,
            base_addr,
            actor,
        };

        // Set channel size to 0 first.  Channel is only valid once size is non-zero.
        channel.write_channel_size(0).await?;

        // Initialize control block
        channel.write_producer_seq(0).await?;
        channel.write_consumer_seq(0).await?;
        channel.write_flags(ChannelFlags::Ok).await?;
        channel.write_data_size(0).await?;

        // Final step is to set the channel size
        channel.write_channel_size(size).await?;

        debug!("Created channel {actor:?} at {base_addr:#010X} size {size} bytes");

        Ok(channel)
    }

    /// Connect to existing channel.  Used by the Host to connect to the
    /// Target's channel.
    ///
    /// Arguments:
    /// - `io` - Object implementing [`AsyncChannelIo`] trait to access shared
    ///   medium
    /// - `actor` - Whether the user is a Consumer or Producer
    /// - `base_addr` - Base address of the channel on that medium
    pub async fn from_target(io: &'a mut I, actor: ChannelActor, base_addr: u32) -> Result<Self> {
        check_base_addr(base_addr)?;

        let mut channel = Self {
            io,
            actor,
            base_addr,
        };

        // Validate existing control block
        let channel_size = channel.read_channel_size().await?;
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
    pub async fn publish_data(&mut self, data: &[u32]) -> Result<()> {
        producer_only(self.actor)?;

        let byte_len = data.len() * 4;
        if byte_len > self.data_capacity().await? {
            return Err(Error::PayloadTooLarge);
        }

        // Check availability
        self.check_idle().await?;

        // Write data payload first (bulk)
        let data_addr = self.data_start_addr();
        self.write_bulk(data_addr, data).await?;

        // Write metadata before publishing
        self.write_data_size(byte_len).await?;
        self.write_flags(ChannelFlags::Ok).await?;

        // Atomically publish by incrementing producer_seq last
        self.inc_producer_seq().await?;

        Ok(())
    }

    /// Producer: Atomically publish byte data - handles byte data which is
    /// potentially unaligned and/or not a multiple of word-length.
    ///
    /// This is less efficient than [`Self::publish_data()`] where the data is
    /// guaranteed word aligned.
    pub async fn publish_bytes(&mut self, data: &[u8]) -> Result<()> {
        producer_only(self.actor)?;

        if data.len() > self.data_capacity().await? {
            return Err(Error::PayloadTooLarge);
        }

        // Check availability
        self.check_idle().await?;

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
            self.write_u32(data_addr + (word_idx as u32 * 4), word)
                .await?;
        }

        // Handle remaining 1-3 bytes
        let remaining = data.len() % 4;
        if remaining > 0 {
            let mut final_word = 0u32;
            let base_offset = word_count * 4;
            for i in 0..remaining {
                final_word |= (data[base_offset + i] as u32) << (i * 8);
            }
            self.write_u32(data_addr + (base_offset as u32), final_word)
                .await?;
        }

        // Write metadata before publishing
        self.write_data_size(data.len()).await?;
        self.write_flags(ChannelFlags::Ok).await?;

        // Atomically publish by incrementing producer_seq last
        self.inc_producer_seq().await?;

        Ok(())
    }

    /// Producer: Check if channel is available for publishing.
    pub async fn can_publish(&mut self) -> Result<bool> {
        self.idle().await
    }

    /// Consumer: Atomically consume data as bytes.
    ///
    /// Less efficient than [`Self::consume_data()`], but handles numbers of
    /// bytes that aren't multiples of word-length.
    pub async fn consume_bytes(&mut self, buf: &mut [u8]) -> Result<usize> {
        consumer_only(self.actor)?;
        self.check_busy().await?;

        // Read data size
        let data_size = self.read_data_size().await?;
        if data_size > buf.len() {
            return Err(Error::BufferTooSmall);
        }
        if data_size > self.data_capacity().await? {
            return Err(Error::PayloadTooLarge);
        }

        let data_addr = self.data_start_addr();

        // Read aligned portion with individual u32 reads (convert to bytes)
        let word_count = data_size / 4;
        for word_idx in 0..word_count {
            let word = self.read_u32(data_addr + (word_idx as u32 * 4)).await?;
            let bytes = word.to_le_bytes();
            let base_offset = word_idx * 4;
            buf[base_offset..base_offset + 4].copy_from_slice(&bytes);
        }

        // Handle remaining 1-3 bytes
        let remaining = data_size % 4;
        if remaining > 0 {
            let final_word = self.read_u32(data_addr + (word_count as u32 * 4)).await?;
            let bytes = final_word.to_le_bytes();
            let base_offset = word_count * 4;
            buf[base_offset..base_offset + remaining].copy_from_slice(&bytes[..remaining]);
        }

        // Atomically consume by updating consumer_seq last
        self.set_consumer_seq_to_producer().await?;

        Ok(data_size)
    }

    /// Consumer: Atomically consume data as words
    ///
    /// More efficient than [`Self::consume_bytes`], but only handles word
    /// length data (although it pads the last u32 read if required).
    pub async fn consume_data(&mut self, buf: &mut [u32]) -> Result<usize> {
        consumer_only(self.actor)?;

        self.check_busy().await?;

        // Read data size in bytes
        let byte_size = self.read_data_size().await?;
        let word_size = byte_size.div_ceil(4); // Round up to words

        if word_size > buf.len() {
            return Err(Error::BufferTooSmall);
        }
        if byte_size > self.data_capacity().await? {
            return Err(Error::PayloadTooLarge);
        }

        // Read data payload (bulk read)
        let data_addr = self.data_start_addr();
        self.read_bulk(data_addr, &mut buf[..word_size]).await?;

        // Atomically consume by updating consumer_seq last
        self.set_consumer_seq_to_producer().await?;

        Ok(word_size)
    }

    /// Consumer: Check available data size.  Use to both check if there is
    /// data available to be read, and also how much.
    pub async fn data_available(&mut self) -> Result<Option<usize>> {
        if !self.idle().await? {
            let data_size = self.read_data_size().await?;
            Ok(Some(data_size))
        } else {
            Ok(None)
        }
    }

    /// Get data capacity for this channel
    pub async fn data_capacity(&mut self) -> Result<usize> {
        let channel_size = self
            .io
            .read_u32(self.base_addr + ChannelCb::channel_size_offset())
            .await? as usize;
        Ok(channel_size - (ChannelCb::data_offset() as usize))
    }
}

// Internal functions
impl<I: AsyncChannelIo> AsyncChannel<'_, I> {
    async fn write_channel_size(&mut self, size: usize) -> Result<()> {
        self.io
            .write_u32(
                self.base_addr + ChannelCb::channel_size_offset(),
                size as u32,
            )
            .await
    }

    async fn write_producer_seq(&mut self, seq: u32) -> Result<()> {
        self.io
            .write_u32(self.base_addr + ChannelCb::producer_seq_offset(), seq)
            .await
    }

    async fn write_consumer_seq(&mut self, seq: u32) -> Result<()> {
        self.io
            .write_u32(self.base_addr + ChannelCb::consumer_seq_offset(), seq)
            .await
    }

    async fn write_flags(&mut self, flags: ChannelFlags) -> Result<()> {
        self.io
            .write_u32(self.base_addr + ChannelCb::flags_offset(), flags as u32)
            .await
    }

    async fn write_data_size(&mut self, size: usize) -> Result<()> {
        self.io
            .write_u32(self.base_addr + ChannelCb::data_size_offset(), size as u32)
            .await
    }

    async fn read_channel_size(&mut self) -> Result<usize> {
        let channel_size = self
            .io
            .read_u32(self.base_addr + ChannelCb::channel_size_offset())
            .await? as usize;
        Ok(channel_size)
    }

    async fn read_producer_seq(&mut self) -> Result<u32> {
        self.io
            .read_u32(self.base_addr + ChannelCb::producer_seq_offset())
            .await
    }

    async fn read_consumer_seq(&mut self) -> Result<u32> {
        self.io
            .read_u32(self.base_addr + ChannelCb::consumer_seq_offset())
            .await
    }

    #[allow(dead_code)]
    async fn read_flags(&mut self) -> Result<ChannelFlags> {
        let flags = self
            .io
            .read_u32(self.base_addr + ChannelCb::flags_offset())
            .await?;
        Ok(ChannelFlags::from(flags))
    }

    async fn read_data_size(&mut self) -> Result<usize> {
        let data_size = self
            .io
            .read_u32(self.base_addr + ChannelCb::data_size_offset())
            .await? as usize;
        Ok(data_size)
    }

    fn data_start_addr(&mut self) -> u32 {
        self.base_addr + ChannelCb::data_offset()
    }

    async fn write_bulk(&mut self, addr: u32, data: &[u32]) -> Result<()> {
        self.io.write_bulk(addr, data).await
    }

    async fn read_bulk(&mut self, addr: u32, buf: &mut [u32]) -> Result<()> {
        self.io.read_bulk(addr, buf).await
    }

    async fn write_u32(&mut self, addr: u32, value: u32) -> Result<()> {
        self.io.write_u32(addr, value).await
    }

    async fn read_u32(&mut self, addr: u32) -> Result<u32> {
        self.io.read_u32(addr).await
    }

    async fn idle(&mut self) -> Result<bool> {
        let producer_seq = self.read_producer_seq().await?;
        let consumer_seq = self.read_consumer_seq().await?;
        Ok(producer_seq == consumer_seq)
    }

    async fn check_idle(&mut self) -> Result<()> {
        if self.idle().await? {
            Ok(())
        } else {
            Err(Error::Busy)
        }
    }

    async fn check_busy(&mut self) -> Result<()> {
        if !self.idle().await? {
            Ok(())
        } else {
            Err(Error::NoData)
        }
    }

    async fn inc_producer_seq(&mut self) -> Result<()> {
        let producer_seq = self.read_producer_seq().await?;
        self.write_producer_seq(producer_seq.wrapping_add(1)).await
    }

    async fn set_consumer_seq_to_producer(&mut self) -> Result<()> {
        let producer_seq = self.read_producer_seq().await?;
        self.write_consumer_seq(producer_seq).await
    }
}

/// Async Reader/Writer channel type.  Typically used by a Host.
// It is important that AsyncChannel and ReaderWriterChannelIo have different
// lifetimes - this allows borrowing of both to be decoupled from each other.
pub type ReaderWriterChannel<'a, 'b, R, W> = AsyncChannel<'a, ReaderWriterChannelIo<'b, R, W>>;

/// Channel I/O implementation using [`crate::io::Reader`] and
/// [`crate::io::Writer`] traits.
pub struct ReaderWriterChannelIo<'a, R: Reader, W: Writer> {
    reader: &'a mut R,
    writer: &'a mut W,
}

impl<'a, R: Reader, W: Writer> ReaderWriterChannelIo<'a, R, W> {
    /// Create new instance
    pub fn new(reader: &'a mut R, writer: &'a mut W) -> Self {
        Self { reader, writer }
    }
}

#[async_trait(?Send)]
impl<R: Reader, W: Writer> AsyncChannelIo for ReaderWriterChannelIo<'_, R, W> {
    async fn read_u32(&mut self, addr: u32) -> Result<u32> {
        let mut buf = [0u8; 4];
        self.reader
            .read(addr, &mut buf)
            .await
            .map_err(|_| Error::Io)?;
        Ok(u32::from_le_bytes(buf))
    }

    async fn write_u32(&mut self, addr: u32, value: u32) -> Result<()> {
        self.writer
            .write(addr, &value.to_le_bytes())
            .await
            .map_err(|_| Error::Io)
    }

    async fn read_bulk(&mut self, addr: u32, buf: &mut [u32]) -> Result<()> {
        let byte_len = buf.len() * 4;
        let byte_addr = addr;
        let byte_buf =
            unsafe { core::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, byte_len) };
        self.reader
            .read(byte_addr, byte_buf)
            .await
            .map_err(|_| Error::Io)
    }

    async fn write_bulk(&mut self, addr: u32, data: &[u32]) -> Result<()> {
        let byte_len = data.len() * 4;
        let byte_addr = addr;
        let byte_data =
            unsafe { core::slice::from_raw_parts(data.as_ptr() as *const u8, byte_len) };
        self.writer
            .write(byte_addr, byte_data)
            .await
            .map_err(|_| Error::Io)
    }
}
