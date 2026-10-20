# SPDX-License-Identifier: GPL-3.0-or-later
"""Drift profiles: curves drawn in Drift, swept or lathed in Blender, updated when Drift saves.

In Drift, mark a block as a Blender profile; saving the drawing writes its curves beside it
as ``<name>.drift-profiles.json``. Import that here (File → Import → Drift Profiles), use the
curves as bevel objects or with a Screw modifier, and Reload (or turn on Auto-reload in the
3D view's Drift tab) to pick up Drift's changes in place.
"""

from . import ui


def register():
    ui.register()


def unregister():
    ui.unregister()
