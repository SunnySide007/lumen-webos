# Lumen

A tvOS-style home screen for a rooted LG webOS TV, and the tools that build,
check and measure it on the TV.

## Language

### The home loop

**Home loop**:
The shell's main loop: passes, bursts and idle waits, from the first pass
until the shell leaves.
_Avoid_: main loop, run loop, event loop

**Pass**:
One turn of the home loop: it takes the events that arrived, steps the
motion, and draws at most one frame.
_Avoid_: tick, iteration, frame

**Burst**:
The frames drawn one after another until the loop stops drawing. It ends at
the next idle wait (the one after a hide included), when a launch begins, or
when the shell leaves.
_Avoid_: animation, run

**Settled**:
The state in which nothing moves and nothing has changed, so no frame is due.
_Avoid_: idle, still

**Idle wait**:
The loop blocked until the next event or deadline, while settled or hidden.
_Avoid_: sleep, idle

**Dip**:
The focused tile's quick sink when OK is pressed; a launch waits until a frame
shows it at its deepest.
_Avoid_: pulse, bounce

### The TV as a whole

**Contract**:
What the TV does, written in decision 10: the inputs, the named apps, the
screensaver and LG's picture and sound settings. A feature fits it, or a new
record changes it.
_Avoid_: scope, feature list

**Resident**:
Said of the shell when it stays running behind other apps, so a Home press
brings it back instead of starting it.
_Avoid_: background mode, always-on

**Platform layer**:
The shell's own code for the window, the keys and the app's lifecycle: today
LG's SDL, later Lumen's Rust over the TV's Wayland and EGL libraries.
_Avoid_: backend, windowing

**Door**:
The few lines of POSIX `sh` a boot hook keeps: they check the kill switch,
time-box the start and run a binary. The work is in the binary.
_Avoid_: wrapper, launcher script

**lumend**:
Lumen's one root daemon on the TV: it holds LG's senders down, closes LG
Home after boot, starts and restarts the shell, and opens no network socket.
_Avoid_: the agent, the service

### Privacy

**Allowlist**:
The names Guard lets resolve; under default deny, every other name gets
0.0.0.0. Its generic part is in the repository; the owner's additions are not.
_Avoid_: whitelist

**Carve-out**:
A name under an allowed suffix that stays blocked, such as the store's
request log under the store's domain.
_Avoid_: exception, override

**Learning window**:
A stretch of minutes, started by hand from an owner device, in which Guard
allows and logs new names for the owner to keep or drop.
_Avoid_: training, discovery mode

**Owner device**:
The PC with its SSH key, or a phone the PC promoted: the only devices that
can loosen privacy. Every other paired device is **remote-only**.
_Avoid_: admin, trusted device
