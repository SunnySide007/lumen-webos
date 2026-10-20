# Device profiles

One profile per TV model Lumen has run on, in the format of Glasshouse's
[tv-specs](https://github.com/rorygallagher2024/glasshouse/tree/main/docs/development/tv-specs)
so a profile can go upstream unchanged.

A profile is written from a recon report:

1. `tools/recon.sh` saves a redacted report here as `recon-<UTC timestamp>.txt`.
2. Read it. The redactor removes serial numbers, MAC addresses, local IPs,
   device names, locations and account identifiers, but a person checks before
   anything is committed.
3. Summarise it as `<model>.md` (for example `b9.md`): model, firmware, webOS
   version, SoC, CPU, GPU, RAM, storage, graphics libraries, how the home and
   screensaver apps are built, and anything that surprised us.

Profiles record measurements, not assumptions: a value that has not been read
from a real TV is marked as such.
