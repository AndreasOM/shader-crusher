r:
	cargo run --release

d:
	cargo run

cr:
	cargo check --release

c:
	cargo check
	
fmt:
	cargo +nightly fmt

test-noisy:
	cargo test -- --nocapture
