"""Page screenshots for regression checks.

    python tools/shots.py render <out_dir> [--theme <id>] [page ...]
    python tools/shots.py diff <dir_a> <dir_b> [tolerance]

`render` draws every page (or the named ones) with `ghubx.exe --shot`, which
never touches the mouse, starting from tools/ui_seed_config.toml so the
user's live settings cannot change the result. Two renders of the same code
are pixel-identical, so `diff ... 0` after a change shows exactly which pages
it altered. Pages that print live device values (diag, home) can differ when
the mouse's state changed in between.
"""
import os
import shutil
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parent.parent
EXE = ROOT / "target" / "debug" / "ghubx.exe"
SEED = Path(__file__).resolve().parent / "ui_seed_config.toml"
PAGES = [
    "home", "dropdown", "assign", "assign2", "gshift", "select", "keys", "actions", "p180game", "gamecmds",
    "macro", "macroname", "macrotype", "macroseq", "macroedit", "macrorec", "macromenu", "macrocustom", "macrofold", "macroconfirm", "macroaction", "obsactions", "obsnew", "p180warn", "sens", "light", "fixed", "effectmenu", "screenfx", "screenfxmine", "sampleedit", "audiofx", "audiofxadv", "devset", "onboard", "slotmenu", "slotconfirm", "slotdetails", "wdl",
    "profiles", "appdetail", "addgame", "scriptedit", "settings", "sleep", "appearance", "themeedit", "mythemes", "ghub", "diag", "notify",
]


def render(out: Path, theme: str | None, pages: list[str]) -> int:
    out.mkdir(parents=True, exist_ok=True)
    cfg = Path(os.environ["TEMP"]) / "ghubx_shots_cfg"
    if cfg.exists():
        shutil.rmtree(cfg)
    (cfg / "ghubx").mkdir(parents=True)
    shutil.copy(SEED, cfg / "ghubx" / "config.toml")
    env = dict(os.environ, GHUBX_CONFIG_DIR=str(cfg))
    failed = 0
    for page in pages:
        cmd = [str(EXE), "--shot", page, str(out / f"{page}.png")]
        if theme:
            cmd += ["--theme", theme]
        try:
            rc = subprocess.run(cmd, env=env, timeout=60).returncode
        except subprocess.TimeoutExpired:
            rc = -1
        if rc != 0 or not (out / f"{page}.png").exists():
            print(f"FAILED {page} (exit {rc})")
            failed += 1
    print(f"rendered {len(pages) - failed}/{len(pages)} page(s) into {out}")
    return 1 if failed else 0


def diff(a: Path, b: Path, tol: int) -> int:
    bad = 0
    for pa in sorted(a.glob("*.png")):
        pb = b / pa.name
        if not pb.exists():
            print(f"{pa.name:16} MISSING in {b}")
            bad += 1
            continue
        ia, ib = Image.open(pa).convert("RGB"), Image.open(pb).convert("RGB")
        if ia.size != ib.size:
            print(f"{pa.name:16} SIZE {ia.size} vs {ib.size}")
            bad += 1
            continue
        d = ImageChops.difference(ia, ib)
        r, g, bl = d.split()
        worst = ImageChops.lighter(ImageChops.lighter(r, g), bl)
        over = worst.point(lambda v: 255 if v > tol else 0)
        n = over.histogram()[255]
        print(f"{pa.name:16} max={worst.getextrema()[1]:3d}  pixels>{tol}: {n:7d}  bbox={over.getbbox()}")
        bad += 1 if n else 0
    print("PAGES WITH DIFFERENCES:", bad)
    return 1 if bad else 0


def main() -> int:
    args = sys.argv[1:]
    if len(args) >= 2 and args[0] == "render":
        theme = None
        rest = args[2:]
        if "--theme" in rest:
            i = rest.index("--theme")
            theme = rest[i + 1]
            rest = rest[:i] + rest[i + 2:]
        unknown = [p for p in rest if p not in PAGES]
        if unknown:
            print("unknown page(s):", ", ".join(unknown))
            return 2
        return render(Path(args[1]), theme, rest or PAGES)
    if len(args) >= 3 and args[0] == "diff":
        return diff(Path(args[1]), Path(args[2]), int(args[3]) if len(args) > 3 else 0)
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main())
