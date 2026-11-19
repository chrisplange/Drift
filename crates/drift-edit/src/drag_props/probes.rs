//! Probes (not for the PR).
#![allow(unused_variables)]
use super::*;

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_10() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 15,
            len: 2,
        },
        Op::Arc {
            from: 8,
            angle: 19,
            radius: 2,
            sweep: 2,
            ccw: false,
        },
        Op::Step {
            from: 1,
            angle: 7,
            len: 0,
        },
        Op::OnArc {
            arc: 0,
            at: 7,
            angle: 5,
            len: 1,
        },
        Op::Step {
            from: 26,
            angle: 5,
            len: 0,
        },
    ],
    extras: vec![
        false,
        false,
        false,
        false,
        false,
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: Some(
        5,
    ),
    length: None,
};
    let pick = Pick::Curves(
    8542891717419927433,
);
    let offset = DVec2::new(
    0.0,
    13.199198872981677,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        -199.49299077678333,
        0.0,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_11() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 18,
            radius: 0,
            sweep: 4,
            ccw: true,
        },
        Op::Step {
            from: 8,
            angle: 11,
            len: 3,
        },
        Op::OnLine {
            line: 0,
            at: 0,
            angle: 19,
            len: 2,
        },
        Op::Arc {
            from: 21,
            angle: 17,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
        Op::Step {
            from: 0,
            angle: 0,
            len: 2,
        },
    ],
    extras: vec![
        false,
        false,
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: Some(
        60,
    ),
    length: None,
};
    let pick = Pick::Point(
    40,
);
    let offset = DVec2::new(
    -17.76537880740859,
    0.0,
);
    let steps = 6;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -288.6079639043261,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_12() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 22,
            len: 2,
        },
        Op::OnLine {
            line: 0,
            at: 1,
            angle: 10,
            len: 2,
        },
        Op::Dup {
            from: 27,
            angle: 7,
            len: 2,
        },
        Op::OnLine {
            line: 43,
            at: 1,
            angle: 9,
            len: 2,
        },
    ],
    extras: vec![
        false,
        false,
        true,
        false,
        false,
        true,
        true,
        false,
        false,
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: Some(
        26,
    ),
    length: None,
};
    let pick = Pick::Curves(
    15254425735770910903,
);
    let offset = DVec2::new(
    -8.42510877572246,
    -17.063400864507344,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -462.30850980444995,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_13() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 2,
            len: 0,
        },
        Op::Arc {
            from: 44,
            angle: 4,
            radius: 0,
            sweep: 2,
            ccw: true,
        },
        Op::Close {
            from: 3,
            to: 11,
        },
        Op::Dup {
            from: 3,
            angle: 15,
            len: 1,
        },
        Op::Step {
            from: 2,
            angle: 10,
            len: 3,
        },
        Op::OnLine {
            line: 19,
            at: 3,
            angle: 21,
            len: 2,
        },
        Op::Step {
            from: 45,
            angle: 16,
            len: 1,
        },
    ],
    extras: vec![
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: None,
    length: Some(
        28,
    ),
};
    let pick = Pick::Point(
    63,
);
    let offset = DVec2::new(
    11.094087675076649,
    -19.947824380961997,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        463.09904468684647,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_14() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 1,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
        Op::Arc {
            from: 52,
            angle: 1,
            radius: 0,
            sweep: 2,
            ccw: false,
        },
        Op::Arc {
            from: 33,
            angle: 17,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
        Op::Step {
            from: 0,
            angle: 2,
            len: 2,
        },
        Op::Close {
            from: 9,
            to: 58,
        },
        Op::Dup {
            from: 10,
            angle: 4,
            len: 1,
        },
        Op::Tangent {
            arc: 40,
            end: true,
            forward: false,
            len: 0,
        },
    ],
    extras: vec![
        false,
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        true,
        false,
        false,
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: Some(
        56,
    ),
    length: None,
};
    let pick = Pick::Curves(
    10209933662206102316,
);
    let offset = DVec2::new(
    0.0,
    17.546300846956495,
);
    let steps = 20;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        94.93915121554136,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_16() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 8,
            radius: 1,
            sweep: 4,
            ccw: true,
        },
        Op::Arc {
            from: 1,
            angle: 21,
            radius: 1,
            sweep: 3,
            ccw: false,
        },
        Op::Step {
            from: 47,
            angle: 7,
            len: 1,
        },
        Op::OnArc {
            arc: 30,
            at: 11,
            angle: 11,
            len: 2,
        },
        Op::Mid {
            line: 2,
            angle: 7,
            len: 0,
        },
    ],
    extras: vec![
        false,
        false,
        true,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: Some(
        27,
    ),
    length: None,
};
    let pick = Pick::Curves(
    12005400971653210572,
);
    let offset = DVec2::new(
    0.0,
    14.174323292153025,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 21.40432408154111,
    shift: DVec2::new(
        367.1844252856773,
        0.0,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_17() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 11,
            radius: 2,
            sweep: 1,
            ccw: true,
        },
        Op::Step {
            from: 0,
            angle: 10,
            len: 1,
        },
        Op::Step {
            from: 2,
            angle: 13,
            len: 1,
        },
        Op::Arc {
            from: 26,
            angle: 2,
            radius: 1,
            sweep: 2,
            ccw: false,
        },
        Op::Arc {
            from: 41,
            angle: 2,
            radius: 1,
            sweep: 1,
            ccw: false,
        },
        Op::Tangent {
            arc: 54,
            end: false,
            forward: false,
            len: 0,
        },
    ],
    extras: vec![
        true,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: Some(
        9,
    ),
    length: None,
};
    let pick = Pick::Curves(
    8859355852990347992,
);
    let offset = DVec2::new(
    13.237830586994622,
    -2.0329626285582023,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        7.291562797100103,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_18() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 15,
            radius: 1,
            sweep: 0,
            ccw: false,
        },
        Op::Arc {
            from: 20,
            angle: 6,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::Step {
            from: 2,
            angle: 1,
            len: 0,
        },
    ],
    extras: vec![
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Point(
    43,
);
    let offset = DVec2::new(
    18.642192258397074,
    -15.150142334258259,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 70.37604266271786,
    shift: DVec2::new(
        0.0,
        0.0,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_19() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 9,
            radius: 0,
            sweep: 2,
            ccw: true,
        },
        Op::Arc {
            from: 44,
            angle: 8,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::Tangent {
            arc: 47,
            end: false,
            forward: false,
            len: 2,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: false,
            len: 0,
        },
        Op::Step {
            from: 2,
            angle: 13,
            len: 0,
        },
    ],
    extras: vec![
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: Some(
        44,
    ),
    length: None,
};
    let pick = Pick::Curves(
    12883702180960982740,
);
    let offset = DVec2::new(
    0.0,
    -16.848940102060517,
);
    let steps = 11;
    let t = Similarity {
    scale: 0.25,
    turn: 207.2585326113549,
    shift: DVec2::new(
        0.0,
        0.0,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_2() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 19,
            radius: 2,
            sweep: 2,
            ccw: false,
        },
        Op::OnArc {
            arc: 0,
            at: 1,
            angle: 18,
            len: 2,
        },
        Op::Step {
            from: 11,
            angle: 5,
            len: 0,
        },
        Op::Mid {
            line: 14,
            angle: 3,
            len: 0,
        },
        Op::OnLine {
            line: 34,
            at: 0,
            angle: 7,
            len: 2,
        },
        Op::Arc {
            from: 1,
            angle: 7,
            radius: 1,
            sweep: 3,
            ccw: true,
        },
        Op::Close {
            from: 33,
            to: 6,
        },
    ],
    extras: vec![
        false,
        false,
        true,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: Some(
        58,
    ),
    length: None,
};
    let pick = Pick::Curves(
    1079296240429988705,
);
    let offset = DVec2::new(
    11.33064892758793,
    -7.0092076192390955,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -169.72930566149037,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_20() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 3,
            radius: 1,
            sweep: 2,
            ccw: false,
        },
        Op::Step {
            from: 0,
            angle: 3,
            len: 1,
        },
        Op::OnArc {
            arc: 0,
            at: 17,
            angle: 22,
            len: 2,
        },
        Op::Step {
            from: 5,
            angle: 6,
            len: 1,
        },
        Op::Step {
            from: 36,
            angle: 12,
            len: 0,
        },
        Op::Step {
            from: 20,
            angle: 16,
            len: 1,
        },
    ],
    extras: vec![
        true,
        true,
        false,
        false,
        false,
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    10623740333444224106,
);
    let offset = DVec2::new(
    -18.880935808342876,
    0.0,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        178.4077126183674,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_3() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 20,
            len: 2,
        },
        Op::Step {
            from: 5,
            angle: 20,
            len: 3,
        },
        Op::Step {
            from: 3,
            angle: 23,
            len: 3,
        },
        Op::Dup {
            from: 37,
            angle: 7,
            len: 3,
        },
        Op::Arc {
            from: 2,
            angle: 8,
            radius: 1,
            sweep: 4,
            ccw: false,
        },
    ],
    extras: vec![
        false,
        false,
        true,
        true,
        true,
        false,
        false,
        false,
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Point(
    0,
);
    let offset = DVec2::new(
    12.00402710845554,
    -17.81193061815077,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -396.98609236523913,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_4() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 18,
            radius: 0,
            sweep: 0,
            ccw: true,
        },
        Op::Step {
            from: 11,
            angle: 10,
            len: 0,
        },
        Op::OnLine {
            line: 0,
            at: 0,
            angle: 22,
            len: 1,
        },
        Op::OnArc {
            arc: 0,
            at: 5,
            angle: 2,
            len: 0,
        },
        Op::Dup {
            from: 8,
            angle: 9,
            len: 1,
        },
        Op::Mid {
            line: 22,
            angle: 0,
            len: 0,
        },
        Op::OnArc {
            arc: 0,
            at: 15,
            angle: 7,
            len: 0,
        },
    ],
    extras: vec![
        true,
        false,
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    5312934562240649240,
);
    let offset = DVec2::new(
    17.36197153472971,
    15.910133147337284,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -100.3680343561654,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_5() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 5,
            radius: 0,
            sweep: 2,
            ccw: true,
        },
        Op::Step {
            from: 22,
            angle: 2,
            len: 0,
        },
        Op::OnLine {
            line: 0,
            at: 1,
            angle: 3,
            len: 0,
        },
        Op::Arc {
            from: 2,
            angle: 4,
            radius: 0,
            sweep: 0,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: false,
            len: 2,
        },
        Op::OnArc {
            arc: 30,
            at: 15,
            angle: 13,
            len: 0,
        },
        Op::Step {
            from: 9,
            angle: 10,
            len: 1,
        },
    ],
    extras: vec![
        false,
        false,
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: Some(
        9,
    ),
    length: None,
};
    let pick = Pick::Curves(
    7710219322523347356,
);
    let offset = DVec2::new(
    -19.396793234146298,
    14.22082754429065,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 30.510478411680264,
    shift: DVec2::new(
        0.0,
        196.7343444219725,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_6() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 21,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::Step {
            from: 46,
            angle: 7,
            len: 2,
        },
        Op::Arc {
            from: 2,
            angle: 11,
            radius: 2,
            sweep: 3,
            ccw: false,
        },
        Op::Step {
            from: 0,
            angle: 19,
            len: 3,
        },
        Op::OnArc {
            arc: 0,
            at: 8,
            angle: 5,
            len: 1,
        },
        Op::OnArc {
            arc: 14,
            at: 8,
            angle: 0,
            len: 2,
        },
        Op::Close {
            from: 49,
            to: 59,
        },
    ],
    extras: vec![
        false,
        true,
        false,
        false,
        true,
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    16892029386315100904,
);
    let offset = DVec2::new(
    -15.357097659669554,
    3.2908174150228318,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -448.7091463631882,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_7() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 6,
            radius: 0,
            sweep: 0,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: false,
            len: 3,
        },
        Op::Step {
            from: 10,
            angle: 10,
            len: 2,
        },
    ],
    extras: vec![
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: Some(
        5,
    ),
    length: None,
};
    let pick = Pick::Curves(
    9064916502261783346,
);
    let offset = DVec2::new(
    -12.604753358758412,
    0.0,
);
    let steps = 2;
    let t = Similarity {
    scale: 0.25,
    turn: 179.56011528623327,
    shift: DVec2::new(
        0.0,
        0.0,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_8() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 3,
            radius: 2,
            sweep: 3,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: false,
            len: 3,
        },
        Op::Step {
            from: 29,
            angle: 13,
            len: 1,
        },
        Op::OnLine {
            line: 1,
            at: 3,
            angle: 8,
            len: 0,
        },
    ],
    extras: vec![
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    1425371450287138964,
);
    let offset = DVec2::new(
    -12.117531591216181,
    -18.0036231092522,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        -58.90107550604262,
        0.0,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_9() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 15,
            len: 3,
        },
        Op::OnLine {
            line: 0,
            at: 0,
            angle: 21,
            len: 3,
        },
        Op::Step {
            from: 2,
            angle: 16,
            len: 0,
        },
        Op::Arc {
            from: 9,
            angle: 14,
            radius: 0,
            sweep: 0,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: false,
            len: 3,
        },
        Op::Mid {
            line: 46,
            angle: 1,
            len: 1,
        },
    ],
    extras: vec![
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    8220936942135468612,
);
    let offset = DVec2::new(
    -14.400469524983192,
    -6.911382640601451,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 206.65827902376716,
    shift: DVec2::new(
        53.47899219770963,
        0.0,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_lands_exactly_8() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 0,
            radius: 2,
            sweep: 2,
            ccw: false,
        },
        Op::Arc {
            from: 0,
            angle: 0,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::Arc {
            from: 38,
            angle: 0,
            radius: 2,
            sweep: 4,
            ccw: false,
        },
        Op::Mid {
            line: 0,
            angle: 0,
            len: 0,
        },
        Op::Tangent {
            arc: 2,
            end: false,
            forward: false,
            len: 0,
        },
    ],
    extras: vec![
        false,
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    17427008508036965285,
);
    let offset = DVec2::new(
    -13.696429046222605,
    0.0,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_lands_exactly_9() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 16,
            len: 0,
        },
        Op::Dup {
            from: 30,
            angle: 20,
            len: 0,
        },
        Op::Step {
            from: 6,
            angle: 6,
            len: 1,
        },
        Op::Arc {
            from: 17,
            angle: 7,
            radius: 1,
            sweep: 1,
            ccw: false,
        },
        Op::Step {
            from: 35,
            angle: 19,
            len: 1,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: false,
            len: 3,
        },
    ],
    extras: vec![
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        true,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    8380422305955308544,
);
    let offset = DVec2::new(
    -11.667140951098396,
    14.843085762833717,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}
