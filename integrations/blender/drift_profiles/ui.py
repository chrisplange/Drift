# SPDX-License-Identifier: GPL-3.0-or-later
"""File → Import → Drift profiles, the Drift tab in the 3D view's sidebar, and Auto-reload."""

import os
import time

import bpy
from bpy.props import BoolProperty, CollectionProperty, EnumProperty, StringProperty
from bpy_extras.io_utils import ImportHelper

from . import convert, curves

ORIENTATION_ITEMS = (
    ("XY", "XY · sweep", "Lay the profile flat, to sweep along a curve as its bevel object"),
    ("XZ", "XZ · lathe", "Stand the profile up, to spin round Z with a Screw modifier"),
)

# How often Auto-reload looks at the files, in seconds: Drift rewrites them as often while a
# profile block is open for editing (its live link), and on every save.
POLL = 0.1


def set_status(text):
    wm = bpy.context.window_manager
    if wm is None:
        return
    wm.drift_status = "{} · {}".format(text, time.strftime("%H:%M:%S"))
    for window in wm.windows:
        for area in window.screen.areas:
            if area.type == "VIEW_3D":
                area.tag_redraw()


def reload_and_report(objects, scene):
    updated, missing, errors = curves.reload(objects, scene)
    parts = []
    if updated:
        parts.append("Updated {} profile{}".format(len(updated), "" if len(updated) == 1 else "s"))
    if missing:
        parts.append("{} missing in Drift".format(len(missing)))
    parts.extend(errors)
    set_status(", ".join(parts) if parts else "No Drift profiles to reload")
    return updated, missing, errors


class DRIFT_PG_choice(bpy.types.PropertyGroup):
    profile_id: StringProperty()
    use: BoolProperty(name="Import", default=True)
    orientation: EnumProperty(name="Orientation", items=ORIENTATION_ITEMS, default="XY")


class DRIFT_OT_import_profiles(bpy.types.Operator, ImportHelper):
    """Import profiles drawn in Drift, to sweep or lathe; they update when Drift saves"""

    bl_idname = "import_curve.drift_profiles"
    bl_label = "Import Drift Profiles"
    bl_options = {"REGISTER", "UNDO"}

    filename_ext = ".json"
    filter_glob: StringProperty(default=convert.FILTER_GLOB, options={"HIDDEN"})

    def execute(self, context):
        try:
            convert.read(self.filepath)
        except (OSError, convert.ProfileError) as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        bpy.ops.drift.choose_profiles("INVOKE_DEFAULT", filepath=self.filepath)
        return {"FINISHED"}


class DRIFT_OT_choose_profiles(bpy.types.Operator):
    """Choose which of the file's profiles to import, and how each is laid"""

    bl_idname = "drift.choose_profiles"
    bl_label = "Import Drift Profiles"
    bl_options = {"REGISTER", "UNDO", "INTERNAL"}

    filepath: StringProperty(subtype="FILE_PATH", options={"HIDDEN"})
    choices: CollectionProperty(type=DRIFT_PG_choice)

    def invoke(self, context, event):
        try:
            data = convert.read(self.filepath)
        except (OSError, convert.ProfileError) as e:
            self.report({"ERROR"}, str(e))
            return {"CANCELLED"}
        if not data["profiles"]:
            self.report({"WARNING"}, "The file has no profiles")
            return {"CANCELLED"}
        self.choices.clear()
        for p in data["profiles"]:
            c = self.choices.add()
            c.name = p["name"]
            c.profile_id = p["id"]
            c.orientation = "XZ" if convert.is_lathe(p) else "XY"
        return context.window_manager.invoke_props_dialog(self, width=360)

    def draw(self, context):
        layout = self.layout
        layout.label(text=os.path.basename(self.filepath))
        for c in self.choices:
            row = layout.row()
            row.prop(c, "use", text=c.name)
            row.prop(c, "orientation", text="")

    def execute(self, context):
        chosen = {c.profile_id: c.orientation for c in self.choices if c.use}
        objs = import_profiles(context, self.filepath, chosen)
        if objs is None:
            self.report({"ERROR"}, "Could not read {}".format(self.filepath))
            return {"CANCELLED"}
        self.report({"INFO"}, "Imported {} Drift profile(s)".format(len(objs)))
        return {"FINISHED"}


