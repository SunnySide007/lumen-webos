// The long-Home key filter (takeover/filters/lumen-home.js) in a fake of LG's
// compositor: the objects its handlers use as far as LG's own filters show
// their shape (KeyPolicy, Qt's key codes, settingsService,
// applicationManager.launch, genericTimer, pmLog), with a clock the tests
// move by hand. Nothing reaches a TV.
//
//   node --test tools/tests/takeover-filter.test.js

"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const FILE = path.join(__dirname, "..", "..", "takeover", "filters", "lumen-home.js");
const SOURCE = fs.readFileSync(FILE, "utf8");
// The same one-line change tools/takeover.sh makes for --noop.
const NOOP = SOURCE.replace(/^var lumenHomeActs = true;$/m, "var lumenHomeActs = false;");

const LUMEN = "io.github.sunnyside007.lumen.shell";
const LG_HOME = "com.webos.app.home";
// Qt's key codes; the compositor's filters compare against Qt.Key_*.
const Qt = {
    Key_Super_L: 0x01000053,
    Key_Menu: 0x01000055,
    Key_Back: 0x01000061,
    Key_Escape: 0x01000000,
    Key_Left: 0x01000012,
    Key_Up: 0x01000013,
    Key_Right: 0x01000014,
    Key_Down: 0x01000015,
    Key_Return: 0x01000004,
    Key_VolumeUp: 0x01000072,
    Key_VolumeMute: 0x01000071,
    Key_0: 0x30,
    Key_1: 0x31,
};
const KeyPolicy = { NotAccepted: "NotAccepted", Accepted: "Accepted", NextPolicy: "NextPolicy" };

// A compositor with the filter loaded. `options` takes away or changes one
// object at a time.
function compositor(options = {}) {
    const launches = [];
    const logs = [];
    const timers = new Map();
    let now = 0;
    let nextId = 0;
    const genericTimer = {
        set(ms, callback) {
            const id = nextId++;
            timers.set(id, { at: now + ms, ms, callback });
            return id;
        },
        cancel(id) {
            timers.delete(id);
        },
        isRunning(id) {
            return timers.has(id);
        },
    };
    const sandbox = {
        Qt,
        KeyPolicy,
        settingsService: { getLaunchDefaultApp: LUMEN, disableHome: false },
        applicationManager: {
            launch(id, params) {
                launches.push([id, params]);
            },
        },
        genericTimer,
        pmLog: {
            info(id, detail) {
                logs.push([id, detail]);
            },
        },
    };
    for (const [name, value] of Object.entries(options.objects || {})) {
        if (value === undefined) {
            delete sandbox[name];
        } else {
            sandbox[name] = value;
        }
    }
    vm.createContext(sandbox);
    vm.runInContext(options.source || SOURCE, sandbox, { filename: "lumen-home.js" });
    return {
        sandbox,
        launches,
        logs,
        timers,
        key(code, pressed, autoRepeat = false) {
            return sandbox.lumenHomeKeys(code, pressed, autoRepeat);
        },
        // Moves the clock on, firing each timer that comes due.
        advance(ms) {
            now += ms;
            for (const [id, timer] of [...timers]) {
                if (timer.at <= now) {
                    timers.delete(id);
                    timer.callback();
                }
            }
        },
    };
}

const HOME = Qt.Key_Super_L;
const PRESS = true;
const RELEASE = false;

test("a short Home press goes on to LG's handler and opens nothing", () => {
    const tv = compositor();
    assert.equal(tv.key(HOME, PRESS), KeyPolicy.NextPolicy);
    tv.advance(400);
    assert.equal(tv.key(HOME, RELEASE), KeyPolicy.NextPolicy);
    tv.advance(5000);
    assert.deepEqual(tv.launches, []);
    assert.equal(tv.timers.size, 0);
});

test("a long Home press opens LG Home once, with activateType recent, and keeps the release", () => {
    const tv = compositor();
    assert.equal(tv.key(HOME, PRESS), KeyPolicy.NextPolicy);
    tv.advance(999);
    assert.deepEqual(tv.launches, []);
    tv.advance(1);
    assert.deepEqual(tv.launches, [[LG_HOME, JSON.stringify({ activateType: "recent" })]]);
    assert.equal(tv.key(HOME, RELEASE), KeyPolicy.Accepted);
    assert.equal(tv.launches.length, 1);
});

test("the press is timed for the compositor's long-press time, one second", () => {
    const tv = compositor();
    tv.key(HOME, PRESS);
    assert.deepEqual([...tv.timers.values()].map((t) => t.ms), [1000]);
});

