# airfrog-rpc

Used to perform remote procedure calls and reading/writing over SWD and other debug protocols.

## Overview

Airfrog-rpc provides a robust, bidirectional communication protocol between an SWD controller and target microcontroller. Unlike debugging tools, this enables the target to function as a co-processor, handling commands and returning results reliably.

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