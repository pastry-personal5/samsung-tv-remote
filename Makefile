.PHONY: help run build bundle bundle-candidate bundle-p3-spike-candidate run-mac run-mac-app open-mac open-mac-app server-status request-api-access clean test fmt check clippy install install-bundle install-built-bundle install-mac uninstall
.DEFAULT_GOAL := help

# Binary name from Cargo.toml
BINARY := samsung-tv-remote
BUNDLE := target/bundle/Samsung TV Remote.app
APPLICATIONS_DIR := $(HOME)/Applications
APP_BUNDLE := $(APPLICATIONS_DIR)/Samsung TV Remote.app
APP_EXECUTABLE := $(APP_BUNDLE)/Contents/MacOS/$(BINARY)

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
		'  bundle-p3-spike-candidate Build a preserved diagnostic-only candidate' \
		'  run-mac         Alias for run-mac-app' \
		'  run-mac-app     Open the installed application' \
		'  open-mac[-app]   Alias for run-mac-app' \
		'  server-status   Check authenticated installed-server status (no start)' \
		'  request-api-access Explicitly request native access to existing API items' \
		'  install-bundle   Build, settle the installed app/server, then install' \
		'  install-built-bundle Install the existing signed bundle without rebuilding' \
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

bundle bundle-candidate bundle-p3-spike-candidate:
	@set -eu; \
	if [ -n "$${SAMSUNG_TV_CODESIGN_IDENTITY:-}" ]; then \
		zsh scripts/build_macos_bundle.sh $(if $(filter bundle-p3-spike-candidate,$@),--p3-spike-candidate,$(if $(filter bundle-candidate,$@),--acceptance-candidate)); \
	elif [ -s target/local-signing-identity.txt ]; then \
		IFS= read -r samsung_signing_identity < target/local-signing-identity.txt; \
		SAMSUNG_TV_CODESIGN_IDENTITY="$$samsung_signing_identity" zsh scripts/build_macos_bundle.sh $(if $(filter bundle-p3-spike-candidate,$@),--p3-spike-candidate,$(if $(filter bundle-candidate,$@),--acceptance-candidate)); \
	else \
		printf '%s\n' 'No signing identity configured. Set SAMSUNG_TV_CODESIGN_IDENTITY to an Apple Development or Developer ID identity.' >&2; \
		exit 2; \
	fi

run-mac open-mac open-mac-app: run-mac-app

run-mac-app:
	open "$(APP_BUNDLE)"

server-status:
	"$(APP_EXECUTABLE)" --server-status

request-api-access:
	"$(APP_EXECUTABLE)" --server-api-access --request-access

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

install-bundle install-built-bundle:
	python3 scripts/install_macos_bundle.py install "$(BUNDLE)" "$(APP_BUNDLE)"

install-mac: install-bundle

uninstall:
	rm -f /usr/local/bin/$(BINARY)
