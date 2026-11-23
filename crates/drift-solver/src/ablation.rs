//! Switching one heuristic off at a time (#187): test-only, for the heuristic ledger
//! (`docs/solver-heuristics.md`).
//!
//! Every rule and constant layered onto the solve and the drag has a [`Switch`]. With the
//! `ablate` feature, the environment's `DRIFT_ABLATE` names the switches that are off (by
//! [`Switch::name`], comma separated), read once a process; the sketch corpus and the tests
//! are then run with each off in turn, and what breaks is what the rule protects
//! (`python scripts/solver-bar/ablate.py`). Without the feature, which is every build of the
//! app and every normal test build, [`off`] is `false` at compile time and each rule's code
//! is exactly as it was: a switch changes nothing unless it is thrown.
//!
//! A switch is thrown where its rule is, as `ablation::off(Switch::…)`. Adding a heuristic
//! means adding its switch here, its row in the ledger, and its run.

/// One heuristic or constant. The ledger describes each, by [`name`](Switch::name).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Switch {
    // The solve (`newton` and around it).
    /// The kept shapes' weight in the first stage's metric (`KEEP_SHAPE`).
    KeepShapeWeight,
    /// `KEEP_SHAPE` ten times as heavy, and a tenth.
    KeepShapeTimes10,
    KeepShapeOver10,
    /// The second-order correction after a first-stage step (`CORRECTION_SHARE`).
    SecondOrderCorrection,
    /// A first-stage step longer than the drawing is across is refused.
    StepLimit,
    /// A drag's steps together move no point farther than the drawing is across.
    DragBoundFromEntry,
    /// An edit's preference steps together move no point farther than the drawing is across.
    EditPreferenceBound,
    /// A step that delivers under `WANDERING_GAIN` of its promise counts as wandered.
    WanderingGain,
    /// A second-stage trial is kept only at a quarter of the gain it promised.
    PreferenceQuarterGain,
    /// The second stage settles where a step would bring the shapes no more than a bar nearer.
    PreferenceNearerStop,
    /// The second stage doesn't settle on its projected gradient while a kept radius is off.
    RadiiHoldStationary,
    /// A drag solve that leaves a kept radius off that its free geometry could change is
    /// unfinished (`unkept`).
    UnkeptRadiusUnfinished,
    /// The landing passes (`land`).
    LandingPasses,
    /// A row that holds with next to no gradient left is left out of the step
    /// (`NEGLIGIBLE_ROW`, the zero scale).
    NegligibleRows,
    /// Every row is scaled to a unit gradient.
    RowScaling,
    /// The floor of ulps judges stored values only (`leave_storage`).
    LeaveStorage,
    /// The rows hold to the bar of the part as the solve leaves it (`bar_now`).
    BarWhereItStands,
    /// A fold solve's minimum change comes down as its rows close (`fold_scale`).
    FoldScale,
    /// The damping keeps the dragged and carried heavy.
    DampingKeepsDraggedHeavy,
    /// An arc with a dragged end, or a circle with a dragged radius, isn't asked to keep it.
    DraggedEndDropsRadius,
    /// A line with a dragged end isn't asked to keep its length.
    DraggedEndDropsLength,
    /// Tangencies touch where they did as the drag began (`SolveOptions::touching`).
    TouchingFrozen,
    /// `MIN_CHANGE` a hundred times as large, and a hundredth.
    MinChangeTimes100,
    MinChangeOver100,
    /// `DRAGGED_MOBILITY` a hundred times as mobile (as the carried are).
    DraggedMobilityTimes100,
    /// `MAX_ITERATIONS` ten times as many.
    IterationCapTimes10,
    /// `FOLD_MET` a thousand times as loose.
    FoldMetTimes1000,
    /// Geometry outside a new relation's focus is a thousand times as heavy
    /// (`UNFOCUSED_MOBILITY`).
    UnfocusedMobility,
    // What counts as turned (`Orientation`).
    /// An arc whose sweep jumps over `SWEEP_JUMP` turned.
    ArcSweepTurn,
    /// A collapsed arc or crushed circle counts as turned (`COLLAPSED_RADIUS`).
    CollapsedCountsAsTurned,
    /// A tangency that changed side turned.
    TangentSides,
    /// A signed distance that changed sign turned.
    DistanceSigns,
    // The drag (`Editor::drag` and its steps).
    /// A long move goes in steps along the pointer's line, not in one solve first.
    Strides,
    /// A frame whose steps didn't land is settled by the pointer.
    SettleByThePointer,
    /// The settle is kept only where it moves nothing else a hundred times as far as the
    /// dragged (`LINE_HOLD`).
    SettleFlingsNothing,
    /// The settle is kept only where it leaves the part's relations met.
    SettleMeetsRelations,
    /// The settle is undone where it breaks a held shape.
    SettleKeepsHeldShapes,
    /// Where the steps can't be made, the move is tried in one solve.
    OneSolveFallback,
    /// A step that fails is taken again in halves (`drag_towards`).
    HalveFailedSteps,
    /// A step that wandered is taken again in halves (`WANDERED`, `drag_towards`).
    HalveWanderingSteps,
    /// A solve that fails or turns the shape is split in halves (`MAX_SPLITS`).
    SplitSteps,
    /// A step whose solve doesn't finish is not kept.
    UnfinishedIsRefused,
    /// A step that turns an arc, a tangency or a signed distance is not kept.
    TurnedIsRefused,
    /// A step that turns a carried line end for end is not kept (`NO_WAY_ROUND`,
    /// `SQUEEZED_SHUT`).
    CarriedLineFlip,
    /// A line squeezed to `SQUEEZED_SHUT` of its length has no way round to keep.
    SqueezedShut,
    /// A step that turns a line a lowered relation measures from is not kept.
    LoweredLineFlip,
    /// Curves even the smallest step turns are carried along.
    CarryTurnedCurves,
    /// The curves a dragged point is held on are carried along (`carry_holding`).
    CarryHolding,
    /// A curve one step pushes is pushed by every later step of the frame.
    PushedStaysPushed,
    /// The try with the fold steps.
    FoldTry,
    /// The try at keeping a hanging line's length (`LENGTH_TRY_*`).
    LengthTry,
    /// The whole of `breaks_held_shape`.
    HeldShapeStop,
    /// An arc or circle the drag doesn't move never grows.
    NeverGrow,
    /// A line the drag doesn't move isn't crushed or flung short (`LINE_HOLD`,
    /// `COLLAPSED_LINE`).
    CrushedLineStop,
    /// A drag that doesn't land doesn't change a held radius faster than `SHAPE_HOLD` times
    /// the pointer.
    UnlandedRadiusHold,
    /// An offset refused once in a drag is refused again without a solve.
    RefusedAgain,
    /// A frame's later solves are held no looser than its first (`bar_cap`).
    BarCap,
    /// A drag asks its rounds to keep their radii.
    KeptRadii,
    /// A move asks the lines hanging from it to keep their lengths.
    KeptLengths,
    /// Smooth anchors are kept smooth before the solve.
    SmoothAnchors,
    /// `DRAG_SOLVES` ten times as many.
    DragSolvesTimes10,
    /// `UNFINISHED_WORK` ten times as much, and none.
    UnfinishedWorkTimes10,
    UnfinishedWorkNone,
}

