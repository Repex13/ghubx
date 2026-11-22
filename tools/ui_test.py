"""Scripted UI tests for GHUB-X: drive the real UI with synthetic egui input
(`ghubx.exe --dev --script`), then assert on the saved config and on pixels.

    python tools/ui_test.py [name ...]      # all scenarios, or the named ones

Nothing here touches the real mouse: the dev instance is observer-only, runs
off-screen, and uses throw-away config folders under %TEMP%\\ghubx_ui_test.
Every run starts from tools/ui_seed_config.toml; the coordinates below assume it.
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
RUN = TEMP / "ghubx_ui_test"
SEED = Path(__file__).resolve().parent / "ui_seed_config.toml"

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
    env = dict(os.environ, GHUBX_CONFIG_DIR=str(d / "cfg"))
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
                                       "click 471 515"])  # green accent swatch
    ok_run(res)
    must(res.cfg["appearance"].get("accent") == [68, 214, 44], res.cfg["appearance"])


@scenario
def appearance_toggles():
    res = run_script("appearance_toggles", ["wait 1500", GEAR, SETTLE, NAV_APPEARANCE, SETTLE,
                                            "click 952 556",  # Match mouse lighting switch
                                            "click 952 658"])  # Show DPI changes switch
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
                                                                 "click 200 673",  # USE MY COUNTER-STRIKE 2 SENSITIVITY (0.51)
                                                                 SETTLE], config=cs2_app_config())
    ok_run(res)
    must(prof(res.cfg, 1)["turn"]["counts"] == 16043, prof(res.cfg, 1).get("turn"))


@scenario
def click_assign():
    res = run_script("click_assign", device_assign() + [click(G9), SETTLE, TAB_ACTIONS, SETTLE,
                                                        "click 150 420",  # Perfect 180 card (below the collapsed OBS header)
                                                        SETTLE])
    ok_run(res)
    must(prof(res.cfg)["base"][8] == {"Perfect180": {"left": False}}, prof(res.cfg)["base"][8])


@scenario
def drag_assign():
    res = run_script("drag_assign", device_assign() + [TAB_ACTIONS, SETTLE,
                                                       "drag 150 452 %d %d" % G3,  # Perfect 180 (Left) card -> Middle Click marker
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


MACRO_NAME_T1 = ["text t1", "key Enter", SETTLE]  # name page: type the name, Enter
MACRO_NEW_EDIT = ["wait 1500", HOME_TILE, SETTLE, RAIL_ASSIGN, SETTLE, TAB_MACROS, SETTLE]
CONFIRM_YES = "click 442 637"  # confirmation panel YES
CONFIRM_NO = "click 582 637"  # confirmation panel NO
TILE = {"norepeat": "click 260 413", "repeat": "click 428 413", "toggle": "click 596 413", "sequence": "click 764 413"}  # type tiles
EDIT_CL = ["move 200 380", "click 310 380", SETTLE]  # macros list: hover the "cl" row, EDIT
EDIT_JUMP = ["move 200 412", "click 310 412", SETTLE]  # "jump" row, EDIT
M_SAVE = "click 620 720"  # editor SAVE
M_RESET = "click 404 720"  # editor RESET
M_BACK = "click 39 58"  # editor back arrow


def edit_macro(res, name):
    return [m for m in desktop(res.cfg)["macros"] if m["name"] == name]


def seed_macro(name):
    import tomllib as t
    return [m for m in t.loads(seed_config())["apps"][0]["macros"] if m["name"] == name][0]


def simple(steps):
    return [(s["vk"], s["down"], s["delay_ms"]) for s in steps]


@scenario
def macro_create_no_repeat():
    res = run_script("macro_create_no_repeat", MACRO_NEW + MACRO_NAME_T1 + [
        TILE["norepeat"], SETTLE,
        "click 119 476",  # "+" of the only section
        SETTLE, "click 220 344",  # menu: RECORD KEYSTROKES
        SETTLE, "key J", SETTLE,
        "click 241 476",  # STOP RECORDING (after two chips)
        SETTLE, M_SAVE, SETTLE])
    ok_run(res)
    m = edit_macro(res, "t1")
    must(len(m) == 1, [x["name"] for x in desktop(res.cfg)["macros"]])
    must(m[0]["kind"] == "NoRepeat" and m[0].get("std_delay") == 50, m[0])
    must(simple(m[0]["steps"]) == [(74, True, 0), (74, False, 50)], simple(m[0]["steps"]))


@scenario
def macro_sequence_sections():
    res = run_script("macro_sequence_sections", MACRO_NEW + ["text seq", "key Enter", SETTLE,
        TILE["sequence"], SETTLE,
        "click 119 409", SETTLE, "click 220 277",  # ON PRESS: "+", RECORD KEYSTROKES
        SETTLE, "key W", SETTLE, "click 241 409",  # STOP
        SETTLE, "click 119 554", SETTLE, "click 220 422",  # WHILE HOLDING: "+", RECORD
        SETTLE, "key A", SETTLE, "click 241 554",  # STOP
        SETTLE, "scroll 500 500 -300", SETTLE,
        "click 119 609", SETTLE, "click 190 521",  # ON RELEASE: "+", TEXT & EMOJIS
        SETTLE, "text hi", SETTLE, "click 424 609",  # DONE
        SETTLE, M_SAVE, SETTLE])
    ok_run(res)
    m = edit_macro(res, "seq")
    must(len(m) == 1, [x["name"] for x in desktop(res.cfg)["macros"]])
    m = m[0]
    must(m["kind"] == "Sequence", m)
    must(simple(m["steps"]) == [(87, True, 0), (87, False, 50)], simple(m["steps"]))
    must(simple(m["hold"]) == [(65, True, 0), (65, False, 50), (0, False, 50)], simple(m["hold"]))
    must(len(m["release"]) == 1 and m["release"][0]["act"] == {"Text": "hi"}, m["release"])


@scenario
def macro_switch_type():
    res = run_script("macro_switch_type", MACRO_NEW_EDIT + EDIT_JUMP + ["click 278 166", SETTLE, M_SAVE, SETTLE])
    ok_run(res)
    m = edit_macro(res, "jump")[0]
    want = seed_macro("jump")
    must(m["kind"] == "Toggle" and simple(m["steps"]) == simple(want["steps"]), m)


@scenario
def macro_custom_delay():
    res = run_script("macro_custom_delay", MACRO_NEW_EDIT + EDIT_CL + [
        "click 655 135",  # USE STANDARD DELAYS off
        SETTLE, "shot {shots}/custom.png",
        "dclick 158 470", SETTLE,  # first delay item: edit in place
        "key Backspace", "key Backspace", "key Backspace", "key Backspace", "text 120", "key Enter", SETTLE,
        M_SAVE, SETTLE])
    ok_run(res)
    m = edit_macro(res, "cl")[0]
    must("std_delay" not in m, m)
    must(m["steps"][1]["delay_ms"] == 120, simple(m["steps"]))


@scenario
def macro_delete_and_reset():
    res = run_script("macro_delete_and_reset", MACRO_NEW_EDIT + EDIT_JUMP + [
        "click 119 476",  # first chip
        SETTLE, "key Delete", SETTLE, "shot {shots}/deleted.png", M_RESET, SETTLE, M_BACK, SETTLE])
    ok_run(res)
    must(edit_macro(res, "jump")[0]["steps"] == seed_macro("jump")["steps"], edit_macro(res, "jump")[0])


@scenario
def macro_leave_confirm():
    for answer, name in ((CONFIRM_NO, "no"), (CONFIRM_YES, "yes")):
        res = run_script(f"macro_leave_confirm_{name}", MACRO_NEW_EDIT + EDIT_JUMP + [
            "click 119 476", SETTLE, "key Delete", SETTLE, M_BACK, SETTLE, "shot {shots}/ask.png",
            answer, SETTLE, "shot {shots}/ask2.png"] + ([CONFIRM_YES, SETTLE] if name == "yes" else []))
        ok_run(res)
        m = edit_macro(res, "jump")[0]
        if name == "no":
            must(m["steps"] == seed_macro("jump")["steps"], m)
        else:
            must(len(m["steps"]) == 2, m)


@scenario
def macro_system_step():
    res = run_script("macro_system_step", MACRO_NEW + MACRO_NAME_T1 + [
        TILE["norepeat"], SETTLE, "click 119 476", SETTLE,
        "click 220 520",  # menu: SYSTEM (fifth entry)
        SETTLE, "click 120 553",  # category list: Media
        SETTLE, "click 120 505",  # card: Play/Pause
        SETTLE, M_SAVE, SETTLE])
    ok_run(res)
    m = edit_macro(res, "t1")
    must(len(m) == 1 and len(m[0]["steps"]) == 1, m)
    must(m[0]["steps"][0]["act"]["Do"]["Shortcut"]["name"] == "Play/Pause", m[0]["steps"])


@scenario
def macro_fold_toggle():
    res = run_script("macro_fold_toggle", MACRO_NEW_EDIT + EDIT_JUMP + [
        "shot {shots}/before.png", "click 655 210",  # SHOW KEY DOWN/KEY UP off
        SETTLE, "shot {shots}/after.png", M_SAVE, SETTLE])
    ok_run(res)
    # second chip centre (the "Space" key-up chip): chip fill before, background after
    before, after = pixel(res, "before.png", (178, 470)), pixel(res, "after.png", (178, 470))
    must(before != (0, 0, 0) and after == (0, 0, 0), (before, after))
    m = edit_macro(res, "jump")[0]
    must(m.get("show_up_down") is False and simple(m["steps"]) == simple(seed_macro("jump")["steps"]), m)


OBS_HEADER = "click 230 341"  # ACTIONS tab: OBS section header (starts collapsed)


@scenario
def obs_assign():
    res = run_script("obs_assign", device_assign() + [click(G9), SETTLE, TAB_ACTIONS, SETTLE, OBS_HEADER, SETTLE,
                                                      "click 150 561",  # Toggle Recording card
                                                      SETTLE])
    ok_run(res)
    must(prof(res.cfg)["base"][8] == {"Obs": "ToggleRecord"}, prof(res.cfg)["base"][8])


@scenario
def obs_create_action():
    res = run_script("obs_create_action", device_assign() + [click(G9), SETTLE, TAB_ACTIONS, SETTLE, OBS_HEADER, SETTLE,
                                                             "scroll 230 500 -380", SETTLE,
                                                             "click 180 534",  # + CREATE NEW ACTION
                                                             SETTLE, "click 120 530",  # kind pill: SCENE
                                                             SETTLE, "click 200 595", "text Live",  # name field
                                                             SETTLE, "shot {shots}/form.png",
                                                             "click 147 632",  # ADD
                                                             SETTLE, "click 150 534",  # the new card: assign it to G9
                                                             SETTLE])
    ok_run(res)
    must(res.cfg.get("obs_actions") == [{"Scene": "Live"}], res.cfg.get("obs_actions"))
    must(prof(res.cfg)["base"][8] == {"Obs": {"Scene": "Live"}}, prof(res.cfg)["base"][8])


@scenario
def macro_obs_step():
    res = run_script("macro_obs_step", MACRO_NEW + MACRO_NAME_T1 + [
        TILE["norepeat"], SETTLE, "click 119 476", SETTLE,
        "click 200 432",  # menu: ACTION
        SETTLE, "click 120 508",  # integration list: OBS
        SETTLE, "scroll 150 560 -300", SETTLE,
        "click 127 513",  # action list: Save Buffer
        SETTLE, M_SAVE, SETTLE])
    ok_run(res)
    m = edit_macro(res, "t1")
    must(len(m) == 1 and len(m[0]["steps"]) == 1, m)
    must(m[0]["steps"][0]["act"] == {"Do": {"Obs": "SaveBuffer"}}, m[0]["steps"])


def fortnite_config(sens, counts):
    cfg = seed_config()
    for key, val in (("p180_counts", counts), ("p180_game", 5), ("p180_sens", sens)):
        cfg = re.sub(rf"(?m)^{key}\s*=.*$", f"{key} = {val}", cfg)
    return cfg


@scenario
def p180_fortnite_decimal():
    res = run_script("p180_fortnite_decimal", device_assign() + [TAB_ACTIONS, SETTLE, "scroll 230 500 -200", SETTLE,
                                                                 "click 252 486", "key End",  # IN-GAME SENS field
                                                                 "key Backspace", "key Backspace", "key Backspace", "key Backspace", "key Backspace",
                                                                 "text 0.138", "key Enter", SETTLE], config=fortnite_config(13.8, 2348))
    ok_run(res)
    ex = res.cfg["extras"]
    must(ex["p180_game"] == 5 and abs(ex["p180_sens"] - 0.138) < 0.0005 and ex["p180_counts"] == 2348, ex)


@scenario
def p180_fortnite_migrates():
    res = run_script("p180_fortnite_migrates", device_assign() + [TAB_ACTIONS, SETTLE, "scroll 230 500 -200", SETTLE,
                                                                  "shot {shots}/migrated.png", SETTLE], config=fortnite_config(0.138, 234806))
    ok_run(res)
    must(res.cfg["extras"]["p180_counts"] == 2348, res.cfg["extras"])


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
                                  "click 194 224",  # LOGO tab
                                  SETTLE, "click 110 300",  # EFFECT menu
                                  SETTLE, "shot {shots}/menu.png",
                                  "click 110 346",  # menu: FIXED
                                  SETTLE, "click 122 502",  # red preset swatch
                                  SETTLE, "shot {shots}/after.png"])
    ok_run(res)
    p = prof(res.cfg)
    l1, l0 = p["lighting"][1], p["lighting"][0]
    must(l1["effect"] == "Fixed" and l1["color"] == [255, 0, 0], l1)
    must(l0["effect"] == "Cycle", l0)


@scenario
def picker_matches_pixels():
    for i, (x, y) in enumerate([(150, 384), (300, 424)]):  # two points inside the SV square (92..370 x 339..456)
        name = f"picker_matches_pixels_{i}"
        res = run_script(name, ["wait 1500", HOME_TILE, SETTLE, RAIL_LIGHT, SETTLE,
                                "click 194 224",  # LOGO tab
                                SETTLE, "click 110 300",  # EFFECT menu
                                SETTLE, "click 110 346",  # menu: FIXED
                                SETTLE, "shot {shots}/before.png",
                                f"click {x} {y}",  # inside the SV square
                                SETTLE])
        ok_run(res)
        want = pixel(res, "before.png", (x, y))
        got = prof(res.cfg)["lighting"][1]["color"]
        must(all(abs(a - b) <= 6 for a, b in zip(got, want)), f"click ({x},{y}): saved {got}, pixel {want}")


LIGHT_LOGO_EFFECT_MENU = ["wait 1500", HOME_TILE, SETTLE, RAIL_LIGHT, SETTLE,
                          "click 194 224",  # LOGO tab
                          SETTLE, "click 110 300",  # EFFECT menu
                          SETTLE]


@scenario
def lighting_screen_sampler():
    res = run_script("lighting_screen_sampler", LIGHT_LOGO_EFFECT_MENU + [
        "click 110 418",  # menu: SCREEN SAMPLER
        SETTLE, "click 108 411",  # mini screen: top-left cell
        SETTLE, "click 227 494",  # COLOR BOOST slider at about 75 %
        SETTLE])
    ok_run(res)
    l1, l0 = prof(res.cfg)["lighting"][1], prof(res.cfg)["lighting"][0]
    must(l1["effect"] == "Screen" and l1["host"]["area"] == "TopLeft", l1)
    must(70 <= l1["host"]["boost"] <= 80, l1)
    must(l0["effect"] == "Cycle", l0)


@scenario
def lighting_audio_visualizer():
    res = run_script("lighting_audio_visualizer", LIGHT_LOGO_EFFECT_MENU + [
        "click 110 442",  # menu: AUDIO VISUALIZER
        SETTLE, "click 240 346",  # BACKGROUND COLOR tab
        SETTLE, "dclick 345 524", "text 101030", "key Enter",  # picker hex field
        SETTLE, "click 130 596",  # ADVANCED SETTINGS row
        SETTLE, "click 150 507",  # PULSE ON BASS ONLY
        SETTLE, "click 150 427",  # AUTO-ADJUST SENSITIVITY
        SETTLE])
    ok_run(res)
    l1 = prof(res.cfg)["lighting"][1]
    h = l1["host"]
    must(l1["effect"] == "Audio" and h["color2"] == [16, 16, 48] and h["bass_only"] is True and h["auto_sens"] is False, l1)


def sampler_seed_config() -> str:
    """Seed with one own sample; the LOGO zone's SCREEN SAMPLER uses its rectangle."""
    text = seed_config()
    i = text.rfind('[[apps.profiles.lighting]]')
    head, tail = text[:i], text[i:]
    tail = tail.replace('effect = "Cycle"', 'effect = "Screen"', 1)
    tail = tail.replace('brightness = 100', 'brightness = 100\n\n[apps.profiles.lighting.host]\narea = { Rect = [333, 333, 667, 667] }', 1)
    return head + tail + '\n[[samples]]\nname = "Custom sample 1"\nrect = [333, 333, 667, 667]\n'


