# takeover: making Home open the shell (Phase 3, not started)

* **Primary:** a compositor key filter, an ES5 JavaScript handler named in the
  configd key `com.webos.surfacemanager.keyFilters`, ahead of LG's own filters
  in `/usr/lib/qt5/qml/KeyFilters/` on webOS 4. Glasshouse documents this
  mechanism and runs it on webOS 4 (see its IMPLEMENTATION.md, "Remote keys
  through the compositor's filter chain").
  * On webOS 4 the handler's case goes at the top of `handleSystemKeys`' switch,
    because stock cases there fall through into each other.
  * node 0.12 has no `--check`, so the file is validated with `new Function(src)`
    before it is activated.
* **Behaviour:** a short Home press opens the Lumen shell, and holding Home
  opens LG's Home.
* **Backstops:**
  * a foreground watcher, rate-limited to 3 switches in 30 s
  * LG Input Hook, if the key filter can't be used
  * a heartbeat: if the shell stops answering for 10 s, Home goes back to LG
* **Coexistence:** Glasshouse Home stays off. Lumen's filter entry handles Home
  only and is removed on uninstall.