impl Switch {
    /// Every switch, in the ledger's order.
    pub const ALL: [Switch; 63] = [
        Switch::KeepShapeWeight,
        Switch::KeepShapeTimes10,
        Switch::KeepShapeOver10,
        Switch::SecondOrderCorrection,
        Switch::StepLimit,
        Switch::DragBoundFromEntry,
        Switch::EditPreferenceBound,
        Switch::WanderingGain,
        Switch::PreferenceQuarterGain,
        Switch::PreferenceNearerStop,
        Switch::RadiiHoldStationary,
        Switch::UnkeptRadiusUnfinished,
        Switch::LandingPasses,
        Switch::NegligibleRows,
        Switch::RowScaling,
        Switch::LeaveStorage,
        Switch::BarWhereItStands,
        Switch::FoldScale,
        Switch::DampingKeepsDraggedHeavy,
        Switch::DraggedEndDropsRadius,
        Switch::DraggedEndDropsLength,
        Switch::TouchingFrozen,
        Switch::MinChangeTimes100,
        Switch::MinChangeOver100,
        Switch::DraggedMobilityTimes100,
        Switch::IterationCapTimes10,
        Switch::FoldMetTimes1000,
        Switch::UnfocusedMobility,
        Switch::ArcSweepTurn,
        Switch::CollapsedCountsAsTurned,
        Switch::TangentSides,
        Switch::DistanceSigns,
        Switch::Strides,
        Switch::SettleByThePointer,
        Switch::SettleFlingsNothing,
        Switch::SettleMeetsRelations,
        Switch::SettleKeepsHeldShapes,
        Switch::OneSolveFallback,
        Switch::HalveFailedSteps,
        Switch::HalveWanderingSteps,
        Switch::SplitSteps,
        Switch::UnfinishedIsRefused,
        Switch::TurnedIsRefused,
        Switch::CarriedLineFlip,
        Switch::SqueezedShut,
        Switch::LoweredLineFlip,
        Switch::CarryTurnedCurves,
        Switch::CarryHolding,
        Switch::PushedStaysPushed,
        Switch::FoldTry,
        Switch::LengthTry,
        Switch::HeldShapeStop,
        Switch::NeverGrow,
        Switch::CrushedLineStop,
        Switch::UnlandedRadiusHold,
        Switch::RefusedAgain,
        Switch::BarCap,
        Switch::KeptRadii,
        Switch::KeptLengths,
        Switch::SmoothAnchors,
        Switch::DragSolvesTimes10,
        Switch::UnfinishedWorkTimes10,
        Switch::UnfinishedWorkNone,
    ];

