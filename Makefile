# Lumen build targets. Run them inside the build container:
#
#   tools/dev.sh make ipk      # cross-compile for the TV and package an .ipk
#   tools/dev.sh make check    # formatting, lints and tests
#   tools/dev.sh make capture-tools   # tcpdump for the TV (dist/capture/)
#
# Talking to the TV (install, launch, logs, recon) is tools/tv.sh, which runs on
# the host so it can use your SSH key.

# webOS app ID, reverse-DNS from the GitHub owner (github.com/SunnySide007).
APP_ID  ?= io.github.sunnyside007.lumen.shell
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' shell/Cargo.toml | head -n 1)
TARGET  := armv7-unknown-linux-gnueabi
BIN     := shell/target/$(TARGET)/release/lumen-shell
STAGE   := dist/stage/$(APP_ID)

.PHONY: all build-tv ipk capture-tools test lint check clean

all: ipk

# tcpdump for the TV's DNS capture (tools/capture.sh); not part of check.
capture-tools:
	sh tools/capture/build.sh

build-tv:
	cd shell && cargo build --release --target $(TARGET)

# ares-package (LG's packager) writes the control fields the webosbrew
# repository checks for, which a hand-made .ipk would lack. It also makes every
# folder 0777, which fix_modes.py puts right; an IPK it cannot fix is removed,
# so none is left to install with world-writable folders.
ipk: build-tv
	rm -rf dist/stage
	mkdir -p $(STAGE)
	install -m 0755 $(BIN) $(STAGE)/lumen-shell
	install -m 0644 packaging/shell/icon80.png packaging/shell/icon130.png $(STAGE)/
	install -m 0644 shell/assets/fonts/Inter-LICENSE.txt $(STAGE)/
	sed -e 's/@APP_ID@/$(APP_ID)/' -e 's/@VERSION@/$(VERSION)/' \
	    packaging/shell/appinfo.json.in > $(STAGE)/appinfo.json
	ares-package --no-minify --outdir dist $(STAGE)
	python3 -I tools/ipk/fix_modes.py dist/$(APP_ID)_$(VERSION)_arm.ipk \
	    || { rm -f dist/$(APP_ID)_$(VERSION)_arm.ipk; exit 1; }
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
	shellcheck tools/*.sh tools/recon/*.sh tools/capture/*.sh profiles/glasshouse/*.sh .githooks/pre-push

check: lint test

clean:
	rm -rf dist
