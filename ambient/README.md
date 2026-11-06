# ambient: HDR screensavers (Phase 2, not started)

Lumen's own screensaver, played by the shell (decision 16). The TV's graphics
layer is always SDR, and only its video plane can show HDR, so the loops play
there, under the shell's own clear surface.

* **Mechanism:** with Lumen in front, the shell subscribes to the power
  service's screensaver request and answers `ack:false`, which holds LG's
  screensaver off, then shows its own. With another app in front, LG's
  screensaver runs as today. If Lumen dies, its subscription goes with it and
  LG's screensaver comes back by itself. Nothing is mounted over LG's files.
  (The earlier plan, a QML file bind-mounted over LG's screensaver, is
  dropped.)
* **Playback:** LG's media libraries, loaded at run time (`libplayerAPIs` and
  ACB, the route plx-native proved for HDR10 and Dolby Vision 5 and 8 on
  webOS 4.10.2). A seamless-loop spike comes first: nobody has tested one.
* **Sources:** original loops from `studio/`, and the owner's own videos from
  the PC or a LAN folder. The TV streams nothing for the screensaver
  (decision 12).
* **OLED care:** nothing static on screen, the picture moving and dimming
  slowly, and LG's Screen Off after 2 hours with no key pressed; any key
  wakes it.
* **Later trial:** taking over from other apps when they sit idle, with a key
  going back to the app.
