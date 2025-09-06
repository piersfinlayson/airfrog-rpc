# airfrog-rpc

A crate to perform remote procedure calls and reading/writing between controllers and targets over SWD and other debug protocols.

## Overview

Airfrog-rpc provides a robust, bidirectional communication protocol between an SWD controller and target microcontroller. Unlike debugging tools, this enables the controller and target to function as a co-processing pair of MCUs, each handling commands and returning results reliably over a channel usually used for debug purposes.

## Features

- Host and Target implementations
- Asynchronous and synchronous APIs
- Command/response semantics with sequence numbers
- Variable-length payloads
- Atomic operations using aligned word writes
- `no-std` target implementation
- Error handling and timeout support

## Getting Started

See the [crate documentation](https://docs.rs/airfrog-rpc).