EDIT_SAMPLES = "click 357 390"  # EDIT on the SAMPLE header line -> "Edit Samples"


@scenario
def sampler_add_sample():
    res = run_script("sampler_add_sample", LIGHT_LOGO_EFFECT_MENU + [
        "click 110 418",  # menu: SCREEN SAMPLER
        SETTLE, EDIT_SAMPLES,
        SETTLE, "shot {shots}/editor.png",
        "click 149 650",  # ADD NEW SAMPLE
        SETTLE, "click 39 58",  # back arrow
        SETTLE, "click 200 476",  # the new sample's row
        SETTLE])
    ok_run(res)
    l1 = prof(res.cfg)["lighting"][1]
    must(l1["effect"] == "Screen" and l1["host"]["area"] == {"Rect": [333, 333, 667, 667]}, l1)
    must(res.cfg["samples"] == [{"name": "Custom sample 1", "rect": [333, 333, 667, 667]}], res.cfg.get("samples"))


@scenario
def sampler_move_and_resize():
    res = run_script("sampler_move_and_resize", LIGHT_LOGO_EFFECT_MENU[:7] + [
        # the zone is already on SCREEN SAMPLER with the sample's rectangle (seed)
        EDIT_SAMPLES,
        SETTLE, "drag 512 394 612 394",  # drag the sample by its middle 100 px right
        SETTLE, "drag 745 470 795 519",  # drag its bottom-right handle
        SETTLE, "shot {shots}/editor.png",
        "click 39 58",  # back arrow
        SETTLE], config=sampler_seed_config())
    ok_run(res)
    r = res.cfg["samples"][0]["rect"]
    area = prof(res.cfg)["lighting"][1]["host"]["area"]
    must(400 < r[0] <= 500 and r[1] == 333, f"moved right: {r}")
    must(820 <= r[2] <= 890 and 740 <= r[3] <= 820, f"resized: {r}")
    must(all(0 <= v <= 1000 for v in r), r)
    must(area == {"Rect": r}, f"zone area {area} vs sample {r}")


