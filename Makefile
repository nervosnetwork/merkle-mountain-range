default: ci

ci: fmt check-no-std clippy test bench-test c-test

test:
	cargo test --all --all-features

c-test:
	cd c/rust-tests && cargo test --all --all-features

bench-test:
	cargo bench -- --test

clippy:
	cargo clippy  --all --all-features --all-targets

fmt:
	cargo fmt --all -- --check

check-no-std:
	cargo check --all --no-default-features
