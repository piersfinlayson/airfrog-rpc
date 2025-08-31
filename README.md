# airfrog-rpc

Airfrog-rpc contains objects for performing remote procedure calls and reading/writing over SWD and other debug processing.

## Overview

Airfrog-rpc provides a robust, bidirectional communication protocol between an SWD controller and target microcontroller. Unlike debugging tools, this enables the target to function as a co-processor, handling commands and returning results reliably.

## Features

- Command/response semantics with sequence numbers
- Variable-length payloads
- Atomic operations using aligned word writes
- `no-std` target implementation
- Error handling and timeout support

## Supported Targets

Includes:

- ARM Cortex-M4 (STM32F4 series)
- ARM Cortex-M33 (RP2350)

## Usage

**Target side:**

```rust
use airfrog_rpc::Target;

let mut rpc = Target::new(sram_region);
rpc.register_handler(0x01, my_command_handler);
rpc.poll(); // Call from main loop or separate Task
```

**Controller side:**

```rust
use airfrog_rpc::Controller;

let response = controller.call(0x01, &command_data).await?;
```

## Protocol

Uses separate memory-mapped channels for commands and responses, leveraging Cortex-M strong memory ordering and SWD atomic visibility.