@scenario
def sampler_delete():
    res = run_script("sampler_delete", LIGHT_LOGO_EFFECT_MENU[:7] + [
        EDIT_SAMPLES,
        SETTLE, "click 512 394",  # select the sample
        SETTLE, "key Delete",
        SETTLE, "click 39 58",  # back arrow
        SETTLE], config=sampler_seed_config())
    ok_run(res)
    must("samples" not in res.cfg, res.cfg.get("samples"))
    l1 = prof(res.cfg)["lighting"][1]
    must(l1["effect"] == "Screen" and l1.get("host", {}).get("area", "Full") == "Full", l1)


@scenario
def wdl_switch():
    open_light = ["wait 1500", HOME_TILE, SETTLE, RAIL_LIGHT, SETTLE]
    res = run_script("wdl_switch", open_light + [
        "click 346 174",  # WINDOWS SETTINGS switch: ON, panel goes inert
        SETTLE, "click 110 396",  # EFFECT value (inert)
        SETTLE, "click 110 426",  # where OFF would be if the menu had opened
        SETTLE, "click 346 174",  # switch OFF again
        SETTLE, "click 110 300",  # EFFECT value
        SETTLE, "click 110 346",  # menu: FIXED
        SETTLE])
    ok_run(res)
    l0 = prof(res.cfg)["lighting"][0]
    must(l0["effect"] == "Fixed", l0)
    res = run_script("wdl_switch_inert", open_light + [
        "click 346 174", SETTLE, "click 110 396", SETTLE, "click 110 426", SETTLE])
    ok_run(res)
    must(prof(res.cfg)["lighting"][0]["effect"] == "Cycle", prof(res.cfg)["lighting"][0])