    /// The switch's name in `DRIFT_ABLATE` and the ledger.
    pub const fn name(self) -> &'static str {
        match self {
            Switch::KeepShapeWeight => "keep-shape-weight",
            Switch::KeepShapeTimes10 => "keep-shape-x10",
            Switch::KeepShapeOver10 => "keep-shape-over10",
            Switch::SecondOrderCorrection => "second-order-correction",
            Switch::StepLimit => "step-limit",
            Switch::DragBoundFromEntry => "drag-bound-from-entry",
            Switch::EditPreferenceBound => "edit-preference-bound",
            Switch::WanderingGain => "wandering-gain",
            Switch::PreferenceQuarterGain => "preference-quarter-gain",
            Switch::PreferenceNearerStop => "preference-nearer-stop",
            Switch::RadiiHoldStationary => "radii-hold-stationary",
            Switch::UnkeptRadiusUnfinished => "unkept-radius-unfinished",
            Switch::LandingPasses => "landing-passes",
            Switch::NegligibleRows => "negligible-rows",
            Switch::RowScaling => "row-scaling",
            Switch::LeaveStorage => "leave-storage",
            Switch::BarWhereItStands => "bar-where-it-stands",
            Switch::FoldScale => "fold-scale",
            Switch::DampingKeepsDraggedHeavy => "damping-keeps-dragged-heavy",
            Switch::DraggedEndDropsRadius => "dragged-end-drops-radius",
            Switch::DraggedEndDropsLength => "dragged-end-drops-length",
            Switch::TouchingFrozen => "touching-frozen",
            Switch::MinChangeTimes100 => "min-change-x100",
            Switch::MinChangeOver100 => "min-change-over100",
            Switch::DraggedMobilityTimes100 => "dragged-mobility-x100",
            Switch::IterationCapTimes10 => "iteration-cap-x10",
            Switch::FoldMetTimes1000 => "fold-met-x1000",
            Switch::UnfocusedMobility => "unfocused-mobility",
            Switch::ArcSweepTurn => "arc-sweep-turn",
            Switch::CollapsedCountsAsTurned => "collapsed-counts-as-turned",
            Switch::TangentSides => "tangent-sides",
            Switch::DistanceSigns => "distance-signs",
            Switch::Strides => "strides",
            Switch::SettleByThePointer => "settle-by-the-pointer",
            Switch::SettleFlingsNothing => "settle-flings-nothing",
            Switch::SettleMeetsRelations => "settle-meets-relations",
            Switch::SettleKeepsHeldShapes => "settle-keeps-held-shapes",
            Switch::OneSolveFallback => "one-solve-fallback",
            Switch::HalveFailedSteps => "halve-failed-steps",
            Switch::HalveWanderingSteps => "halve-wandering-steps",
            Switch::SplitSteps => "split-steps",
            Switch::UnfinishedIsRefused => "unfinished-is-refused",
            Switch::TurnedIsRefused => "turned-is-refused",
            Switch::CarriedLineFlip => "carried-line-flip",
            Switch::SqueezedShut => "squeezed-shut",
            Switch::LoweredLineFlip => "lowered-line-flip",
            Switch::CarryTurnedCurves => "carry-turned-curves",
            Switch::CarryHolding => "carry-holding",
            Switch::PushedStaysPushed => "pushed-stays-pushed",
            Switch::FoldTry => "fold-try",
            Switch::LengthTry => "length-try",
            Switch::HeldShapeStop => "held-shape-stop",
            Switch::NeverGrow => "never-grow",
            Switch::CrushedLineStop => "crushed-line-stop",
            Switch::UnlandedRadiusHold => "unlanded-radius-hold",
            Switch::RefusedAgain => "refused-again",
            Switch::BarCap => "bar-cap",
            Switch::KeptRadii => "kept-radii",
            Switch::KeptLengths => "kept-lengths",
            Switch::SmoothAnchors => "smooth-anchors",
            Switch::DragSolvesTimes10 => "drag-solves-x10",
            Switch::UnfinishedWorkTimes10 => "unfinished-work-x10",
            Switch::UnfinishedWorkNone => "unfinished-work-none",
        }
    }
}

