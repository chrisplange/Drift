# SPDX-License-Identifier: GPL-3.0-or-later
"""Profile curves in the scene: making them, and reloading them in place from Drift's file."""

import os

import bpy

from . import convert

# Custom properties on each profile object.
SOURCE = "drift_source"
PROFILE_ID = "drift_profile_id"
ORIENTATION = "drift_orientation"
NAME = "drift_profile_name"
# The file's modification time (ns, as text) when the curve was last loaded from it.
MTIME = "drift_mtime"
# Set while the profile is no longer in the file (deleted or unmarked in Drift).
MISSING = "drift_missing"

GONE = "gone"


def unit_scale(scene):
    return scene.unit_settings.scale_length or 1.0


def mtime(path):
    """The file's modification time as text, or ``GONE``."""
    try:
        return str(os.stat(path).st_mtime_ns)
    except OSError:
        return GONE


def source_path(obj):
    return os.path.normpath(bpy.path.abspath(obj[SOURCE]))


def is_profile(obj):
    return obj is not None and obj.type == "CURVE" and SOURCE in obj and PROFILE_ID in obj


def profile_objects(objects=None):
    """The profile curves among ``objects`` (all of the file's, by default)."""
    if objects is None:
        objects = bpy.data.objects
    return [o for o in objects if is_profile(o)]


def fill(curve, splines):
    """Replace the curve's splines, leaving the curve and everything else about it as is."""
    curve.splines.clear()
    for s in splines:
        spline = curve.splines.new("BEZIER")
        points = s["points"]
        spline.bezier_points.add(len(points) - 1)
        for bp, p in zip(spline.bezier_points, points):
            bp.handle_left_type = "FREE"
            bp.handle_right_type = "FREE"
            bp.co = p["co"]
            bp.handle_left = p["left"]
            bp.handle_right = p["right"]
        spline.use_cyclic_u = s["cyclic"]
    curve.update_tag()


def new_object(context, path, profile, orientation, stamp):
    """A curve object for a profile, its origin at the profile's base point, at the 3D cursor."""
    curve = bpy.data.curves.new(profile["name"], "CURVE")
    if orientation == "XY":
        # Filled, so holes show and the profile reads as a section.
        curve.dimensions = "2D"
        curve.fill_mode = "BOTH"
    else:
        curve.dimensions = "3D"
    fill(curve, convert.splines(profile, orientation, unit_scale(context.scene)))
    obj = bpy.data.objects.new(profile["name"], curve)
    obj.location = context.scene.cursor.location
    obj[SOURCE] = os.path.abspath(path)
    obj[PROFILE_ID] = profile["id"]
    obj[ORIENTATION] = orientation
    obj[NAME] = profile["name"]
    obj[MTIME] = stamp
    obj[MISSING] = False
    context.collection.objects.link(obj)
    return obj


def reload(objects, scene):
    """Reload profile objects from their files, in place: only the curve's splines change, so
    the object, its transform, modifiers and materials, and whatever uses it (a bevel object,
    a Screw modifier) stay as they are.

    Returns ``(updated, missing, errors)``: objects updated, objects whose profile is no
    longer in its file, and messages for files that couldn't be read.
    """
    by_file = {}
    for obj in profile_objects(objects):
        by_file.setdefault(source_path(obj), []).append(obj)
    updated, missing, errors = [], [], []
    scale = unit_scale(scene)
    for path, objs in sorted(by_file.items()):
        stamp = mtime(path)
        try:
            data = convert.read(path)
        except FileNotFoundError:
            # Drift deletes the file when no block is a profile any more.
            for obj in objs:
                obj[MISSING] = True
                obj[MTIME] = stamp
            missing.extend(objs)
            continue
        except (OSError, convert.ProfileError) as e:
            errors.append("{}: {}".format(os.path.basename(path), e))
            continue
        for obj in objs:
            obj[MTIME] = stamp
            profile = convert.find(data, obj[PROFILE_ID])
            if profile is None:
                obj[MISSING] = True
                missing.append(obj)
                continue
            orientation = obj.get(ORIENTATION, "XY")
            if orientation not in convert.ORIENTATIONS:
                orientation = "XY"
            fill(obj.data, convert.splines(profile, orientation, scale))
            # Renamed in Drift: follow, unless the object was renamed here.
            if obj.get(NAME) != profile["name"]:
                if obj.name == obj.get(NAME):
                    obj.name = profile["name"]
                obj[NAME] = profile["name"]
            obj[MISSING] = False
            updated.append(obj)
    return updated, missing, errors


def changed(objects=None):
    """The profile objects whose file has changed since they were loaded."""
    return [o for o in profile_objects(objects) if mtime(source_path(o)) != o.get(MTIME)]
