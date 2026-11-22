"""Scripted UI tests for GHUB-X: drive the real UI with synthetic egui input
(`ghubx.exe --dev --script`), then assert on the saved config and on pixels.

    python tools/ui_test.py [name ...]      # all scenarios, or the named ones

Nothing here touches the real mouse: the dev instance is observer-only, runs
off-screen, and uses throw-away config folders under %TEMP%\\ghubx_b7\\run.
"""
import ctypes
import os
import re
import shutil
import subprocess
import sys
import time
import tomllib
from ctypes import wintypes as w
from pathlib import Path

from PIL import Image

TEMP = Path(os.environ["TEMP"])
EXE = Path(__file__).resolve().parent.parent / "target" / "debug" / "ghubx.exe"
RUN = TEMP / "ghubx_b7" / "run"
SEED = TEMP / "ghubx_theme" / "cfg" / "ghubx" / "config.toml"
CACHE = TEMP / "ghubx_b4" / "cache"

CS2_ID = "ca572e6a-c76d-4e34-bd64-3665c0d0b0c1"
CS2_DIR = r"C:\\Program Files (x86)\\Steam\\steamapps\\common\\Counter-Strike Global Offensive"

# ---- screen coordinates (1024x768 window), one name per control -------------
GEAR = "click 914 59"  # header gear -> Settings
NAV_GENERAL = "click 150 135"
NAV_APPEARANCE = "click 150 187"
NAV_GHUB = "click 150 239"
HOME_NAV_DEVICES = "click 145 59"
HOME_NAV_PROFILES = "click 228 59"
# device pages: left rail icons
RAIL_SENS = "click 38 140"
RAIL_ASSIGN = "click 38 192"
RAIL_LIGHT = "click 38 244"
# assignments tabs
TAB_COMMANDS = "click 136 178"
TAB_KEYS = "click 214 178"
TAB_ACTIONS = "click 280 178"
TAB_MACROS = "click 123 208"
# marker dots on the mouse render (VIEW 1)
G9 = (725, 389)  # Onboard Profile Cycle
G3 = (720, 314)  # Middle Click
G2 = (764, 277)  # Secondary Click
SETTLE = "wait 400"


def seed_config(extra: str = "", profile_switching_off: bool = True) -> str:
    text = SEED.read_text(encoding="utf-8")
    if profile_switching_off:
        text = re.sub(r"(?m)^profile_switching\s*=.*$", "profile_switching = false", text)
    return text + extra


def cs2_app_config() -> str:
    """Seed with a second app, Counter-Strike 2, selected for editing."""
    text = seed_config()
    text = re.sub(r"(?m)^current_app\s*=.*$", "current_app = 1", text)
    app = (
        f'\n[[apps]]\nname = "Counter-Strike 2"\nghub_id = "{CS2_ID}"\ndir = "{CS2_DIR}"\nenabled = true\nactive_profile = 0\n'
        '\n[[apps.profiles]]\nname = "Default"\n'
    )
    text += app
    return text


class Result:
    def __init__(self, cfg, rc, shots, out):
        self.cfg, self.rc, self.shots, self.out = cfg, rc, shots, out


