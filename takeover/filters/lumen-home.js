// Lumen's long-Home key filter (takeover/README.md, "The key filter").
//
// LG's compositor calls each handler listed in the configd value
// com.webos.surfacemanager.keyFilters, in order, as handler(key, pressed,
// autoRepeat). A handler answers with a KeyPolicy: NextPolicy hands the key
// to the next handler, Accepted keeps it, NotAccepted gives it to the app in
// front. tools/takeover.sh filter apply lists lumenHomeKeys just before LG's
// handleSystemKeys, which handles Home.
//
// LG's handler opens the default home on the release of a short Home press,
// and on a press held for a second does something only when the default is
// one of LG's two homes. With Lumen as the default (route D), that long press
// does nothing. This handler gives it a meaning: held for LONG_PRESS_MS, it
// opens LG's Home the way LG's handler does for its own (activateType
// "recent"), and keeps the release from LG's handler, which would otherwise
// open Lumen on it. Every other key, every short press, and every press while
// another app is the default home go on untouched, so LG's handler sees them
// as before. Any error lets the key through.
//
// ES5 only: the compositor's JavaScript engine (Qt's) and the TV's node 0.12,
// which checks the file before it is used, parse nothing newer. Nothing is
// read from disk; the only state is the press being timed. Every name starts
// with lumenHome, so nothing clashes with LG's handlers or the compositor's
// objects. The no-op build (tools/takeover.sh filter apply --noop) sets
// lumenHomeActs to false: it times presses and logs what it would do, and
// acts on nothing.

var lumenHomeActs = true;

var LUMEN_HOME_ID = "io.github.sunnyside007.lumen.shell";
var LUMEN_HOME_LG = "com.webos.app.home";
// The compositor's own long-press time for Home (systemUi.js).
var LUMEN_HOME_LONG_PRESS_MS = 1000;

// The press being timed: the timer's id while it runs, and whether it
// fired, which makes the release ours.
var lumenHomeTimer = -1;
var lumenHomeFired = false;
var lumenHomeSeen = false;

// One pmLog line, when the compositor gives filters pmLog.
function lumenHomeLog(what, detail) {
    try {
        if (typeof pmLog !== "undefined" && pmLog && typeof pmLog.info === "function") {
            pmLog.info("LUMEN_HOME_FILTER", {"what": what, "detail": detail || "",
                                             "acts": lumenHomeActs});
        }
    } catch (e) {
        // Logging never stops a key.
    }
}

function lumenHomeIsDefault() {
    return typeof settingsService !== "undefined" && !!settingsService &&
        settingsService.getLaunchDefaultApp === LUMEN_HOME_ID;
}

// The times LG's handler ignores Home: switched off, a modal alert, the
// first-use wizard. Lumen's handler leaves those to it.
function lumenHomeAllowed() {
    if (typeof settingsService !== "undefined" && settingsService &&
        settingsService.disableHome === true) {
        return false;
    }
    if (typeof modalAlert !== "undefined" && modalAlert && modalAlert.visible) {
        return false;
    }
    if (typeof bootManager !== "undefined" && bootManager && bootManager.bootFirstUse) {
        return false;
    }
    return true;
}

function lumenHomeCancel() {
    if (lumenHomeTimer !== -1) {
        try {
            genericTimer.cancel(lumenHomeTimer);
        } catch (e) {
            // A timer that is gone has nothing to cancel.
        }
    }
    lumenHomeTimer = -1;
}

// The timer fired: Home is still down after LUMEN_HOME_LONG_PRESS_MS.
function lumenHomeLongPress() {
    lumenHomeTimer = -1;
    if (!lumenHomeIsDefault()) {
        return;
    }
    lumenHomeFired = true;
    if (!lumenHomeActs) {
        lumenHomeLog("long Home press: would open LG Home (no-op build)");
        return;
    }
    try {
        applicationManager.launch(LUMEN_HOME_LG, JSON.stringify({"activateType": "recent"}));
        lumenHomeLog("long Home press: opened LG Home");
    } catch (e) {
        lumenHomeLog("long Home press: could not open LG Home", String(e));
    }
}

function lumenHomeKeys(key, pressed, autoRepeat) {
    try {
        if (key !== Qt.Key_Super_L) {
            return KeyPolicy.NextPolicy;
        }
        if (!lumenHomeSeen) {
            lumenHomeSeen = true;
            lumenHomeLog("first Home key");
        }
        if (pressed) {
            // A held key repeats; the timer from its first press runs on.
            if (autoRepeat) {
                return KeyPolicy.NextPolicy;
            }
            lumenHomeCancel();
            lumenHomeFired = false;
            if (lumenHomeIsDefault() && lumenHomeAllowed() &&
                typeof genericTimer !== "undefined" && genericTimer) {
                lumenHomeTimer = genericTimer.set(LUMEN_HOME_LONG_PRESS_MS, lumenHomeLongPress);
            }
            return KeyPolicy.NextPolicy;
        }
        // The release. After a long press it is ours, so LG's handler
        // doesn't open the default home on it as on a short press.
        if (lumenHomeFired) {
            lumenHomeFired = false;
            return lumenHomeActs ? KeyPolicy.Accepted : KeyPolicy.NextPolicy;
        }
        lumenHomeCancel();
        return KeyPolicy.NextPolicy;
    } catch (e) {
        lumenHomeCancel();
        lumenHomeFired = false;
        lumenHomeLog("error; the key goes on", String(e));
        return KeyPolicy.NextPolicy;
    }
}