@scenario
def effect_menu_order():
    names = ["Off", "Fixed", "Cycle", "Breathing", "Screen", "Audio"]
    for i, want in enumerate(names):
        res = run_script(f"effect_menu_order_{i}", ["wait 1500", HOME_TILE, SETTLE, RAIL_LIGHT, SETTLE,
                                                    "click 194 224",  # LOGO tab
                                                    SETTLE, "click 110 300", SETTLE, "click 110 346",  # menu: FIXED
                                                    SETTLE, "click 110 300", SETTLE,
                                                    f"click 110 {322 + 24 * i}",  # menu entry i
                                                    SETTLE])
        ok_run(res)
        got = prof(res.cfg)["lighting"][1]["effect"]
        must(got == want, f"entry {i}: expected {want}, saved {got}")


@scenario
def onboard_replace_confirm():
    res = run_script("onboard_replace_confirm", ["wait 1500", HOME_TILE, SETTLE,
                                                 "click 38 724",  # Device Settings gear
                                                 SETTLE, "click 710 392",  # slot 1 name
                                                 SETTLE, "click 640 483",  # menu: Desktop > Default
                                                 SETTLE, "shot {shots}/bar.png",
                                                 "click 949 740",  # CANCEL
                                                 SETTLE, "shot {shots}/gone.png"])
    ok_run(res)
    bar, gone = pixel(res, "bar.png", (30, 760)), pixel(res, "gone.png", (30, 760))
    must(bar != (0, 0, 0), f"no confirmation bar at (30,760): {bar}")
    must(gone == (0, 0, 0), f"bar still there after CANCEL: {gone}")
    must(res.cfg["apps"][0]["profiles"][0]["name"] == "Default", res.cfg["apps"][0]["profiles"][0])


