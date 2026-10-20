# ambient: HDR screensavers (Phase 2, not started)

A replacement for webOS's screensaver that plays HDR10 and Dolby Vision video
loops, since the TV's graphics layer is always SDR and only its video plane can
show HDR.

* **Mechanism:** a bind mount over
  `/usr/palm/applications/com.webos.app.screensaver/qml/main.qml`, as
  [webosbrew/custom-screensaver](https://github.com/webosbrew/custom-screensaver)
  does. Phase 0's recon confirms the file exists on the B9.
* **Playback:** QML's `Video` element. If it cannot do HDR on webOS 4.5, an
  ambient mode inside the shell plays the loops through LG's native media
  pipeline instead.
* **Sources:** original loops from `studio/`, Apple aerials streamed at run
  time (opt-in, never bundled), and the owner's own videos from USB, a LAN
  folder or Plex.
* **OLED care:** nothing static on screen, a slowly drifting clock, and LG's
  screen-off after a set time.
* **Upstream:** offered to Glasshouse as an "HDR video" screensaver. These
  files will be dual-licensed MIT for that. Until it lands, Glasshouse's
  screensaver stays on "LG default" so only one tool owns the mount.