test("repeats while Home is held start nothing new and LG Home opens once", () => {
    const tv = compositor();
    tv.key(HOME, PRESS);
    for (let i = 0; i < 5; i++) {
        tv.advance(150);
        assert.equal(tv.key(HOME, PRESS, true), KeyPolicy.NextPolicy);
    }
    assert.equal(tv.timers.size, 1);
    tv.advance(250);
    for (let i = 0; i < 10; i++) {
        tv.advance(150);
        assert.equal(tv.key(HOME, PRESS, true), KeyPolicy.NextPolicy);
    }
    assert.equal(tv.key(HOME, RELEASE), KeyPolicy.Accepted);
    assert.equal(tv.launches.length, 1);
});

test("the next press after a long one is timed afresh, and its short release goes on", () => {
    const tv = compositor();
    tv.key(HOME, PRESS);
    tv.advance(1200);
    tv.key(HOME, RELEASE);
    assert.equal(tv.key(HOME, PRESS), KeyPolicy.NextPolicy);
    tv.advance(300);
    assert.equal(tv.key(HOME, RELEASE), KeyPolicy.NextPolicy);
    tv.advance(2000);
    assert.equal(tv.launches.length, 1);
});

test("a second release without a press goes on and opens nothing", () => {
    const tv = compositor();
    tv.key(HOME, PRESS);
    tv.advance(1000);
    assert.equal(tv.key(HOME, RELEASE), KeyPolicy.Accepted);
    assert.equal(tv.key(HOME, RELEASE), KeyPolicy.NextPolicy);
    assert.equal(tv.launches.length, 1);
});

test("a new press before the release starts the timing again", () => {
    const tv = compositor();
    tv.key(HOME, PRESS);
    tv.advance(600);
    tv.key(HOME, PRESS);
    tv.advance(600);
    assert.deepEqual(tv.launches, []);
    tv.advance(400);
    assert.equal(tv.launches.length, 1);
});

test("every other key goes on untouched, pressed, held or released, even while Home is down", () => {
    const tv = compositor();
    const others = Object.entries(Qt).filter(([name]) => name !== "Key_Super_L");
    for (const [, code] of others) {
        assert.equal(tv.key(code, PRESS), KeyPolicy.NextPolicy);
        assert.equal(tv.key(code, PRESS, true), KeyPolicy.NextPolicy);
        assert.equal(tv.key(code, RELEASE), KeyPolicy.NextPolicy);
    }
    assert.equal(tv.timers.size, 0);
    tv.key(HOME, PRESS);
    for (const [, code] of others) {
        assert.equal(tv.key(code, PRESS), KeyPolicy.NextPolicy);
        assert.equal(tv.key(code, RELEASE), KeyPolicy.NextPolicy);
    }
    // Menu and Recent go to LG's Home handler too, but only Home is ours.
    tv.advance(3000);
    assert.equal(tv.key(Qt.Key_Menu, RELEASE), KeyPolicy.NextPolicy);
    assert.equal(tv.launches.length, 1);
    assert.deepEqual(tv.logs.map(([, d]) => d.what), ["first Home key", "long Home press: opened LG Home"]);
});

test("with LG's Home as the default, Home goes on and nothing is timed", () => {
    for (const home of [LG_HOME, "com.webos.app.fullhome", "", undefined]) {
        const tv = compositor({ objects: { settingsService: { getLaunchDefaultApp: home } } });
        assert.equal(tv.key(HOME, PRESS), KeyPolicy.NextPolicy);
        assert.equal(tv.timers.size, 0);
        tv.advance(3000);
        assert.equal(tv.key(HOME, RELEASE), KeyPolicy.NextPolicy);
        assert.deepEqual(tv.launches, []);
    }
});

test("when LG's handler ignores Home, so does Lumen's", () => {
    const cases = [
        { settingsService: { getLaunchDefaultApp: LUMEN, disableHome: true } },
        { modalAlert: { visible: true } },
        { bootManager: { bootFirstUse: true } },
    ];
    for (const objects of cases) {
        const tv = compositor({ objects });
        assert.equal(tv.key(HOME, PRESS), KeyPolicy.NextPolicy);
        tv.advance(3000);
        assert.equal(tv.key(HOME, RELEASE), KeyPolicy.NextPolicy);
        assert.deepEqual(tv.launches, [], JSON.stringify(objects));
    }
});

test("the default home changing during the press: nothing opens and the release goes on", () => {
    const tv = compositor();
    tv.key(HOME, PRESS);
    tv.sandbox.settingsService.getLaunchDefaultApp = LG_HOME;
    tv.advance(1000);
    assert.equal(tv.key(HOME, RELEASE), KeyPolicy.NextPolicy);
    assert.deepEqual(tv.launches, []);
});

test("without genericTimer, Home goes on and nothing throws", () => {
    const tv = compositor({ objects: { genericTimer: undefined } });
    assert.equal(tv.key(HOME, PRESS), KeyPolicy.NextPolicy);
    assert.equal(tv.key(HOME, RELEASE), KeyPolicy.NextPolicy);
    assert.deepEqual(tv.launches, []);
});

