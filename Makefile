# Lumen build targets. Run them inside the build container:
#
#   tools/dev.sh make ipk      # cross-compile for the TV and package an .ipk
#   tools/dev.sh make check    # formatting, lints and tests
#   tools/dev.sh make capture-tools   # tcpdump for the TV (dist/capture/)
#   tools/dev.sh make kern-table      # Inter's kerning table (shell/assets/fonts/)
#   tools/dev.sh make guard    # Lumen Guard for the TV (dist/guard/)
#   tools/dev.sh make ctl      # lumen-ctl for Lumen Remote (dist/ctl/)
#   tools/dev.sh make lumend   # lumend, its stand-in and its door (dist/lumend/)
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
LUMEND  := shell/target/$(TARGET)/release/lumend
STANDIN := shell/target/$(TARGET)/release/lumen-standin
STAGE   := dist/stage/$(APP_ID)
# The platform layer's trial (lane L, docs/specs/depth.md item 17):
# PLATFORM=wayland puts a `platform` file beside the executable, so every
# launch, a Home press too, runs the Wayland path; LIFECYCLE=2 marks the app
# for the app manager's lifecycle version 2 (an envelope for the launch
# argument, and residence), which only the Wayland path registers for. The
# defaults are the IPK the B9 runs today: SDL, version 1.
PLATFORM  ?=
LIFECYCLE ?= 1

.PHONY: all build-tv check-trial ipk capture-tools kern-table guard ctl lumend licenses test lint check clean

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

# Lumen's root daemon for the TV (tools/lumend.sh push); not part of check,
# which builds and tests it with the rest of the workspace.
lumend:
	cd shell && cargo build --release --target $(TARGET) -p lumend
	mkdir -p dist/lumend
	install -m 0755 $(LUMEND) $(STANDIN) dist/lumend/
	install -m 0644 profiles/lumend/lumend dist/lumend/door
	@ls -l dist/lumend/

# The trial's switches, refused before the build when they make no sense.
check-trial:
	@case "$(PLATFORM)" in ""|sdl|wayland) ;; *) echo "PLATFORM must be sdl or wayland, not $(PLATFORM)" >&2; exit 1 ;; esac
	@case "$(LIFECYCLE)" in 1) ;; 2) [ "$(PLATFORM)" = wayland ] || { echo "LIFECYCLE=2 needs PLATFORM=wayland: LG's SDL registers for version 1" >&2; exit 1; } ;; *) echo "LIFECYCLE must be 1 or 2, not $(LIFECYCLE)" >&2; exit 1 ;; esac

# ares-package (LG's packager) writes the control fields the webosbrew
# repository checks for, which a hand-made .ipk would lack. It also makes every
# folder 0777, which fix_modes.py puts right; an IPK it cannot fix is removed,
# so none is left to install with world-writable folders.
ipk: check-trial build-tv
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
	if [ "$(LIFECYCLE)" = 2 ]; then \
	    sed -i 's/^  "main": "lumen-shell",$$/&\n  "nativeLifeCycleInterfaceVersion": 2,/' \
	        $(STAGE)/appinfo.json; \
	    grep -q '"nativeLifeCycleInterfaceVersion": 2' $(STAGE)/appinfo.json; \
	fi
	python3 -I -m json.tool $(STAGE)/appinfo.json > /dev/null
	if [ -n "$(PLATFORM)" ]; then \
	    echo "$(PLATFORM)" > $(STAGE)/platform && chmod 0644 $(STAGE)/platform; \
	fi
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
	shellcheck tools/*.sh tools/bench/*.sh tools/bench/tv/*.sh tools/recon/*.sh tools/backup/*.sh tools/capture/*.sh profiles/glasshouse/*.sh profiles/privacy/*.sh profiles/remote/*.sh profiles/lumend/*.sh profiles/lumend/lumend profiles/privacy/lumen-privacy profiles/privacy/lumen-guard takeover/*.sh takeover/lumen-home packaging/shell/*.sh .githooks/pre-push

# Every crate in Cargo.lock under a license Lumen's GPL-3.0-or-later can meet,
# and every direct dependency named in THIRD-PARTY.md (tools/licenses.py).
licenses:
	cd shell && cargo metadata --format-version 1 --locked | python3 -I ../tools/licenses.py ../THIRD-PARTY.md

check: lint licenses test

clean:
	rm -rf dist
