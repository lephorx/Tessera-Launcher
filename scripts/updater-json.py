#!/usr/bin/env python3
"""Writes latest.json (what the launcher's updater reads) for a release from its signed
files, once all platforms have built. Run where `gh` is signed in:

    scripts/updater-json.py v0.5.2 > latest.json
"""
import json
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

REPO = "lephorx/Tessera-Launcher"
# updater platform keys → how the matching file name ends
PLATFORMS = {
    "darwin-aarch64": "_aarch64.app.tar.gz",
    "darwin-aarch64-app": "_aarch64.app.tar.gz",
    "darwin-x86_64": "_x64.app.tar.gz",
    "darwin-x86_64-app": "_x64.app.tar.gz",
    "windows-x86_64": "-setup.exe",
    "windows-x86_64-nsis": "-setup.exe",
    "windows-x86_64-msi": ".msi",
    "linux-x86_64": ".AppImage",
    "linux-x86_64-appimage": ".AppImage",
    "linux-x86_64-deb": ".deb",
    "linux-x86_64-rpm": ".rpm",
}


def main(tag: str) -> None:
    info = json.loads(subprocess.check_output(["gh", "release", "view", tag, "-R", REPO, "--json", "body,assets"]))
    names = [a["name"] for a in info["assets"]]
    with tempfile.TemporaryDirectory() as tmp:
        subprocess.check_call(["gh", "release", "download", tag, "-R", REPO, "-p", "*.sig", "-D", tmp], stdout=subprocess.DEVNULL)
        platforms = {}
        for key, ending in PLATFORMS.items():
            file = next((n for n in names if n.endswith(ending) and not n.endswith(".sig")), None)
            sig = Path(tmp) / f"{file}.sig" if file else None
            if file and sig.exists():
                platforms[key] = {
                    "signature": sig.read_text().strip(),
                    "url": f"https://github.com/{REPO}/releases/download/{tag}/{file}",
                }
    missing = sorted({k.split("-")[0] for k in PLATFORMS} - {k.split("-")[0] for k in platforms})
    if missing:
        sys.exit(f"no signed files for: {', '.join(missing)}")
    print(json.dumps({
        "version": tag.lstrip("v"),
        "notes": info["body"],
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }, indent=2))


if __name__ == "__main__":
    main(sys.argv[1])
