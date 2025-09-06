# Cargo fmt
cargo fmt

# Cargo clippy
cargo clippy --all-features -- -D warnings
cargo clippy --no-default-features -- -D warnings

# Builds
cargo build --release
cargo build --release --no-default-features