def import_profiles(context, path, chosen):
    """Import the profiles ``chosen`` (id → orientation) from the file at ``path``, picked.
    Returns the new objects, or None if the file can't be read."""
    stamp = curves.mtime(path)
    try:
        data = convert.read(path)
    except (OSError, convert.ProfileError):
        return None
    objs = [
        curves.new_object(context, path, p, chosen[p["id"]], stamp)
        for p in data["profiles"]
        if p["id"] in chosen
    ]
    if objs:
        for o in context.view_layer.objects:
            o.select_set(False)
        for o in objs:
            o.select_set(True)
        context.view_layer.objects.active = objs[0]
        set_status("Imported {} from {}".format(len(objs), os.path.basename(path)))
    return objs


class DRIFT_OT_reload(bpy.types.Operator):
    """Reload profiles from Drift's file, in place: sweeps and lathes follow"""

    bl_idname = "drift.reload_profiles"
    bl_label = "Reload Drift Profiles"
    bl_options = {"REGISTER", "UNDO"}

    selected_only: BoolProperty(name="Selected Only", default=False)

    def execute(self, context):
        objects = context.selected_objects if self.selected_only else None
        updated, missing, errors = reload_and_report(objects, context.scene)
        for e in errors:
            self.report({"ERROR"}, e)
        if missing:
            self.report(
                {"WARNING"},
                "Not in Drift's file any more: {}".format(", ".join(o.name for o in missing)),
            )
        return {"FINISHED"}


class DRIFT_PT_profiles(bpy.types.Panel):
    bl_label = "Drift Profiles"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Drift"

    def draw(self, context):
        layout = self.layout
        layout.operator(DRIFT_OT_import_profiles.bl_idname, text="Import…", icon="IMPORT")
        row = layout.row(align=True)
        row.operator(DRIFT_OT_reload.bl_idname, text="Reload All", icon="FILE_REFRESH")
        op = row.operator(DRIFT_OT_reload.bl_idname, text="Selected")
        op.selected_only = True
        layout.prop(context.scene, "drift_auto_reload")
        status = context.window_manager.drift_status
        if status:
            layout.label(text=status)
        missing = [o for o in curves.profile_objects() if o.get(curves.MISSING)]
        if missing:
            box = layout.box()
            box.label(text="Not in Drift's file any more:", icon="ERROR")
            for o in missing:
                box.label(text=o.name)
        obj = context.active_object
        if curves.is_profile(obj):
            box = layout.box()
            box.label(text="{} · {}".format(obj.get(curves.NAME, obj.name), obj[curves.PROFILE_ID]))
            box.label(text=os.path.basename(obj[curves.SOURCE]), icon="FILE")
            box.label(text="Laid in {}".format(obj.get(curves.ORIENTATION, "XY")))


def auto_reload():
    """The timer: reload profiles whose file Drift has saved since they were loaded."""
    try:
        scene = bpy.context.scene
        if scene is not None and scene.drift_auto_reload:
            stale = curves.changed()
            if stale:
                reload_and_report(stale, scene)
    except Exception as e:  # A timer that raises is dropped: keep going.
        print("Drift profiles auto-reload:", e)
    return POLL


def menu_import(self, context):
    self.layout.operator(DRIFT_OT_import_profiles.bl_idname, text="Drift Profiles (.drift-profiles.json)")


classes = (
    DRIFT_PG_choice,
    DRIFT_OT_import_profiles,
    DRIFT_OT_choose_profiles,
    DRIFT_OT_reload,
    DRIFT_PT_profiles,
)


def register():
    for cls in classes:
        bpy.utils.register_class(cls)
    bpy.types.Scene.drift_auto_reload = BoolProperty(
        name="Auto-reload",
        description="Reload profiles as Drift changes them: on save, and while a profile "
        "block is open for editing in Drift",
        default=False,
    )
    bpy.types.WindowManager.drift_status = StringProperty(name="Drift profiles status")
    bpy.types.TOPBAR_MT_file_import.append(menu_import)
    if not bpy.app.timers.is_registered(auto_reload):
        bpy.app.timers.register(auto_reload, first_interval=POLL, persistent=True)


def unregister():
    if bpy.app.timers.is_registered(auto_reload):
        bpy.app.timers.unregister(auto_reload)
    bpy.types.TOPBAR_MT_file_import.remove(menu_import)
    del bpy.types.WindowManager.drift_status
    del bpy.types.Scene.drift_auto_reload
    for cls in reversed(classes):
        bpy.utils.unregister_class(cls)