test("without settingsService, Home goes on", () => {
    const tv = compositor({ objects: { settingsService: undefined } });
    assert.equal(tv.key(HOME, PRESS), KeyPolicy.NextPolicy);
    tv.advance(2000);
    assert.equal(tv.key(HOME, RELEASE), KeyPolicy.NextPolicy);
});

test("a launch that throws still keeps the release, logs, and throws nothing", () => {
    const tv = compositor({
        objects: {
            applicationManager: {
                launch() {
                    throw new Error("SAM is busy");
                },
            },
        },
    });
    tv.key(HOME, PRESS);
    assert.doesNotThrow(() => tv.advance(1000));
    assert.equal(tv.key(HOME, RELEASE), KeyPolicy.Accepted);
    assert.match(tv.logs.at(-1)[1].what, /could not open LG Home/);
});

test("an error inside the handler lets the key through", () => {
    const settingsService = {};
    Object.defineProperty(settingsService, "getLaunchDefaultApp", {
        get() {
            throw new Error("not ready");
        },
    });
    const tv = compositor({ objects: { settingsService } });
    assert.equal(tv.key(HOME, PRESS), KeyPolicy.NextPolicy);
    assert.equal(tv.key(HOME, RELEASE), KeyPolicy.NextPolicy);
    assert.equal(tv.timers.size, 0);
    assert.match(tv.logs.at(-1)[1].what, /error/);
});

test("without Qt every key still goes on", () => {
    const tv = compositor({ objects: { Qt: undefined } });
    assert.equal(tv.key(0x01000053, PRESS), KeyPolicy.NextPolicy);
    assert.equal(tv.key(0x31, RELEASE), KeyPolicy.NextPolicy);
});

test("without pmLog nothing throws, and with it there is one line per action", () => {
    const quiet = compositor({ objects: { pmLog: undefined } });
    quiet.key(HOME, PRESS);
    quiet.advance(1000);
    assert.equal(quiet.key(HOME, RELEASE), KeyPolicy.Accepted);

    const tv = compositor();
    tv.key(HOME, PRESS);
    tv.key(HOME, RELEASE);
    tv.key(HOME, PRESS);
    tv.advance(1000);
    tv.key(HOME, RELEASE);
    assert.deepEqual(tv.logs.map(([id, d]) => [id, d.what]), [
        ["LUMEN_HOME_FILTER", "first Home key"],
        ["LUMEN_HOME_FILTER", "long Home press: opened LG Home"],
    ]);
});

test("the no-op build times the press, logs what it would do, and acts on nothing", () => {
    assert.notEqual(NOOP, SOURCE);
    const tv = compositor({ source: NOOP });
    assert.equal(tv.key(HOME, PRESS), KeyPolicy.NextPolicy);
    tv.advance(1000);
    assert.equal(tv.key(HOME, RELEASE), KeyPolicy.NextPolicy);
    assert.deepEqual(tv.launches, []);
    assert.deepEqual(tv.logs.map(([, d]) => [d.what, d.acts]), [
        ["first Home key", false],
        ["long Home press: would open LG Home (no-op build)", false],
    ]);
    for (const code of Object.values(Qt)) {
        assert.equal(tv.key(code, PRESS), KeyPolicy.NextPolicy);
        assert.equal(tv.key(code, RELEASE), KeyPolicy.NextPolicy);
    }
});

// What the TV's engines can't parse, outside comments. Strings in the file
// hold no "//".
function code(source) {
    return source.replace(/\/\/.*$/gm, "");
}

test("the file is ES5: no let, const, class, arrows, templates or spread", () => {
    const body = code(SOURCE);
    for (const pattern of [/\blet\b/, /\bconst\b/, /\bclass\b/, /=>/, /`/, /\.\.\./, /\basync\b/, /\bawait\b/]) {
        assert.doesNotMatch(body, pattern);
    }
    // The TV's check: new Function compiles it without running it.
    assert.doesNotThrow(() => new Function(SOURCE));
    assert.doesNotThrow(() => new Function(NOOP));
});

test("every top-level name starts with lumenHome or LUMEN_HOME, and there is no init", () => {
    const names = [...code(SOURCE).matchAll(/^(?:var|function)\s+([A-Za-z_$][\w$]*)/gm)].map((m) => m[1]);
    assert.ok(names.includes("lumenHomeKeys"));
    for (const name of names) {
        assert.match(name, /^(lumenHome|LUMEN_HOME_)/);
    }
    assert.doesNotMatch(code(SOURCE), /^function init\b/m);
});

test("the file reads nothing from disk and touches no other service", () => {
    const body = code(SOURCE);
    assert.doesNotMatch(body, /\b(require|XMLHttpRequest|Qt\.include|TextFile|readFile|lunaCall)\b/);
    assert.doesNotMatch(body, /\bsettingsService\.\w+\s*=[^=]/);
    const launches = [...body.matchAll(/applicationManager\.(\w+)/g)].map((m) => m[1]);
    assert.deepEqual(launches, ["launch"]);
});
