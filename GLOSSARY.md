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
