//! Remote Procedure Call crate for co-processing with ARM targets over SWD and other debug
//! protocols.
//!
//! This crate enables reliable, bidirectional communication between a debug host and target
//! microcontroller using memory-mapped channels. Unlike traditional debugging tools, this
//! allows the host and target act as co-processors alongisde each other, with one being
//! controlled by the other, handling commands and returning results.
//!
//! `no_std`.  Requires `alloc` for async traits, typically used on the Host side..
//!
//! Includes sample host and target implementations.
//!
//! ## Architecture
//!
//! Assumes a Host (debug controller) and Target (microcontroller) architecture.
//!
//! Communication typically takes place using two unidirectional channels in the target's SRAM:
//! - **Command channel**: Host writes commands, target reads
//! - **Response channel**: Target writes responses, host reads
//!
//! Channels can be used in either direction, and for different purposes as required.  All that
//! is required is a dedicated memory region in the target's SRAM for each channel.  CCM RAM
//! may also be used on STM32F4 MCUs if available.
//!
//! Each channel contains a control block (sequence numbers, flags, data size) followed by a
//! data area, with the data area used to transmit information on the channel.  The amount of
//! memory provided for each channel is used for both the control block and data area.  The
//! control block is small (tens of byes), so the majority of the reserved memory is available
//! for data.
//!
//! Only a single producer and single consumer are supported per channel, and only one data
//! chunk can be written and "transferred" by the producer to the consumer at a time.
//!
//! The channel protocol ensures reliable delivery using producer/consumer sequence numbers
//! and atomic word operations using the target's memory ordering guarantees.
//!
//! Data can be writte in any format, using bytes or 32-bit little endian words.
//!
//! Currently this crate only supports little-endian Hosts _and_ Targets.
//!
//! ## Modules
//!
//! - [`channel`] - Channel objects for unidirectional communication in either direction
//! - [`client`] - RPC client for sending commands and receiving responses, typically used
//!   on the host
//! - [`io`] - Async I/O traits for debug interface access, typically used for host access
//!   to target RAM/flash peripherals
//!
//! ## Supported Targets
//!
//! Works on ARM Cortex-M microcontrollers with strong memory ordering, with SWD accessing
//! the SRAM via the AHB bus matrix, and no data caches:
//! - Cortex-M0/M0+/M3/M4/M23/M33 (STM32, RP2040/2350, nRF52, etc.)
//!
//! The target implementation, [`channel::RamChannel`] can be used as is.  The host
//! implementation needs [`io::Reader`] and [`io::Writer`] implementations.
//!
//! `airfrog::airfrog_bin::firmare` contains an SWD implementation of [`io::Reader`] and
//! [`io::Writer`] for ESP32-C3, and uses [`channel::ReaderWriterChannel`], allowing it
//! to communicate using these channels with an SWD target.
//!
//! ## Getting Started
//!
//! The target must reserved dedicated SRAM regions for each channel.
//!
//! The host (debug controller) must know the addresses of these regions and reads/writes
//! them over the debug interface.
//!
//! If you wish the host to dynamically learn the addresses/sizes of these regions you must
//! implement this separately - for example, include pointers at well know SRAM or flash
//! locations on the target pointeing to these regions.  Alternatively, you could arrange
//! for the locations and sizes to be fixed by the target's linker script.
//!
//! Each communication channel consists of a control block, and data area.  The control
//! block is used by the protocol to ensure reliable message passing.  The data area is
//! where the actual command or response payload is stored.
//!
//! **Target setup**:
//! 1. Reserve SRAM region(s) e.g. 1KB each for each channel
//! 2. Create a [`channel::RamChannelIo`] instance for each required channel
//! 3. Create a [`channel::RamChannel`] instance for each channel
//! 4. Poll [`channel::RamChannel::data_available()`] for your consumer channel, in your
//!    main loop or dedicated task
//! 5. When data arrives, process it, and optionally send responses on alternate channel
//! 6. Data format is application-specific and currently either bytes or u32s
//!
//! **Host setup**:
//! 1. Configure channel locations, or dynamically read from the target using well-known
//!    locations for pointers to the channel locations/sizes
//! 2. Create a [`channel::ReaderWriterChannelIo`] instance with your debug interface
//!    reader/writer implementation (see `airfrog::airfrog_bin::firmware` for an SWD
//!    implementation)
//! 3. Create a [`channel::ReaderWriterChannel`] instance for your channel.
//! 4. Send data with [`channel::ReaderWriterChannel::publish_bytes()`].
//! 5. Before using a different channel you will likely need to ensure the previous
//!    channel is dropped, to free up the Io instance to be mutably borrowed by your
//!    new channel.
//!
//! As implied a Channel is intended to be short-lived - create, use, drop. This allows
//! temporary ownership of the Reader/Writer, which may be a shared hardware resource on
//! the host.  If this becomes tedious, create a wrapper to abstract away this creation/
//! destruction or use [`client::AsyncRpcClient`] which abstracts this lifecycle away,
//! and provides a higher-level request() API, using a pair of channels (one command,
//! the other response).
//!
//! The RPC layer handles reliable delivery, but your application defines the actual
//! command/response protocol and data formats.
//!
//! While the above documentation describes the Host controlling the Target, it is
//! possible to use the channel(s) in the reverse direction.
//!
//! See individual module and struct documentation for usage examples.
//!
//! ## Features
//!
//! Default features:
//! - `async` - Enable async channel implementations and traits (requires `alloc`), which
//!   is generally required by the Host, but not by the Target.
//!
//! Compile with `--no-default-features` to disable unnecessary async support for a Target.

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#![no_std]

#[cfg(feature = "async")]
extern crate alloc;

pub mod channel;
pub mod client;
pub mod io;

/// RPC errors
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Error {
    /// No data available
    NoData,
    /// Channel busy
    Busy,
    /// Timeout waiting for response
    Timeout,
    /// Invalid operation
    InvalidOperation,
    /// Payload too large for buffer
    PayloadTooLarge,
    /// Sequence mismatch
    SequenceMismatch,
    /// Buffer too small for operation
    BufferTooSmall,
    /// I/O error
    Io,
    /// Uninitialized channel
    Uninit,
    /// Data area or buffer not aligned
    NotAligned,
}

/// Type to represent the result of an RPC operation
pub type Result<T> = core::result::Result<T, Error>;