@scenario
def dpi_led_checkbox():
    res = run_script("dpi_led_checkbox", ["wait 1500", HOME_TILE, SETTLE,
                                          "click 38 724",  # Device Settings gear
                                          SETTLE, "click 183 396",  # DPI LIGHTING > ALWAYS ON
                                          SETTLE])
    ok_run(res)
    must(res.cfg["general"].get("dpi_led_always_on") is True, res.cfg["general"])


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
                                         "click 183 670",  # FLASH LIGHTING ON DPI CHANGE
                                         "click 183 588",  # scroll acceleration ENABLE
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
                                     "click 921 629",  # ON-SCREEN DISPLAY > PREVIEW
                                     "wait 900"], during=_osd_hook)
    ok_run(res)
    must(res.hook["found"], "no visible GHUBX_OSD window owned by the script's process")


NEW_THEME_TILE = "click 442 395"  # MY THEMES > "+ NEW" tile (first slot)
THEME_NAME_END = "click 650 214"  # right end of the editor's NAME box (cursor goes to the end)
ROLE_BG = "click 500 303"  # editor colour rows: Background
ROLE_PANELS = "click 500 347"  # Panels
ROLE_ACCENT = "click 500 435"  # Accent
PICKER_HEX = "dclick 950 447"  # picker hex field (double-click selects the digits)
THEME_SAVE = "click 444 641"  # editor SAVE
THEME_CANCEL = "click 566 641"  # editor CANCEL
THEME_DELETE = "click 921 641"  # editor DELETE
MYTHEMES_EDIT = "click 778 333"  # MY THEMES > EDIT (active custom theme)
CUSTOM_SEED = ('\n[[appearance.custom]]\nid = "custom-test"\nname = "Neon"\nbg = [26, 16, 35]\n'
               'surface = [30, 20, 40]\ntext = [255, 255, 255]\naccent = [255, 122, 89]\n')


