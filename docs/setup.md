# Setup

## 1. Development machine

* **Docker** (Docker Desktop on Windows or macOS). The build container
  (`docker/Dockerfile`) holds the webOS cross-toolchain, Rust, LG's packager and
  the linters; `tools/dev.sh` runs commands in it.
* **Git Bash** on Windows, for the `tools/*.sh` scripts.
* Optional, for the simulator on Windows: Rust (rustup) and the Visual Studio
  C++ Build Tools. `tools/sim.ps1` downloads the official SDL2 release on first use.

Then, in the clone, turn on the pre-push hook that runs `make check` before
each push:

```sh
git config core.hooksPath .githooks
```

## 2. SSH access to the TV

Lumen talks to the TV over SSH with a key, never a password. With Homebrew
Channel installed and SSH enabled in its settings:

1. Make a key on your computer, used only for the TV:

   ```sh
   ssh-keygen -t ed25519 -f ~/.ssh/lumen_tv -C lumen-tv
   ```

2. Add the **public** half (`~/.ssh/lumen_tv.pub`) to the TV's
   `/home/root/.ssh/authorized_keys`. Glasshouse's Server tab can do this from a
   browser; otherwise use Homebrew Channel's documented SSH setup. Once a key is
   installed, Homebrew Channel turns off its default password.
3. Copy `.env.example` to `.env` and set `TV_HOST` to the TV's address. A DHCP
   reservation on your router keeps the address from changing.
4. Check it:

   ```sh
   tools/tv.sh check
   ```

## 3. Survey the TV (read-only)

```sh
tools/recon.sh
```

This runs [tools/recon/remote.sh](../tools/recon/remote.sh) on the TV, which
only reads: firmware, hardware, graphics libraries, system apps, settings,
mounts, boot hooks, and input, Bluetooth, USB, network and audio devices. The
raw output stays in `tools/out/` (git ignores it); a redacted copy, with serial
numbers, MAC addresses, local IPs, account identifiers and Bluetooth device
names removed, goes to `docs/device/`. Read it before committing it.

## 4. Build, install and run

```sh
tools/dev.sh make ipk
tools/tv.sh install dist/io.github.sunnyside007.lumen.shell_0.0.1_arm.ipk
tools/tv.sh launch io.github.sunnyside007.lumen.shell
tools/tv.sh log
```

`hello-gles` shows the GPU name, GL version and live frame times on screen.
Press BACK to leave it. `tools/tv.sh remove io.github.sunnyside007.lumen.shell` uninstalls it.

## 5. TV settings that matter later

* **Quick Start+ off** (Settings › General › Additional settings): with it on,
  the TV never boots fully, and boot hooks only run after a real restart.
* **Automatic software updates off**, and Homebrew Channel's *Block system
  updates* on. A firmware update can remove root.
