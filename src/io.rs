//! Async I/O traits for accessing RAM, flash, files, etc.
//!
//! This module contains traits for reading/writing flash or RAM data on a target.
//! They can be used remotely, accessing the target over SWD or other protocols,
//! or can be used on the target itself (although they may be overkill in that
//! application).
//!
//! # Possible implementations
//!
//! - For PC-based applications: Read/write in-memory buffers or memory-mapped
//!   files, such as firmware images read from a file
//! - For accessing embedded devices: Read/write from flash via SWD, JTAG, or
//!   other debug interfaces
//! - For implementing on embedded devices: Read directly from flash memory
//!   (although this trait may be overkill)
//!
//! # Address Space
//!
//! The methods uses absolute addresses as they appear in the target's
//! memory map. For STM32F4 devices, flash typically starts at `0x08000000` and
//! RAM at `0x20000000`.
//!
//! The implementation is responsible for translating these addresses to
//! whatever internal representation it uses (file offsets, SWD commands, etc.).

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

/// Reader trait.
pub trait Reader {
    /// The error type returned by read operations.
    ///
    /// This allows implementations to use their own error types
    /// (e.g., `std::io::Error` for file I/O, custom errors for SWD).
    type Error: core::fmt::Debug;

    /// Read bytes from the firmware at the specified absolute address.
    ///
    /// # Arguments
    ///
    /// * `addr` - The absolute address to read from (e.g., `0x08000200`)
    /// * `buf` - Buffer to fill with the read data
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The address is out of bounds for the firmware
    /// - The underlying read operation fails (I/O error, communication error, etc.)
    /// - The requested read size would exceed firmware boundaries
    ///
    /// # Performance Notes
    ///
    /// Implementations should optimize for small reads (1-256 bytes) as the parser
    /// typically reads headers and metadata in small chunks. For embedded implementations
    /// reading via debug interfaces, consider implementing bulk reads and internal
    /// buffering to reduce round-trip overhead.
    fn read(
        &mut self,
        addr: u32,
        buf: &mut [u8],
    ) -> impl core::future::Future<Output = Result<(), Self::Error>> + Send;

    /// Updates the reader's base address if it is later detected that it needs
    /// to change.
    fn update_base_address(&mut self, new_base: u32);
}

/// Writer trait.
pub trait Writer {
    /// The error type returned by write operations.
    type Error: core::fmt::Debug;

    /// Write bytes to the firmware at the specified absolute address.
    ///
    /// # Arguments
    ///
    /// * `addr` - The absolute address to write to (e.g., `0x20000200`)
    /// * `data` - Data to write
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The address is out of bounds for the target
    /// - The underlying write operation fails (I/O error, communication error, etc.)
    /// - The target memory is read-only or protected
    fn write(
        &mut self,
        addr: u32,
        data: &[u8],
    ) -> impl core::future::Future<Output = Result<(), Self::Error>> + Send;

    /// Updates the writer's base address if it is later detected that it needs
    /// to change.
    fn update_base_address(&mut self, new_base: u32);
}
