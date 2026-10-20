"""The profiles file to spline points, without Blender: python -m unittest discover integrations/blender/tests"""

import json
import math
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "drift_profiles"))

import convert  # noqa: E402

FIXTURE = os.path.join(HERE, "fixtures", "Trim.drift-profiles.json")


def bezier(p0, p1, p2, p3, t):
    u = 1.0 - t
    return tuple(
        u * u * u * a + 3 * u * u * t * b + 3 * u * t * t * c + t * t * t * d
        for a, b, c, d in zip(p0, p1, p2, p3)
    )


def samples(spline, n=16):
    pts = spline["points"]
    segments = len(pts) if spline["cyclic"] else len(pts) - 1
    for i in range(segments):
        a, b = pts[i], pts[(i + 1) % len(pts)]
        for k in range(n + 1):
            yield bezier(a["co"], a["right"], b["left"], b["co"], k / n)


class FixtureTest(unittest.TestCase):
    def setUp(self):
        self.data = convert.read(FIXTURE)

    def test_the_fixture_reads(self):
        self.assertEqual(self.data["source"], "Trim.drift")
        names = [(p["name"], p["id"]) for p in self.data["profiles"]]
        self.assertEqual(names, [("Crown", "b2"), ("Vase", "b3")])

    def test_a_moulding_is_an_outline_and_a_hole_in_xy(self):
        crown = convert.find(self.data, "b2")
        splines = convert.splines(crown, "XY")
        self.assertEqual([s["cyclic"] for s in splines], [True, True])
        # The outline runs from the base point to 40 × 30 mm, flat on XY.
        xs = [p["co"][0] for p in splines[0]["points"]]
        ys = [p["co"][1] for p in splines[0]["points"]]
        self.assertAlmostEqual(min(xs), 0.0)
        self.assertAlmostEqual(max(xs), 0.04)
        self.assertAlmostEqual(max(ys), 0.03)
        self.assertTrue(all(p["co"][2] == 0.0 for p in splines[0]["points"]))
        # The hole: a 4 mm circle round (10, 10) mm.
        for x, y, z in samples(splines[1]):
            self.assertAlmostEqual(math.hypot(x - 0.01, y - 0.01), 0.004, delta=1e-7)
            self.assertEqual(z, 0.0)

    def test_a_lathe_profile_stands_up_in_xz(self):
        vase = convert.find(self.data, "b3")
        self.assertTrue(convert.is_lathe(vase))
        self.assertFalse(convert.is_lathe(convert.find(self.data, "b2")))
        (spline,) = convert.splines(vase, "XZ")
        self.assertFalse(spline["cyclic"])
        first, last = spline["points"][0]["co"], spline["points"][-1]["co"]
        # From the axis at the bottom to the axis at the top, 120 mm up Z.
        self.assertEqual(first, (0.0, 0.0, 0.0))
        self.assertEqual(last, (0.0, 0.0, 0.12))
        self.assertTrue(all(p["co"][1] == 0.0 for p in spline["points"]))
        # The bulge: a 60 mm arc round (40, 60) mm.
        for x, _, z in list(samples(spline))[17:-17]:
            self.assertAlmostEqual(math.hypot(x - 0.04, z - 0.06), 0.06, delta=1e-6)

    def test_the_scenes_unit_scale_is_followed(self):
        crown = convert.find(self.data, "b2")
        metres = convert.splines(crown, "XY", 1.0)
        mm = convert.splines(crown, "XY", 0.001)
        self.assertAlmostEqual(mm[0]["points"][1]["co"][0], 40.0)
        self.assertAlmostEqual(metres[0]["points"][1]["co"][0], 0.04)

    def test_profiles_are_found_by_id(self):
        self.assertIsNone(convert.find(self.data, "b99"))


class BadFileTest(unittest.TestCase):
    def good(self):
        with open(FIXTURE, encoding="utf-8") as f:
            return json.load(f)

    def test_other_files_are_refused(self):
        for text in ["", "[]", '{"format":"drift","version":36}', "not json"]:
            with self.assertRaises(convert.ProfileError):
                convert.parse(text)

    def test_a_newer_file_asks_for_an_update(self):
        data = self.good()
        data["version"] = convert.VERSION + 1
        with self.assertRaisesRegex(convert.ProfileError, "update"):
            convert.parse(json.dumps(data))

    def test_a_loop_needs_knots(self):
        data = self.good()
        data["profiles"][0]["loops"][0]["knots"] = []
        with self.assertRaises(convert.ProfileError):
            convert.parse(json.dumps(data))

    def test_an_unknown_orientation_is_refused(self):
        with self.assertRaises(ValueError):
            convert.splines(self.good()["profiles"][0], "YZ")


if __name__ == "__main__":
    unittest.main()