def run_script(name: str, lines: list[str], config: str | None = None, timeout=60, during=None) -> Result:
    d = RUN / name
    if d.exists():
        shutil.rmtree(d)
    cfg_dir = d / "cfg" / "ghubx"
    cfg_dir.mkdir(parents=True)
    (cfg_dir / "config.toml").write_text(config if config is not None else seed_config(), encoding="utf-8")
    shots = d / "shots"
    shots.mkdir()
    body = [ln.replace("{shots}", str(shots).replace("\\", "/")) for ln in lines]
    (d / "script.txt").write_text("\n".join(body + ["quit"]) + "\n", encoding="utf-8")
    env = dict(os.environ, GHUBX_CONFIG_DIR=str(d / "cfg"), GHUBX_CACHE_DIR=str(CACHE))
    p = subprocess.Popen([str(EXE), "--dev", "--script", str(d / "script.txt")], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    hook = during(p) if during else None
    try:
        out, _ = p.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        p.kill()
        out = "TIMEOUT\n" + (p.communicate()[0] or "")
        return Result(None, -1, shots, out)
    cfg_path = cfg_dir / "config.toml"
    cfg = tomllib.loads(cfg_path.read_text(encoding="utf-8")) if cfg_path.exists() else None
    res = Result(cfg, p.returncode, shots, out)
    res.hook = hook
    return res


def desktop(cfg, i=0):
    return cfg["apps"][i]


def prof(cfg, app=0, p=0):
    return cfg["apps"][app]["profiles"][p]


def pixel(res: Result, name: str, xy):
    return Image.open(res.shots / name).convert("RGB").getpixel(xy)


def click(xy):
    return f"click {xy[0]} {xy[1]}"


# ------------------------------------------------------------------ scenarios
SCENARIOS = {}


def scenario(fn):
    SCENARIOS[fn.__name__] = fn
    return fn


def must(cond, msg):
    if not cond:
        raise AssertionError(msg)


def ok_run(res: Result):
    must(res.rc == 0 and res.cfg is not None, f"exit code {res.rc}\n{res.out[-400:]}")


@scenario
def theme_tile():
    res = run_script("theme_tile", ["wait 1500", GEAR, SETTLE, NAV_APPEARANCE, SETTLE,
                                    "click 802 245",  # Midnight tile
                                    "wait 500", "shot {shots}/after.png"])
    ok_run(res)
    must(res.cfg["appearance"]["theme"] == "midnight", res.cfg.get("appearance"))
    px = pixel(res, "after.png", (10, 400))
    must(px == (0x09, 0x0B, 0x14), f"background at (10,400) is {px}")


@scenario
def accent_swatch():
    res = run_script("accent_swatch", ["wait 1500", GEAR, SETTLE, NAV_APPEARANCE, SETTLE,
                                       "click 471 365"])  # green accent swatch
    ok_run(res)
    must(res.cfg["appearance"].get("accent") == [68, 214, 44], res.cfg["appearance"])


@scenario
def appearance_toggles():
    res = run_script("appearance_toggles", ["wait 1500", GEAR, SETTLE, NAV_APPEARANCE, SETTLE,
                                            "click 952 406",  # Match mouse lighting switch
                                            "click 952 643"])  # Show DPI changes switch
    ok_run(res)
    must(res.cfg["appearance"].get("follow_lighting") is True, res.cfg["appearance"])
    must(res.cfg["general"].get("osd_dpi") is False, res.cfg["general"])


@scenario
def general_toggle():
    res = run_script("general_toggle", ["wait 1500", GEAR, SETTLE, NAV_GENERAL, SETTLE,
                                        "click 952 236"])  # Close to tray switch
    ok_run(res)
    must(res.cfg["general"].get("close_to_tray") is False, res.cfg["general"])


@scenario
def ghub_tab():
    res = run_script("ghub_tab", ["wait 1500", GEAR, SETTLE, NAV_GHUB, SETTLE,
                                  "click 952 240"])  # Take control when G HUB is closed switch
    ok_run(res)
    must(res.cfg["general"].get("take_control") is False, res.cfg["general"])


HOME_TILE = "click 512 300"  # device tile on Devices (opens the device pages)


def device_assign():
    return ["wait 1500", HOME_TILE, SETTLE, RAIL_ASSIGN, SETTLE]


@scenario
def add_detected_game():
    res = run_script("add_detected_game", ["wait 1500", HOME_NAV_PROFILES, SETTLE,
                                           "click 773 262",  # ADD GAMES & APPS
                                           "wait 2500",
                                           "click 910 385",  # ADD on the Counter-Strike 2 row
                                           SETTLE])
    ok_run(res)
    apps = [a for a in res.cfg["apps"] if a["name"] == "Counter-Strike 2"]
    must(len(apps) == 1, [a["name"] for a in res.cfg["apps"]])
    a = apps[0]
    must(a.get("ghub_id") == CS2_ID, a.get("ghub_id"))
    must(a["dir"].endswith("Counter-Strike Global Offensive"), a["dir"])
    must(res.cfg["profile_switching"] is True, "profile_switching")
    t = a["profiles"][0]["turn"]
    must(t["counts"] == 16043 and t["game"] == 0 and abs(t["sens"] - 0.51) < 0.001, t)


@scenario
def use_game_sensitivity():
    res = run_script("use_game_sensitivity", device_assign() + [TAB_ACTIONS, "wait 800",
                                                                 "click 200 631",  # USE MY COUNTER-STRIKE 2 SENSITIVITY (0.51)
                                                                 SETTLE], config=cs2_app_config())
    ok_run(res)
    must(prof(res.cfg, 1)["turn"]["counts"] == 16043, prof(res.cfg, 1).get("turn"))


@scenario
def click_assign():
    res = run_script("click_assign", device_assign() + [click(G9), SETTLE, TAB_ACTIONS, SETTLE,
                                                        "click 150 378",  # Perfect 180 card
                                                        SETTLE])
    ok_run(res)
    must(prof(res.cfg)["base"][8] == {"Perfect180": {"left": False}}, prof(res.cfg)["base"][8])


@scenario
def drag_assign():
    res = run_script("drag_assign", device_assign() + [TAB_ACTIONS, SETTLE,
                                                       "drag 150 410 %d %d" % G3,  # Perfect 180 (Left) card -> Middle Click marker
                                                       SETTLE])
    ok_run(res)
    must(prof(res.cfg)["base"][2] == {"Perfect180": {"left": True}}, prof(res.cfg)["base"][2])


@scenario
def gshift_assign():
    res = run_script("gshift_assign", device_assign() + ["click 708 718",  # G-SHIFT switch
                                                         SETTLE, click(G2), SETTLE, TAB_KEYS, SETTLE,
                                                         "click 230 293", "text q", SETTLE,  # search field: only Q matches
                                                         "shot {shots}/search.png",
                                                         "click 120 390",  # first (and only) card: the Q key
                                                         SETTLE, "shot {shots}/keys.png"])
    ok_run(res)
    p = prof(res.cfg)
    must(p["gshift"][1] == {"Key": {"vk": 81, "mods": 0}}, p["gshift"][1])  # the Q key card
    must(p["base"][1] == "Default", p["base"][1])
    print("   gshift[1] =", p["gshift"][1])


@scenario
def disable_and_restore():
    res = run_script("disable_a", device_assign() + [f"rclick {G9[0]} {G9[1]}", SETTLE, "shot {shots}/menu.png",
                                                     f"click {G9[0] + 40} {G9[1] + 58}",  # menu: Disabled
                                                     SETTLE])
    ok_run(res)
    must(prof(res.cfg)["base"][8] == "Disabled", prof(res.cfg)["base"][8])
    res = run_script("disable_b", device_assign() + [f"rclick {G9[0]} {G9[1]}", SETTLE,
                                                     f"click {G9[0] + 40} {G9[1] + 58}", SETTLE,  # Disabled
                                                     f"rclick {G9[0]} {G9[1]}", SETTLE,
                                                     f"click {G9[0] + 40} {G9[1] + 22}",  # menu: Use default
                                                     SETTLE])
    ok_run(res)
    must(prof(res.cfg)["base"][8] == "Default", prof(res.cfg)["base"][8])


MACRO_NEW = ["wait 1500", HOME_TILE, SETTLE, RAIL_ASSIGN, SETTLE, TAB_MACROS, SETTLE,
             "click 230 338",  # CREATE NEW MACRO
             SETTLE]


@scenario
def macro_create():
    res = run_script("macro_create", MACRO_NEW + ["click 200 281", "text t1",  # name field
                                                  "click 124 433", SETTLE,  # + KEY
                                                  "key J", SETTLE,
                                                  "click 266 433",  # + DELAY
                                                  "shot {shots}/before_save.png",
                                                  "click 135 716",  # SAVE
                                                  SETTLE])
    ok_run(res)
    m = [m for m in desktop(res.cfg)["macros"] if m["name"] == "t1"]
    must(len(m) == 1, [m["name"] for m in desktop(res.cfg)["macros"]])
    steps = [(s["vk"], s["down"], s["delay_ms"]) for s in m[0]["steps"]]
    must(steps == [(74, True, 0), (74, False, 50), (0, False, 50)], steps)


@scenario
def macro_text_and_edit():
    res = run_script("macro_text", MACRO_NEW + ["click 337 433", SETTLE,  # + TEXT
                                                "text aB", "key Enter", SETTLE,
                                                "move 200 627", SETTLE,  # hover the last step row
                                                "move 308 627", SETTLE, "shot {shots}/hover.png",  # onto its up chevron
                                                "click 308 627",  # its up chevron
                                                SETTLE, "shot {shots}/rows.png",
                                                "click 135 716", SETTLE])  # SAVE
    ok_run(res)
    m = [m for m in desktop(res.cfg)["macros"] if m["name"] == "Macro"]
    must(len(m) == 1, [m["name"] for m in desktop(res.cfg)["macros"]])
    steps = [(s["vk"], s["down"], s["delay_ms"]) for s in m[0]["steps"]]
    want = [(0x41, True, 10), (0x41, False, 10), (0xA0, True, 10), (0x42, True, 10), (0xA0, False, 10), (0x42, False, 10)]
    must(steps == want, steps)


@scenario
def sensitivity():
    res = run_script("sensitivity", ["wait 1500", HOME_TILE, SETTLE, RAIL_SENS, SETTLE,
                                     "click 156 266",  # 1000 speed in DPI SPEEDS
                                     SETTLE,
                                     "click 163 500",  # 500 report-rate radio
                                     SETTLE])
    ok_run(res)
    p = prof(res.cfg)
    must(p["dpi"]["active"] == 1000 and p["rate_ms"] == 2, (p["dpi"].get("active"), p["rate_ms"]))


@scenario
def lighting():
    res = run_script("lighting", ["wait 1500", HOME_TILE, SETTLE, RAIL_LIGHT, SETTLE,
                                  "click 194 320",  # LOGO tab
                                  SETTLE, "click 110 396",  # EFFECT menu
                                  SETTLE, "shot {shots}/menu.png",
                                  "click 110 430",  # menu: FIXED
                                  SETTLE, "click 122 598",  # red preset swatch
                                  SETTLE, "shot {shots}/after.png"])
    ok_run(res)
    p = prof(res.cfg)
    l1, l0 = p["lighting"][1], p["lighting"][0]
    must(l1["effect"] == "Fixed" and l1["color"] == [255, 0, 0], l1)
    must(l0["effect"] == "Cycle", l0)


@scenario
def profiles_crud():
    res = run_script("profiles_crud", ["wait 1500", HOME_NAV_PROFILES, SETTLE,
                                       "click 498 400",  # Desktop tile
                                       SETTLE,
                                       "click 481 324",  # + ADD PROFILE
                                       SETTLE,
                                       "click 854 272",  # DUPLICATE on the first profile row
                                       SETTLE, "shot {shots}/after.png"])
    ok_run(res)
    must(len(desktop(res.cfg)["profiles"]) == 3, [p["name"] for p in desktop(res.cfg)["profiles"]])


@scenario
def device_settings():
    res = run_script("device_settings", ["wait 1500", HOME_TILE, SETTLE,
                                         "click 38 724",  # Device Settings gear
                                         SETTLE,
                                         "click 183 568",  # FLASH LIGHTING ON DPI CHANGE
                                         "click 183 481",  # scroll acceleration ENABLE
                                         SETTLE])
    ok_run(res)
    must(res.cfg["general"].get("dpi_flash") is True and res.cfg["extras"].get("scroll_accel") is True, (res.cfg["general"], res.cfg["extras"]))


def _osd_hook(p):
    u = ctypes.WinDLL("user32")
    u.FindWindowW.argtypes = [w.LPCWSTR, w.LPCWSTR]
    u.FindWindowW.restype = w.HWND
    u.IsWindowVisible.argtypes = [w.HWND]
    u.GetWindowThreadProcessId.argtypes = [w.HWND, ctypes.POINTER(w.DWORD)]
    seen = {"found": False}
    deadline = time.time() + 30
    while p.poll() is None and time.time() < deadline and not seen["found"]:
        h = u.FindWindowW("GHUBX_OSD", None)
        if h and u.IsWindowVisible(h):
            pid = w.DWORD()
            u.GetWindowThreadProcessId(h, ctypes.byref(pid))
            seen["found"] = pid.value == p.pid
        time.sleep(0.02)
    return seen


@scenario
def osd_preview():
    res = run_script("osd_preview", ["wait 1500", GEAR, SETTLE, NAV_APPEARANCE, SETTLE,
                                     "click 921 613",  # ON-SCREEN DISPLAY > PREVIEW
                                     "wait 900"], during=_osd_hook)
    ok_run(res)
    must(res.hook["found"], "no visible GHUBX_OSD window owned by the script's process")


def main():
    names = sys.argv[1:] or list(SCENARIOS)
    failed = 0
    t0 = time.time()
    for n in names:
        t = time.time()
        try:
            SCENARIOS[n]()
            print(f"PASS {n} ({time.time() - t:.1f}s)", flush=True)
        except Exception as e:  # noqa: BLE001
            failed += 1
            print(f"FAIL {n}: {e}", flush=True)
    print(f"{len(names) - failed}/{len(names)} passed in {time.time() - t0:.0f}s")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