/// Whether `switch` is thrown: its rule is off (or its constant changed) for this process.
/// Always false without the `ablate` feature.
#[cfg(not(feature = "ablate"))]
#[inline(always)]
pub const fn off(_switch: Switch) -> bool {
    false
}

/// Whether `switch` is thrown: named in `DRIFT_ABLATE` (read once a process).
#[cfg(feature = "ablate")]
pub fn off(switch: Switch) -> bool {
    static THROWN: std::sync::OnceLock<Vec<Switch>> = std::sync::OnceLock::new();
    THROWN
        .get_or_init(|| {
            let named = std::env::var("DRIFT_ABLATE").unwrap_or_default();
            named
                .split(',')
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(|n| {
                    *Switch::ALL
                        .iter()
                        .find(|s| s.name() == n)
                        .unwrap_or_else(|| panic!("DRIFT_ABLATE: no switch `{n}`"))
                })
                .collect()
        })
        .contains(&switch)
}

/// `value`, or `ablated` where `switch` is thrown: a constant under a switch.
#[inline(always)]
pub fn or<T>(switch: Switch, value: T, ablated: T) -> T {
    if off(switch) { ablated } else { value }
}

#[cfg(test)]
mod tests {
    use super::Switch;

    /// Every switch has a name of its own, and `ALL` lists each once.
    #[test]
    fn switches_are_named_once() {
        let mut names: Vec<&str> = Switch::ALL.iter().map(|s| s.name()).collect();
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n);
    }

    /// Without the feature nothing is ever thrown.
    #[cfg(not(feature = "ablate"))]
    #[test]
    fn nothing_is_thrown_without_the_feature() {
        assert!(Switch::ALL.iter().all(|s| !super::off(*s)));
    }
}
