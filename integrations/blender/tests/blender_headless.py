"""The add-on in a real Blender, without a window:

    blender --background --factory-startup --python-exit-code 1 --python integrations/blender/tests/blender_headless.py

Imports the fixture, sweeps a rail with one profile and lathes the other, rewrites the file as
Drift would on saving, and checks that Auto-reload changes the curves in place: the sweep's
bevel object, the Screw modifier and the transforms survive, and the swept shape follows.
"""

import json
import os
import shutil
import sys
import tempfile
import traceback

import bpy

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, ".."))

import drift_profiles  # noqa: E402
from drift_profiles import curves, ui  # noqa: E402

FIXTURE = os.path.join(HERE, "fixtures", "Trim.drift-profiles.json")


def extent(obj):
    """The evaluated object's size along each axis."""
    dg = bpy.context.evaluated_depsgraph_get()
    dg.update()
    ev = obj.evaluated_get(dg)
    mesh = ev.to_mesh()
    try:
        vs = [v.co for v in mesh.vertices]
        assert vs, "{} has no mesh".format(obj.name)
        return [max(v[i] for v in vs) - min(v[i] for v in vs) for i in range(3)]
    finally:
        ev.to_mesh_clear()


def rewrite(path, change):
    """Change the file as Drift saving would, with a later modification time."""
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    change(data)
    before = os.stat(path).st_mtime_ns
    with open(path, "w", encoding="utf-8") as f:
        json.dump(data, f)
    os.utime(path, ns=(before + 2_000_000_000, before + 2_000_000_000))


def scaled(profile, k):
    for loop in profile["loops"]:
        for knot in loop["knots"]:
            for key in ("co", "left", "right"):
                knot[key] = [v * k for v in knot[key]]


def run():
    drift_profiles.register()
    try:
        tmp = tempfile.mkdtemp()
        path = os.path.join(tmp, "Trim.drift-profiles.json")
        shutil.copy(FIXTURE, path)
        ctx = bpy.context

        crown, vase = ui.import_profiles(ctx, path, {"b2": "XY", "b3": "XZ"})
        assert (crown.name, vase.name) == ("Crown", "Vase"), (crown.name, vase.name)
        assert crown.data.dimensions == "2D" and crown.data.fill_mode == "BOTH"
        assert [s.use_cyclic_u for s in crown.data.splines] == [True, True]
        assert [s.use_cyclic_u for s in vase.data.splines] == [False]
        assert abs(max(p.co.x for p in crown.data.splines[0].bezier_points) - 0.04) < 1e-6
        assert abs(max(p.co.z for p in vase.data.splines[0].bezier_points) - 0.12) < 1e-6

        # A rail swept with the moulding, and the vase lathed.
        rail_curve = bpy.data.curves.new("Rail", "CURVE")
        rail_curve.dimensions = "3D"
        spline = rail_curve.splines.new("POLY")
        spline.points.add(1)
        spline.points[0].co = (0.0, 0.0, 0.0, 1.0)
        spline.points[1].co = (1.0, 0.0, 0.0, 1.0)
        rail_curve.bevel_mode = "OBJECT"
        rail_curve.bevel_object = crown
        rail = bpy.data.objects.new("Rail", rail_curve)
        ctx.collection.objects.link(rail)
        screw = vase.modifiers.new("Screw", "SCREW")
        screw.axis = "Z"
        crown.location = (1.0, 2.0, 3.0)
        crown_data = crown.data

        swept = extent(rail)
        lathed = extent(vase)
        # The rail's section is the 40 × 30 mm moulding; the vase is 2 × (40 + 60) mm across.
        assert abs(max(swept[1], swept[2]) - 0.04) < 1e-3, swept
        assert abs(lathed[0] - 0.2) < 1e-3, lathed

        # Nothing changed: Auto-reload leaves them be.
        ctx.scene.drift_auto_reload = True
        assert curves.changed() == [], curves.changed()

        # Drift saves: the moulding twice the size, the vase renamed.
        def edit(data):
            scaled(data["profiles"][0], 2.0)
            data["profiles"][1]["name"] = "Urn"

        rewrite(path, edit)
        assert sorted(o.name for o in curves.changed()) == ["Crown", "Vase"]
        ui.auto_reload()
        assert curves.changed() == []
        # Updated in place: the same objects and curves, still used the same way.
        assert rail_curve.bevel_object == crown, rail_curve.bevel_object
        assert crown.data == crown_data
        assert tuple(crown.location) == (1.0, 2.0, 3.0)
        assert vase.modifiers.get("Screw") is not None
        assert vase.name == "Urn", vase.name
        assert abs(max(p.co.x for p in crown.data.splines[0].bezier_points) - 0.08) < 1e-6
        swept_after = extent(rail)
        assert abs(max(swept_after[1], swept_after[2]) - 0.08) < 1e-3, swept_after

        # Unmarked in Drift: kept, and flagged.
        rewrite(path, lambda data: data["profiles"].pop())
        ui.auto_reload()
        assert vase.get(curves.MISSING) is True, dict(vase.items())
        assert crown.get(curves.MISSING) is False
        assert len(vase.data.splines) == 1, "kept as it was"

        # Reload Selected works from the operator too.
        for o in ctx.view_layer.objects:
            o.select_set(o == crown)
        assert bpy.ops.drift.reload_profiles(selected_only=True) == {"FINISHED"}

        # The import dialog's choices: only what's ticked comes in.
        count = len(bpy.data.objects)
        choices = [
            {"name": "Crown", "profile_id": "b2", "use": True, "orientation": "XZ"},
            {"name": "Other", "profile_id": "b9", "use": False, "orientation": "XY"},
        ]
        result = bpy.ops.drift.choose_profiles("EXEC_DEFAULT", filepath=path, choices=choices)
        assert result == {"FINISHED"}, result
        assert len(bpy.data.objects) == count + 1
        upright = ctx.view_layer.objects.active
        assert upright[curves.ORIENTATION] == "XZ" and upright.data.dimensions == "3D"
        shutil.rmtree(tmp)
    finally:
        drift_profiles.unregister()


try:
    run()
except Exception:
    traceback.print_exc()
    sys.exit(1)
print("Drift profiles: headless test passed")
