# Lumen build targets. Run them inside the build container:
#
#   tools/dev.sh make ipk      # cross-compile for the TV and package an .ipk
#   tools/dev.sh make check    # formatting, lints and tests
#   tools/dev.sh make capture-tools   # tcpdump for the TV (dist/capture/)
#   tools/dev.sh make kern-table      # Inter's kerning table (shell/assets/fonts/)
#   tools/dev.sh make guard    # Lumen Guard for the TV (dist/guard/)
#   tools/dev.sh make ctl      # lumen-ctl for Lumen Remote (dist/ctl/)
#
# Talking to the TV (install, launch, logs, recon) is tools/tv.sh, which runs on
# the host so it can use your SSH key.

# webOS app ID, reverse-DNS from the GitHub owner (github.com/SunnySide007).
APP_ID  ?= io.github.sunnyside007.lumen.shell
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' shell/Cargo.toml | head -n 1)
TARGET  := armv7-unknown-linux-gnueabi
BIN     := shell/target/$(TARGET)/release/lumen-shell
GUARD   := shell/target/$(TARGET)/release/lumen-guard
CTL     := shell/target/$(TARGET)/release/lumen-ctl
STAGE   := dist/stage/$(APP_ID)

.PHONY: all build-tv ipk capture-tools kern-table guard ctl test lint check clean

all: ipk

# tcpdump for the TV's DNS capture (tools/capture.sh); not part of check.
capture-tools:
	sh tools/capture/build.sh

# Inter's kerning, pulled out of its GPOS table into the small table text.rs
# builds in (committed; `make test` checks it matches the font).
kern-table:
	python3 -I tools/font/kern_table.py

build-tv:
	cd shell && cargo build --release --target $(TARGET)

# The DNS forwarder for the TV (tools/guard.sh push); not part of check. The
# target directory is the container's, so the binary is copied out to dist/.
guard:
	cd shell && cargo build --release --target $(TARGET) -p lumen-guard
	mkdir -p dist/guard
	install -m 0755 $(GUARD) dist/guard/lumen-guard
	@ls -l dist/guard/lumen-guard

# The remote's control binary for the TV (tools/remote.sh install); not part
# of check, which builds and tests it with the rest of the workspace.
ctl:
	cd shell && cargo build --release --target $(TARGET) -p lumen-ctl
	mkdir -p dist/ctl
	install -m 0755 $(CTL) dist/ctl/lumen-ctl
	@ls -l dist/ctl/lumen-ctl

# ares-package (LG's packager) writes the control fields the webosbrew
# repository checks for, which a hand-made .ipk would lack. It also makes every
# folder 0777, which fix_modes.py puts right; an IPK it cannot fix is removed,
# so none is left to install with world-writable folders.
ipk: build-tv
	rm -rf dist/stage
	mkdir -p $(STAGE)
	install -m 0755 $(BIN) $(STAGE)/lumen-shell
	install -m 0755 packaging/shell/lumen-icons.sh $(STAGE)/
	install -m 0644 packaging/shell/icon80.png packaging/shell/icon130.png $(STAGE)/
	install -m 0644 shell/assets/fonts/Inter-LICENSE.txt $(STAGE)/
	mkdir -p $(STAGE)/art
	install -m 0644 shell/assets/art/hero-*.png shell/assets/art/CREDITS.md $(STAGE)/art/
	# The owner's Top Shelf art, if any (git-ignored; shell/assets/README.md).
	if [ -d shell/assets/art-owner ]; then \
	    mkdir -p $(STAGE)/art-owner; \
	    find shell/assets/art-owner -maxdepth 1 -type f -name '*.png' \
	        -exec install -m 0644 {} $(STAGE)/art-owner/ \; ; \
	fi
	sed -e 's/@APP_ID@/$(APP_ID)/' -e 's/@VERSION@/$(VERSION)/' \
	    packaging/shell/appinfo.json.in > $(STAGE)/appinfo.json
	ares-package --no-minify --outdir dist $(STAGE)
	python3 -I tools/ipk/fix_modes.py dist/$(APP_ID)_$(VERSION)_arm.ipk \
	    || { rm -f dist/$(APP_ID)_$(VERSION)_arm.ipk; exit 1; }
	rm -rf dist/stage
	@ls -l dist/*.ipk

# --workspace: lumen-guard and lumen-ctl are not default members
# (shell/Cargo.toml).
test:
	cd shell && cargo test --workspace
	python3 -I -m unittest discover -s tools/tests -p 'test_*.py'
	bats tools/tests
	node --test tools/tests/takeover-filter.test.js

lint:
	cd shell && cargo fmt --all --check
	cd shell && cargo clippy --workspace --all-targets -- -D warnings
	cd shell && cargo clippy --workspace --release --target $(TARGET) --all-targets -- -D warnings
	shellcheck tools/*.sh tools/recon/*.sh tools/backup/*.sh tools/capture/*.sh profiles/glasshouse/*.sh profiles/privacy/*.sh profiles/remote/*.sh profiles/privacy/lumen-privacy profiles/privacy/lumen-guard takeover/*.sh takeover/lumen-home packaging/shell/*.sh .githooks/pre-push

check: lint test

clean:
	rm -rf dist
