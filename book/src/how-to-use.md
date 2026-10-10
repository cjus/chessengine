# How to use this book

## Reading the book locally

```sh
cargo install mdbook
mdbook serve book --open
```

## Running the engine at any stage

```sh
git clone https://github.com/cjus/chessengine
cd chessengine
git checkout stage-05-alphabeta   # any stage tag
cargo build --release
./target/release/cb2026
```

The Rust toolchain version is pinned in `rust-toolchain.toml`; `rustup` installs it on first
use.

## Switching techniques off

From the stages that add search techniques onwards, every technique can be switched off at
runtime (in the shell with `set <technique> off`), so you can measure what each one is worth
yourself.
