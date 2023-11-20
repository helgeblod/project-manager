alias b := build
alias c := clean
alias i := install
alias r := run
alias t := test
alias l := lint
alias ra := release-all

# Build binary for all platforms 🌐
release-all: release release-linux release-win

# Build the Rust project 🛞
build:
    cargo build

# Build a production release 🚀
release:
    cargo build --release

# Build a release for Linux 🐧
release-linux:
    cargo build --release --target x86_64-unknown-linux-musl

# Build a release for Windows 🪟
release-win:
    cargo build --release --target x86_64-pc-windows-gnu

# Install locally to ~/.cargo/bin/ 🚚
install:
    cargo install --path .

# Run the Rust project 🥾
run:
    cargo run

# Clean the build artifacts 🧹
clean:
    cargo clean

# Test the Rust project 🧪
test:
    cargo test

# Format the Rust code using rustfmt 💅
format:
    cargo fmt

# Check the Rust code using clippy 📎
lint:
    cargo clippy

# Generate documentation for the Rust project 📝
doc:
    cargo doc --no-deps

# Default task 🤖
default: build
