# Forge Terminal

Forge is a fast, hardware-accelerated terminal emulator written in Rust. It utilizes Vulkan for high-performance rendering and natively supports the Wayland display protocol.

## Features
- **GPU Accelerated Rendering**: Powered by Vulkan for ultra-low latency and smooth performance.
- **Wayland Native**: Built from the ground up for modern Linux desktop environments.
- **Configurable**: Fully customizable through Lua scripts. Change themes, keyboard shortcuts, window padding, and more dynamically without needing a restart.
- **Advanced Text Shaping**: Integrates `rustybuzz` for complex font ligatures and shaping.
- **Smart Cursor Context**: Context-aware cursor switching between I-Beam (normal mode) and Block/Pointer (alternate buffer mode for tools like `vim` and `btop`).

## Installation
To install the Forge terminal emulator, please use the `forge-installer` provided in the latest release tag. 
You can find the latest release [here (v1.0.0)](https://github.com/kabir/forge/releases/tag/v1.0.0).

## Build Requirements
Building Forge requires `cmake` and a C compiler for `mlua` (vendored `luajit`) and `shaderc`.

To build the project in release mode:
```bash
cargo build --release
```

## Configuration
Forge uses a `.lua` file for configuration. Check out `config.lua.example` or the `forge-config` crate documentation to get started with creating your own theme and keybindings.

## License
This project is licensed under the Apache License 2.0. See the `LICENSE` file for more details.
