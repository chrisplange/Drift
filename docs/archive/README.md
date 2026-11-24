# Archive

Design notes, audits and plans whose work is built or decided. They are kept as the record of
why things are the way they are, not as current guidance: the code, [FEATURES.md](../../FEATURES.md)
and the GitHub issues win where they disagree. Archived 2026-11-24.

| File | Date | What it was |
|---|---|---|
| [ai-format-research.md](ai-format-research.md) | 2026-10-25 | Research spike on reading Adobe Illustrator `.ai` files natively; any follow-up is a GitHub issue |
| [cad-interop-audit.md](cad-interop-audit.md) | 2026-10-26 | DXF and DWG interop audit (format 42); its fix batches became PRs and issues |
| [fills-and-strokes-audit.md](fills-and-strokes-audit.md) | 2026-10-25 | Audit of the shape, fill and stroke model |
| [fills-202-design.md](fills-202-design.md) | 2026-10-27 | One kind of filled shape (#202); decided and built |
| [open-drawings-decision.md](open-drawings-decision.md) | 2026-10-28 | Per-window sessions versus tabs for several open drawings; decided |
| [solver-drag-regions-plan.md](solver-drag-regions-plan.md) | 2026-10-21 | Constraints only where you add them, predictable drag, robust regions |
| [trim-and-regions-plan.md](trim-and-regions-plan.md) | 2026-10-25 | Trim, regions 3b, Divide and later Booleans |
| [ui-audit-ribbon.md](ui-audit-ribbon.md) | 2026-10-26 | Tool rail layout audit; layout A decided and built |

Still current, and kept in `docs/`: the [solver audit](../solver-audit.md) and its
[references](../solver-audit-references.md), [solver heuristics](../solver-heuristics.md),
[walls design](../walls-design.md) and the [Editor split design](../editor-split-design.md)
(code cites both as the protocol for generated geometry), the [file format](../file-format.md),
[releasing](../releasing.md), the [regression checklist](../regression-checklist.md),
[halo features](../halo-features.md), the [variants grid proposal](../variants-grid-design.md)
and [What's new](../whats-new-draft.md).

PLAN.md §4–11 (the old roadmap, risks and next steps) were cut on the same day; they are in
git history before that date.
