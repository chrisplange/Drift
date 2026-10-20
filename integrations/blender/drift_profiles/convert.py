# SPDX-License-Identifier: GPL-3.0-or-later
"""Drift's profiles file, read and turned into Bézier spline points. No bpy here, so it can be
tested on its own.

The file is written by Drift beside a drawing (``Trim.drift`` → ``Trim.drift-profiles.json``)
when it is saved: one profile per block marked as a Blender profile, in the block's own
coordinates with its base point at the origin, in metres, y up. Each profile has loops of
cubic Bézier knots (``co``, ``left`` and ``right`` handles); closed loops come back to their
first knot.
"""

import json

FORMAT = "drift-profiles"
VERSION = 1
EXTENSION = ".drift-profiles.json"

#: The planes a profile can be laid in: XY to sweep along a curve (a bevel object), XZ to
#: lathe round Z (a Screw modifier), with the profile's x as the radius.
ORIENTATIONS = ("XY", "XZ")


class ProfileError(ValueError):
    """A file that isn't a Drift profiles file this add-on can read."""


def parse(text):
    """The profiles file's contents, checked."""
    try:
        data = json.loads(text)
    except json.JSONDecodeError as e:
        raise ProfileError("not a Drift profiles file: {}".format(e)) from None
    if not isinstance(data, dict) or data.get("format") != FORMAT:
        raise ProfileError("not a Drift profiles file")
    version = data.get("version")
    if not isinstance(version, int) or version < 1:
        raise ProfileError("not a Drift profiles file (no version)")
    if version > VERSION:
        raise ProfileError(
            "this profiles file is version {}: update the Drift profiles add-on".format(version)
        )
    if data.get("units") != "m":
        raise ProfileError("unknown units {!r}".format(data.get("units")))
    profiles = data.get("profiles")
    if not isinstance(profiles, list):
        raise ProfileError("the file has no profiles")
    for p in profiles:
        if not isinstance(p.get("id"), str) or not isinstance(p.get("loops"), list):
            raise ProfileError("a profile without an id or loops")
        for loop in p["loops"]:
            knots = loop.get("knots")
            if not isinstance(knots, list) or not knots:
                raise ProfileError("profile {} has a loop with no knots".format(p["id"]))
    return data


def read(path):
    """Read and check the profiles file at ``path``."""
    with open(path, "r", encoding="utf-8") as f:
        return parse(f.read())


def find(data, profile_id):
    """The profile with this id, or None (its block was deleted or unmarked in Drift)."""
    for p in data["profiles"]:
        if p["id"] == profile_id:
            return p
    return None


def is_lathe(profile):
    """Whether a profile looks like half of a turned shape: only open loops."""
    return bool(profile["loops"]) and not any(loop["closed"] for loop in profile["loops"])


def place(point, orientation, unit_scale=1.0):
    """A profile point (metres) in Blender units, in the profile's plane."""
    x, y = point[0] / unit_scale, point[1] / unit_scale
    if orientation == "XZ":
        return (x, 0.0, y)
    return (x, y, 0.0)


def splines(profile, orientation="XY", unit_scale=1.0):
    """A profile's loops as Bézier splines: dicts of ``cyclic`` and ``points``, each point a
    dict of ``co``, ``left`` and ``right`` in Blender units.

    ``unit_scale`` is the scene's unit scale (metres per Blender unit).
    """
    if orientation not in ORIENTATIONS:
        raise ValueError("orientation must be one of {}".format(ORIENTATIONS))
    if unit_scale <= 0.0:
        unit_scale = 1.0
    out = []
    for loop in profile["loops"]:
        out.append(
            {
                "cyclic": bool(loop["closed"]),
                "points": [
                    {
                        "co": place(k["co"], orientation, unit_scale),
                        "left": place(k["left"], orientation, unit_scale),
                        "right": place(k["right"], orientation, unit_scale),
                    }
                    for k in loop["knots"]
                ],
            }
        )
    return out