def custom_seed_config() -> str:
    text = re.sub(r'(?m)^theme\s*=.*$', 'theme = "custom-test"', seed_config())
    return text + CUSTOM_SEED


def open_appearance():
    return ["wait 1500", GEAR, SETTLE, NAV_APPEARANCE, SETTLE]


@scenario
def theme_custom_create():
    res = run_script("theme_custom_create", open_appearance() + [
        NEW_THEME_TILE, SETTLE, THEME_NAME_END] + ["key Backspace"] * 8 + ["text Neon",
        ROLE_BG, SETTLE, PICKER_HEX, "text 1A1023", "key Enter", SETTLE,
        ROLE_ACCENT, SETTLE, PICKER_HEX, "text FF7A59", "key Enter", SETTLE,
        "shot {shots}/editing.png",
        THEME_SAVE, "wait 600", "shot {shots}/saved.png"])
    ok_run(res)
    ap = res.cfg["appearance"]
    must(len(ap.get("custom", [])) == 1, ap)
    t = ap["custom"][0]
    must(t["name"] == "Neon" and t["bg"] == [26, 16, 35] and t["accent"] == [255, 122, 89], t)
    must(ap["theme"] == t["id"] and "accent" not in ap, ap)
    for shot in ("editing.png", "saved.png"):
        px = pixel(res, shot, (10, 400))
        must(px == (26, 16, 35), f"{shot}: background at (10,400) is {px}")


