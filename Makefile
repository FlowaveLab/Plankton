DESKTOP_DIR := apps/desktop

.PHONY: install fmt fmt-check lint build desktop-build test typecheck check tauri-dev

install:
	npm --prefix $(DESKTOP_DIR) install

fmt:
	cargo fmt --all
	npm --prefix $(DESKTOP_DIR) run format:write

fmt-check:
	cargo fmt --all -- --check
	npm --prefix $(DESKTOP_DIR) run format

lint:
	cargo clippy --workspace --all-targets --all-features -- -D warnings

build:
	cargo build --workspace
	npm --prefix $(DESKTOP_DIR) run build

desktop-build:
	npm --prefix $(DESKTOP_DIR) run tauri build -- --debug

test:
	cargo test --workspace
	npm --prefix $(DESKTOP_DIR) run test

typecheck:
	cargo check --workspace
	npm --prefix $(DESKTOP_DIR) run typecheck

check: fmt-check lint test typecheck build

tauri-dev:
	npm --prefix $(DESKTOP_DIR) run tauri dev
