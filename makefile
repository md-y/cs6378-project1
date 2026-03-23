all: build

build:
	cargo build

lint:
	cargo clippy