@scenario
def theme_custom_cancel():
    res = run_script("theme_custom_cancel", open_appearance() + [
        NEW_THEME_TILE, SETTLE, ROLE_BG, SETTLE, PICKER_HEX, "text 1A1023", "key Enter", SETTLE,
        THEME_CANCEL, "wait 600", "shot {shots}/after.png"])
    ok_run(res)
    ap = res.cfg["appearance"]
    must(not ap.get("custom") and ap["theme"] == "ghub-dark", ap)
    px = pixel(res, "after.png", (10, 400))
    must(px == (0, 0, 0), f"background at (10,400) is {px}")


@scenario
def theme_custom_edit_delete():
    res = run_script("theme_custom_edit", open_appearance() + [
        MYTHEMES_EDIT, SETTLE, ROLE_PANELS, SETTLE, PICKER_HEX, "text 2A1B3D", "key Enter", SETTLE,
        THEME_SAVE, SETTLE], config=custom_seed_config())
    ok_run(res)
    ap = res.cfg["appearance"]
    must(len(ap["custom"]) == 1 and ap["custom"][0]["surface"] == [42, 27, 61] and ap["custom"][0]["id"] == "custom-test", ap)
    res = run_script("theme_custom_delete", open_appearance() + [
        MYTHEMES_EDIT, SETTLE, THEME_DELETE, SETTLE, THEME_DELETE, SETTLE], config=custom_seed_config())
    ok_run(res)
    ap = res.cfg["appearance"]
    must(not ap.get("custom") and ap["theme"] == "ghub-dark", ap)


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
