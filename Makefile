.PHONY: help run build bundle bundle-candidate open-mac-app clean test fmt check clippy install install-bundle install-mac uninstall
.DEFAULT_GOAL := help

# Binary name from Cargo.toml
BINARY := samsung-tv-remote
BUNDLE := target/bundle/Samsung TV Remote.app
APPLICATIONS_DIR := $(HOME)/Applications

help:
	@printf '%s\n' \
		'Usage: make <target>' \
		'' \
		'Targets:' \
		'  help             Show this help message' \
		'  run              Run the application' \
		'  build            Build the release binary' \
		'  bundle           Build the signed application bundle' \
		'  bundle-candidate Build an acceptance-candidate bundle' \
		'  open-mac-app     Open the installed application' \
		'  install-bundle   Install the application bundle to ~/Applications' \
		'  install-mac      Alias for install-bundle' \
		'  install          Install the release binary to /usr/local/bin' \
		'  uninstall        Remove the installed release binary' \
		'  test             Run tests' \
		'  fmt              Format Rust code' \
		'  check            Run cargo check' \
		'  clippy           Run Clippy with warnings denied' \
		'  clean            Remove build artifacts'

run:
	cargo run

build:
	cargo build --release

bundle bundle-candidate:
	@set -eu; \
	if [ -n "$${SAMSUNG_TV_CODESIGN_IDENTITY:-}" ]; then \
		zsh scripts/build_macos_bundle.sh $(if $(filter bundle-candidate,$@),--acceptance-candidate); \
	elif [ -s target/local-signing-identity.txt ]; then \
		IFS= read -r samsung_signing_identity < target/local-signing-identity.txt; \
		SAMSUNG_TV_CODESIGN_IDENTITY="$$samsung_signing_identity" zsh scripts/build_macos_bundle.sh $(if $(filter bundle-candidate,$@),--acceptance-candidate); \
	else \
		printf '%s\n' 'No signing identity configured. Run zsh scripts/create_local_signing_identity.sh or set SAMSUNG_TV_CODESIGN_IDENTITY.' >&2; \
		exit 2; \
	fi

open-mac-app:
	open "/Users/user1/Applications/Samsung TV Remote.app"

test:
	cargo test

check:
	cargo check

fmt:
	cargo fmt

clippy:
	cargo clippy --all-targets -- -D warnings

clean:
	cargo clean

install: build
	install -d /usr/local/bin
	install -m 755 target/release/$(BINARY) /usr/local/bin/$(BINARY)

install-bundle: bundle
	install -d "$(APPLICATIONS_DIR)"
	ditto --noacl "$(BUNDLE)" "$(APPLICATIONS_DIR)/Samsung TV Remote.app"

install-mac: install-bundle

uninstall:
	rm -f /usr/local/bin/$(BINARY)
