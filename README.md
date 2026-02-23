## About

Simple replacement for [fail2ban](https://www.fail2ban.org).

## Prerequisites

This application requires **libsystemd** to be installed on your system.

Install the development package for your distribution:
- Debian/Ubuntu: `sudo apt-get install libsystemd-dev`
- Fedora/RHEL: `sudo dnf install systemd-devel`
- Arch Linux: `sudo pacman -S systemd-libs`
- Alpine Linux: `sudo apk add systemd-dev`

## Installation

### With Cargo

Installation via cargo:

```shell
rustup update stable

git clone https://github.com/potoo0/logban && cd logban
SQLX_OFFLINE=true cargo install --path . --locked
```

### From binaries (Linux, macOS, Windows)

1. Download the [latest release binary](https://github.com/potoo0/logban/releases)
2. Set the `PATH` environment variable

## Usage

```
$ logban -h
Usage: logban [OPTIONS]

Options:
  -c, --config <CONFIG>        [default: config.yaml]
  -n, --dry-run
  -l, --log-level <LOG_LEVEL>  Set the log level (overrides env var), eg: info,logban=trace
  -h, --help                   Print help
  -V, --version                Print version
```

Example:

```bash
logban -c config.example.yaml -l debug,logban=trace,sqlx=info
```

### Configuration

By default, the application looks for a config.yaml file in the current directory.
To use a different file, you can specify a custom path using the `-c` or `--config` option.

A comprehensive example with all available settings can be found in [config.example.yaml](./config.example.yaml), which is a great starting point for creating your own configuration.

## Development

This project is in early stages of development, and is not yet ready for production use. If you would like to contribute, please feel free to submit a pull request or open an issue.

### SQL Compile-time verification

```bash
# build with db
echo 'DATABASE_URL=sqlite://logban.db' >> .env
sqlx database create
sqlx migrate run
cargo sqlx prepare # saving query metadata for offline mode 

# or build with offline mode
SQLX_OFFLINE=true cargo build
```
