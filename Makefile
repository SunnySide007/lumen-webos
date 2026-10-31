# Lumen build targets. Run them inside the build container:
#
#   tools/dev.sh make ipk      # cross-compile for the TV and package an .ipk
#   tools/dev.sh make check    # formatting, lints and tests
#   tools/dev.sh make capture-tools   # tcpdump for the TV (dist/capture/)
#   tools/dev.sh make guard    # Lumen Guard for the TV (dist/guard/)
#
# Talking to the TV (install, launch, logs, recon) is tools/tv.sh, which runs on
# the host so it can use your SSH key.

# webOS app ID, reverse-DNS from the GitHub owner (github.com/SunnySide007).
APP_ID  ?= io.github.sunnyside007.lumen.shell
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' shell/Cargo.toml | head -n 1)
TARGET  := armv7-unknown-linux-gnueabi
BIN     := shell/target/$(TARGET)/release/lumen-shell
GUARD   := shell/target/$(TARGET)/release/lumen-guard
STAGE   := dist/stage/$(APP_ID)

.PHONY: all build-tv ipk capture-tools guard test lint check clean

all: ipk

# tcpdump for the TV's DNS capture (tools/capture.sh); not part of check.
capture-tools:
	sh tools/capture/build.sh

build-tv:
	cd shell && cargo build --release --target $(TARGET)

# The DNS forwarder for the TV (tools/guard.sh push); not part of check. The
# target directory is the container's, so the binary is copied out to dist/.
guard:
	cd shell && cargo build --release --target $(TARGET) -p lumen-guard
	mkdir -p dist/guard
	install -m 0755 $(GUARD) dist/guard/lumen-guard
	@ls -l dist/guard/lumen-guard

# ares-package (LG's packager) writes the control fields the webosbrew
# repository checks for, which a hand-made .ipk would lack.
ipk: build-tv
	rm -rf dist/stage
	mkdir -p $(STAGE)
	install -m 0755 $(BIN) $(STAGE)/lumen-shell
	install -m 0644 packaging/shell/icon80.png packaging/shell/icon130.png $(STAGE)/
	sed -e 's/@APP_ID@/$(APP_ID)/' -e 's/@VERSION@/$(VERSION)/' \
	    packaging/shell/appinfo.json.in > $(STAGE)/appinfo.json
	ares-package --no-minify --outdir dist $(STAGE)
	rm -rf dist/stage
	@ls -l dist/*.ipk

# --workspace: lumen-guard is not a default member (shell/Cargo.toml).
test:
	cd shell && cargo test --workspace
	python3 -I -m unittest discover -s tools/tests -p 'test_*.py'
	bats tools/tests

lint:
	cd shell && cargo fmt --all --check
	cd shell && cargo clippy --workspace --all-targets -- -D warnings
	cd shell && cargo clippy --workspace --release --target $(TARGET) --all-targets -- -D warnings
	shellcheck tools/*.sh tools/recon/*.sh tools/backup/*.sh tools/capture/*.sh profiles/glasshouse/*.sh profiles/privacy/*.sh profiles/privacy/lumen-privacy profiles/privacy/lumen-guard .githooks/pre-push

check: lint test

clean:
	rm -rf dist
