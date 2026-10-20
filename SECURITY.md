# Security policy

Lumen runs as root on people's televisions, so a hole in Lumen is a hole in
someone's TV. Please report vulnerabilities privately, not in a public issue.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting: on this repository's **Security**
tab, choose **Report a vulnerability**. The report opens a draft security
advisory that only you and the maintainers can see. Include:

* what an attacker can do, and from where (the local network, an app on the TV,
  a file or a pull request);
* the TV model, webOS version and firmware, and the Lumen commit or `.ipk`;
* how to reproduce it.

Please keep the details private until a fix is out. The fix and the advisory are
published together, crediting you unless you'd rather not be named. Lumen is
pre-alpha with no releases yet, so fixes land on `main` only.

## Scope

In scope: anything in this repository that runs on the TV or acts on it.

* the native shell (`shell/`) and the `.ipk` it ships in;
* boot hooks Lumen installs in `/var/lib/webosbrew/init.d`, and the kill switch
  they check;
* the takeover key filter (`takeover/`) and its configd entry;
* anything else Lumen puts on the TV, such as the ambient screensaver's bind
  mount;
* `tools/tv.sh` and `tools/recon.sh`, which log in to the TV as root over SSH,
  including the redaction of recon reports.

That a rooted TV runs code as root is the premise, not the bug; how Lumen uses
that access is in scope. [docs/safety.md](docs/safety.md) sets the rules Lumen
follows on the TV. A way to make it break them is worth reporting here. A change
that breaks one without giving anyone access, such as a boot hook that ignores
the kill switch, is a serious bug: open an ordinary issue for it.

Out of scope, and reported to their own projects:

* [Homebrew Channel](https://github.com/webosbrew/webos-homebrew-channel) and
  whichever exploit rooted the TV;
* [Glasshouse](https://github.com/rorygallagher2024/glasshouse) and
  [plx-native](https://github.com/GLinnik21/plx-native);
* LG firmware and webOS itself: [LG's product security team](https://lgsecurity.lge.com/).
