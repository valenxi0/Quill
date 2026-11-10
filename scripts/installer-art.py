"""Renders the installer's Welcome and Finish pictures and header icon from
src-tauri/windows/art/art.html into 24-bit .bmp files, which is what NSIS
loads. Needs Chrome and Pillow. Run from the repo root:

    python scripts/installer-art.py
"""
import os
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

ART = Path(__file__).resolve().parent.parent / "src-tauri" / "windows" / "art"
SIZES = {"welcome": (497, 314), "finish": (497, 314), "header": (150, 57)}
SCALES = {"1x": 1, "1.5x": 1.5, "2x": 2}
CHROME = [
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
]


def browser() -> str:
    for path in CHROME:
        if os.path.exists(path):
            return path
    sys.exit("Chrome or Edge is needed to render the installer art.")


def main() -> None:
    exe = browser()
    page = (ART / "art.html").as_uri()
    with tempfile.TemporaryDirectory() as tmp:
        for name, (width, height) in SIZES.items():
            for label, scale in SCALES.items():
                png = Path(tmp) / f"{name}-{label}.png"
                subprocess.run(
                    [
                        exe,
                        "--headless=new",
                        "--hide-scrollbars",
                        "--disable-gpu",
                        f"--user-data-dir={tmp}\\profile",
                        f"--window-size={width},{height}",
                        f"--force-device-scale-factor={scale}",
                        f"--screenshot={png}",
                        f"{page}#{name}",
                    ],
                    check=True,
                    capture_output=True,
                )
                out = ART / f"{name}@{label}.bmp"
                Image.open(png).convert("RGB").save(out)
                print(out.name, Image.open(out).size)


if __name__ == "__main__":
    main()
