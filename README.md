# BunnySync 

A Rust-based CLI tool for syncing files with bunny.net Storage.

## Features

- Sync files between local directories and bunny.net Storage.
- Delete files from bunny.net Storage that are not present in the local directory.
- Dry run mode to preview changes without making any modifications.
- Simple code with minimal dependencies.

## Installation

To install BunnySync, you need to have Rust and Cargo installed. You can then 
build and install the tool using the following commands:

```bash
cargo install --git https://github.com/akhudek/bunnysync.git
```

## Usage

To sync a local directory to a remote zone.
```bash
bunnysync 
```

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) fil
for details.