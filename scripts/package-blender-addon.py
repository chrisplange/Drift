#!/usr/bin/env python3
"""Zip the Drift profiles add-on for Blender as dist/drift_profiles-<version>.zip, the version
being Drift's (Cargo.toml), so the add-on and the app that writes its files go together.

    python scripts/package-blender-addon.py           write the zip
    python scripts/package-blender-addon.py --check   fail if the manifest's version isn't Drift's

The zip is a Blender extension (4.2 or later): the add-on's files at its top level, with
blender_manifest.toml. Install it in Blender from Edit → Preferences → Get Extensions →
Install from Disk.
"""

import os
import re
import sys
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ADDON = os.path.join(ROOT, "integrations", "blender", "drift_profiles")
MANIFEST = "blender_manifest.toml"
VERSION_LINE = re.compile(r'^version = "[^"]*"$', re.M)


def drift_version():
    with open(os.path.join(ROOT, "Cargo.toml"), encoding="utf-8") as f:
        text = f.read()
    section = text.split("[workspace.package]", 1)[1]
    return re.search(r'^version\s*=\s*"([^"]+)"', section, re.M).group(1)


def main():
    version = drift_version()
    with open(os.path.join(ADDON, MANIFEST), encoding="utf-8") as f:
        manifest = f.read()
    if len(VERSION_LINE.findall(manifest)) != 1:
        sys.exit("{}: expected one version line".format(MANIFEST))
    if "--check" in sys.argv[1:]:
        if 'version = "{}"'.format(version) not in manifest:
            sys.exit(
                "{} has another version than Drift's {}: run this script without --check".format(
                    MANIFEST, version
                )
            )
        return
    stamped = VERSION_LINE.sub('version = "{}"'.format(version), manifest)
    if stamped != manifest:
        # Keep the source in step too, so a release bump needs no hand edit.
        with open(os.path.join(ADDON, MANIFEST), "w", encoding="utf-8", newline="\n") as f:
            f.write(stamped)
    dist = os.path.join(ROOT, "dist")
    os.makedirs(dist, exist_ok=True)
    out = os.path.join(dist, "drift_profiles-{}.zip".format(version))
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        for name in sorted(os.listdir(ADDON)):
            path = os.path.join(ADDON, name)
            if os.path.isfile(path) and (name.endswith(".py") or name == MANIFEST):
                z.write(path, name)
    print(out)


if __name__ == "__main__":
    main()
