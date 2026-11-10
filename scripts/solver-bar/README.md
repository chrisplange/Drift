# The solver's acceptance bar

Every solver or drag PR in `docs/solver-audit.md`'s plan is judged against the same seven
numbers (section 5). This folder runs them. Every PR puts its numbers next to its base's.
Build in release, and run one test per process.

| # | Item | How |
|---|---|---|
| 1 | Heavy matrix: 14 properties × seeds 1–20 | `python scripts/solver-bar/heavy.py`: builds, runs, compares with `expected.txt` |
| 2 | Similarity census | `drag_props::census::similarity_census` (below), then `analyse.py similarity DIR` |
| 3 | #377's census | `drag_props::census::drag_census` and `drag_census_seeded` per place, then `analyse.py census BASE [PR]` |
| 4 | Pins | `cargo test --release -p drift-edit --lib drag_props::seeds:: -- --include-ignored` |
| 5 | Perf baselines | `cargo test --release -- --ignored --nocapture --test-threads=1`: `perf_drag_refused`, `perf_drag_tiles` (drift-edit), `perf_big`, `perf_big_drag` (drift-app, `DRIFT_BIG` set to a big drawing) |
| 6 | Linux | the `Solver bar` workflow: items 1 and 4 on ubuntu, nightly and on PRs that touch the solver |
| 7 | The binding drag rule | `drag_props::held_shapes::` (in the normal run), and the censuses' stop counts |

## Censuses

Each census test prints one line per drag; keep the lines and hand the files to
`analyse.py`. The cases come from the same generator and seed as #365's, #377's and the
audit's, so case numbers match theirs. In bash (Git Bash on Windows):

```bash
exe=$(cargo test --release -p drift-edit --lib --no-run --message-format=json | grep -o '"executable":"[^"]*drift_edit[^"]*"' | tail -1 | cut -d'"' -f4)
mkdir -p sim; for k in 0 1 2 3 4 5; do
  CENSUS_FROM=$((k*2000)) CENSUS_TO=$(((k+1)*2000)) "$exe" drag_props::census::similarity_census \
    --exact --ignored --test-threads=1 --nocapture 2>/dev/null | grep -o 'A [0-9].*' > sim/main-$k.txt &
done; wait
python scripts/solver-bar/analyse.py similarity sim main
```

For #377's census, one file per place, named by the place (`0`, `1m`, `3m`, `10m`, `30m`,
`100m`, `1km`, and `100km` for the seeded one). `CENSUS_SHIFT` is the shift in mm added to x
and to y, so a place d out is d/√2 (1 km: `707106.78`):

```bash
CENSUS_SHIFT=707106.78 "$exe" drag_props::census::drag_census --exact --ignored --test-threads=1 \
  --nocapture 2>/dev/null | grep -o 'C [0-9].*' > random/1km.txt
python scripts/solver-bar/analyse.py census base/random pr/random
```

Every line also carries the solver's exit counts over the drag, from the test-only
`drift_solver::exits` counters (the `exit-counts` feature, which only drift-edit's tests turn
on).
