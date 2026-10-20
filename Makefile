# Lumen build targets. Run them inside the build container:
#
#   tools/dev.sh make ipk      # cross-compile for the TV and package an .ipk
#   tools/dev.sh make check    # formatting, lints and tests
#
# Talking to the TV (install, launch, logs, recon) is tools/tv.sh, which runs on
# the host so it can use your SSH key.

# webOS app ID, reverse-DNS from the GitHub owner (github.com/SunnySide007).
APP_ID  ?= io.github.sunnyside007.lumen.shell
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' shell/Cargo.toml | head -n 1)
TARGET  := armv7-unknown-linux-gnueabi
BIN     := shell/target/$(TARGET)/release/lumen-shell
STAGE   := dist/stage/$(APP_ID)

.PHONY: all build-tv ipk test lint check clean

all: ipk

build-tv:
	cd shell && cargo build --release --target $(TARGET)

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

test:
	cd shell && cargo test
	python3 -I -m unittest discover -s tools/tests -p 'test_*.py'
	bats tools/tests

lint:
	cd shell && cargo fmt --check
	cd shell && cargo clippy --all-targets -- -D warnings
	cd shell && cargo clippy --release --target $(TARGET) --all-targets -- -D warnings
	shellcheck tools/*.sh tools/recon/*.sh

check: lint test

clean:
	rm -rf dist
