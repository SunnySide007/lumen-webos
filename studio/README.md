# studio: HDR content pipeline (Phase 2, not started)

Original screensaver loops, made for an OLED panel and released under
CC BY-SA 4.0.

* **Design for the panel:** near-black scenes with low average light and small
  highlights. An OLED dims large bright areas (ABL), but a small highlight can
  reach the B9's ~700–800 nit peak next to perfect black. Think aurora,
  starfields, bioluminescence, ink in water, neon rain.
* **Pipeline:**
  1. Render procedural scenes to 16-bit linear EXR.
  2. Grade to BT.2020 primaries and the PQ curve, mastered at 1000 nits with the
     real MaxCLL and MaxFALL measured from the frames.
  3. Encode with x265: HEVC Main10, with mastering-display and content-light SEI.
  4. Optionally add a Dolby Vision Profile 8.1 RPU with `dovi_tool`.
  5. Make the last frame meet the first, so loops are seamless.
* **Verification:** the TV must switch into its HDR picture mode on each loop.
