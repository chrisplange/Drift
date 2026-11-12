"""Tests for the merge-gate scripts: tier.py, review_gate.py and auto_merge.py.

Run from the repository root: python -m unittest discover -s scripts/tests -v
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import auto_merge  # noqa: E402
import review_gate  # noqa: E402
import tier  # noqa: E402

RULES = tier.parse_rules(tier.DEFAULT_RULES.read_text(encoding="utf-8"))
SHA = "0123456789abcdef0123456789abcdef01234567"
OTHER = "fedcba9876543210fedcba9876543210fedcba98"


def t(*files):
    return tier.classify(list(files), RULES)[0]


class TierTests(unittest.TestCase):
    def test_docs_only_is_auto(self):
        self.assertEqual(t("docs/guide/getting-started.md", "README.md"), "auto")

    def test_app_ui_is_auto(self):
        self.assertEqual(t("crates/drift-app/src/ui/panels.rs"), "auto")

    def test_solver_file_is_solver(self):
        self.assertEqual(t("crates/drift-solver/src/newton.rs"), "solver")

    def test_drag_props_file_and_dir_are_solver(self):
        self.assertEqual(t("crates/drift-edit/src/drag_props.rs"), "solver")
        self.assertEqual(t("crates/drift-edit/src/drag_props/seeds.rs"), "solver")
        self.assertEqual(t("crates/drift-edit/src/editor/drag.rs"), "solver")

    def test_other_drift_edit_is_auto(self):
        self.assertEqual(t("crates/drift-edit/src/lib.rs"), "auto")
        self.assertEqual(t("crates/drift-edit/src/editor/selection.rs"), "auto")

    def test_document_model_is_auto_but_tolerance_is_solver(self):
        # Papercut sweeps touch walls and sheets in drift-core; they shouldn't need a solver
        # review. The tolerance and relation definitions do.
        self.assertEqual(t("crates/drift-core/src/walls.rs"), "auto")
        self.assertEqual(t("crates/drift-core/src/tolerance.rs"), "solver")
        self.assertEqual(t("crates/drift-core/src/constraint.rs"), "solver")

    def test_app_export_save_ai_link_updater_are_gated(self):
        for f in ("crates/drift-app/src/output.rs", "crates/drift-app/src/app/recovery.rs",
                  "crates/drift-app/src/agent/files.rs", "apps/mcp/src/main.rs",
                  "crates/drift-app/src/update/check.rs"):
            self.assertEqual(t(f), "gated", f)

    def test_heavy_matrix_paths(self):
        h = lambda *fs: tier.needs_heavy(list(fs), RULES)  # noqa: E731
        self.assertTrue(h("crates/drift-core/src/walls.rs"))
        self.assertTrue(h("crates/drift-edit/src/editor/selection.rs"))
        self.assertTrue(h("crates/drift-solver/src/lib.rs"))
        self.assertTrue(h("Cargo.lock"))
        self.assertTrue(h())
        self.assertFalse(h("crates/drift-app/src/app/panels.rs", "docs/guide/x.md"))

    def test_cargo_lock_and_crate_manifest_are_gated(self):
        self.assertEqual(t("Cargo.lock"), "gated")
        self.assertEqual(t("crates/drift-app/Cargo.toml"), "gated")

    def test_process_files_are_gated(self):
        for f in (".claude/settings.json", "CLAUDE.md", "WORKFLOW.md", ".github/tiers.txt",
                  ".github/workflows/ci.yml", "scripts/tier.py", "scripts/tests/test_gates.py"):
            self.assertEqual(t(f), "gated", f)

    def test_io_is_gated(self):
        self.assertEqual(t("crates/drift-io/src/dxf/import.rs"), "gated")

    def test_mixed_takes_the_strictest(self):
        self.assertEqual(t("README.md", "Cargo.lock"), "gated")
        self.assertEqual(t("README.md", "Cargo.lock", "crates/drift-solver/src/doc.rs"), "solver")

    def test_empty_change_is_gated(self):
        self.assertEqual(t(), "gated")
        self.assertEqual(t("", "  "), "gated")

    def test_backslashes_are_normalised(self):
        self.assertEqual(t("crates\\drift-solver\\src\\lib.rs"), "solver")

    def test_glob_star_stays_in_one_segment(self):
        rx = tier.glob_to_regex("scripts/package-*")
        self.assertTrue(rx.match("scripts/package-windows.ps1"))
        self.assertFalse(rx.match("scripts/package-x/inner.sh"))

    def test_double_star_prefix_matches_root_too(self):
        rx = tier.glob_to_regex("**/Cargo.toml")
        self.assertTrue(rx.match("Cargo.toml"))
        self.assertTrue(rx.match("crates/a/Cargo.toml"))
        self.assertFalse(rx.match("crates/a/Cargo.toml.bak"))

    def test_bad_rules_are_refused(self):
        with self.assertRaises(ValueError):
            tier.parse_rules("orphan/**\n")
        with self.assertRaises(ValueError):
            tier.parse_rules("[nope]\nx\n")


def comment(body, assoc="OWNER", login="chrisplange"):
    return {"body": body, "author_association": assoc, "user": {"login": login}}


class ReviewGateTests(unittest.TestCase):
    def test_not_solver_passes_without_review(self):
        self.assertEqual(review_gate.decide("auto", [], SHA)[0], "success")
        self.assertEqual(review_gate.decide("gated", [], SHA)[0], "success")

    def test_solver_without_marker_fails(self):
        self.assertEqual(review_gate.decide("solver", [comment("looks fine")], SHA)[0], "failure")

    def test_owner_marker_for_head_passes(self):
        c = comment(f"Review done.\n<!-- drift-review: approve {SHA} -->")
        self.assertEqual(review_gate.decide("solver", [c], SHA)[0], "success")

    def test_marker_for_an_older_head_fails(self):
        c = comment(f"<!-- drift-review: approve {OTHER} -->")
        self.assertEqual(review_gate.decide("solver", [c], SHA)[0], "failure")

    def test_marker_from_anyone_else_fails(self):
        for assoc in ("CONTRIBUTOR", "NONE", "COLLABORATOR", "MEMBER"):
            c = comment(f"<!-- drift-review: approve {SHA} -->", assoc=assoc, login="someone")
            self.assertEqual(review_gate.decide("solver", [c], SHA)[0], "failure", assoc)

    def test_short_sha_does_not_count(self):
        c = comment(f"<!-- drift-review: approve {SHA[:7]} -->")
        self.assertEqual(review_gate.decide("solver", [c], SHA)[0], "failure")

    def test_description_fits_a_status(self):
        for tier_name in ("auto", "solver"):
            self.assertLessEqual(len(review_gate.decide(tier_name, [], SHA)[1]), 140)


def pr(**over):
    base = {
        "state": "OPEN", "isDraft": False, "headRepositoryOwner": {"login": "chrisplange"},
        "author": {"login": "chrisplange"}, "headRefOid": SHA, "labels": [],
        "mergeStateStatus": "CLEAN",
    }
    base.update(over)
    return base


GREEN = [{"name": "Format and lint", "bucket": "pass"},
         {"name": "Heavy drag matrix and pins (ubuntu)", "bucket": "skipping"}]


class AutoMergeTests(unittest.TestCase):
    def b(self, p=None, sha=SHA, tier_name="auto", checks=GREEN):
        return auto_merge.blockers(p or pr(), "chrisplange", sha, tier_name, checks)

    def test_clean_auto_pr_merges(self):
        self.assertEqual(self.b(), [])
        self.assertEqual(self.b(sha=None), [])

    def test_gated_and_solver_never_merge(self):
        self.assertTrue(self.b(tier_name="gated"))
        self.assertTrue(self.b(tier_name="solver"))

    def test_no_required_checks_means_no_merge(self):
        self.assertTrue(self.b(checks=[]))

    def test_a_failing_or_pending_check_blocks(self):
        for bucket in ("fail", "pending", "cancel"):
            self.assertTrue(self.b(checks=GREEN + [{"name": "Test", "bucket": bucket}]), bucket)

    def test_moved_head_blocks(self):
        self.assertTrue(self.b(sha=OTHER))

    def test_fork_other_author_draft_hold_behind_block(self):
        self.assertTrue(self.b(pr(headRepositoryOwner={"login": "x"})))
        self.assertTrue(self.b(pr(author={"login": "x"})))
        self.assertTrue(self.b(pr(isDraft=True)))
        self.assertTrue(self.b(pr(labels=[{"name": "hold"}])))
        self.assertTrue(self.b(pr(mergeStateStatus="BEHIND")))
        self.assertTrue(self.b(pr(state="MERGED")))

    def test_website_paths(self):
        self.assertTrue(auto_merge.touches_website(["docs/guide/x.md"]))
        self.assertTrue(auto_merge.touches_website(["crates/drift-app/src/lib.rs"]))
        self.assertFalse(auto_merge.touches_website(["PAPERCUTS.md", "integrations/blender/x.py"]))


if __name__ == "__main__":
    unittest.main()
