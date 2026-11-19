//! Probes (not for the PR).
#![allow(unused_variables)]
use super::*;

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_1() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 8,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::OnArc {
            arc: 0,
            at: 12,
            angle: 22,
            len: 1,
        },
        Op::OnArc {
            arc: 0,
            at: 7,
            angle: 9,
            len: 2,
        },
        Op::Arc {
            from: 63,
            angle: 7,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
        Op::Step {
            from: 15,
            angle: 7,
            len: 0,
        },
        Op::Step {
            from: 31,
            angle: 4,
            len: 0,
        },
        Op::Arc {
            from: 27,
            angle: 6,
            radius: 0,
            sweep: 4,
            ccw: false,
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
        7,
    ),
    length: Some(
        2,
    ),
};
    let pick = Pick::Curves(
    10065990343282870640,
);
    let offset = DVec2::new(
    10.288533804145723,
    -7.056799166222707,
);
    let steps = 11;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -159.41195975568405,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_10() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 0,
            radius: 2,
            sweep: 0,
            ccw: false,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: true,
            len: 1,
        },
        Op::OnLine {
            line: 0,
            at: 0,
            angle: 12,
            len: 1,
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
    1525739527724061840,
);
    let offset = DVec2::new(
    16.11006164633332,
    -18.61219490296701,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -111.77129656101158,
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
            angle: 21,
            radius: 2,
            sweep: 0,
            ccw: true,
        },
        Op::Close {
            from: 10,
            to: 5,
        },
        Op::Arc {
            from: 8,
            angle: 7,
            radius: 1,
            sweep: 2,
            ccw: false,
        },
        Op::OnArc {
            arc: 2,
            at: 17,
            angle: 10,
            len: 2,
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
    fix: Some(
        34,
    ),
    length: Some(
        5,
    ),
};
    let pick = Pick::Curves(
    3382641225126799350,
);
    let offset = DVec2::new(
    15.904003619499168,
    10.03725811759831,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.3993630449873273,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        0.0,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_12() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 5,
            radius: 1,
            sweep: 0,
            ccw: false,
        },
        Op::Step {
            from: 2,
            angle: 18,
            len: 3,
        },
        Op::Step {
            from: 12,
            angle: 1,
            len: 0,
        },
        Op::Dup {
            from: 47,
            angle: 2,
            len: 3,
        },
        Op::Step {
            from: 36,
            angle: 12,
            len: 3,
        },
    ],
    extras: vec![
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
        false,
    ],
    fix: Some(
        2,
    ),
    length: None,
};
    let pick = Pick::Curves(
    1016026951364103818,
);
    let offset = DVec2::new(
    -5.044536981434896,
    -0.9796154624751747,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -281.859714523158,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_13() {
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
            end: false,
            forward: false,
            len: 0,
        },
        Op::Arc {
            from: 3,
            angle: 1,
            radius: 0,
            sweep: 3,
            ccw: true,
        },
        Op::Tangent {
            arc: 19,
            end: true,
            forward: false,
            len: 0,
        },
        Op::Arc {
            from: 45,
            angle: 10,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
        Op::Mid {
            line: 6,
            angle: 1,
            len: 1,
        },
        Op::Step {
            from: 25,
            angle: 20,
            len: 2,
        },
    ],
    extras: vec![
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
    5128282379412255862,
);
    let offset = DVec2::new(
    0.0,
    17.14384009465568,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 290.8526825690248,
    shift: DVec2::new(
        -436.12118930533916,
        0.0,
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
            ccw: true,
        },
        Op::Arc {
            from: 8,
            angle: 0,
            radius: 1,
            sweep: 2,
            ccw: false,
        },
        Op::Arc {
            from: 21,
            angle: 3,
            radius: 0,
            sweep: 1,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: false,
            len: 0,
        },
        Op::Step {
            from: 10,
            angle: 1,
            len: 1,
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
        6,
    ),
    length: None,
};
    let pick = Pick::Curves(
    10629369874005498008,
);
    let offset = DVec2::new(
    0.0,
    19.214828477924033,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        9.991755244896693,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_15() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 19,
            radius: 2,
            sweep: 3,
            ccw: true,
        },
        Op::Close {
            from: 8,
            to: 63,
        },
        Op::Dup {
            from: 55,
            angle: 2,
            len: 3,
        },
        Op::Arc {
            from: 15,
            angle: 7,
            radius: 0,
            sweep: 3,
            ccw: false,
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
    length: Some(
        0,
    ),
};
    let pick = Pick::Point(
    17,
);
    let offset = DVec2::new(
    0.0,
    16.11552404544131,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        97.85476182765198,
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
            angle: 1,
            radius: 0,
            sweep: 0,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: false,
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
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    3854403240087446898,
);
    let offset = DVec2::new(
    19.95337899700048,
    0.0,
);
    let steps = 5;
    let t = Similarity {
    scale: 2.223105460691426,
    turn: 260.65013569128837,
    shift: DVec2::new(
        -192.1886635389314,
        148.2026574770012,
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
            angle: 9,
            len: 3,
        },
        Op::Step {
            from: 2,
            angle: 3,
            len: 1,
        },
        Op::Arc {
            from: 26,
            angle: 4,
            radius: 1,
            sweep: 1,
            ccw: false,
        },
        Op::Arc {
            from: 21,
            angle: 0,
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
    fix: Some(
        5,
    ),
    length: None,
};
    let pick = Pick::Curves(
    8894101318868705996,
);
    let offset = DVec2::new(
    13.237830586994622,
    -2.0329626285582023,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 193.1478947109641,
    shift: DVec2::new(
        -334.6630856257933,
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
            angle: 1,
            radius: 2,
            sweep: 3,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: false,
            len: 0,
        },
        Op::OnArc {
            arc: 0,
            at: 9,
            angle: 6,
            len: 2,
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
        5,
    ),
    length: None,
};
    let pick = Pick::Curves(
    11075254483997449416,
);
    let offset = DVec2::new(
    17.49951309706815,
    -18.57985068370464,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -375.89806640396716,
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
            angle: 0,
            radius: 0,
            sweep: 0,
            ccw: true,
        },
        Op::Arc {
            from: 0,
            angle: 11,
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
            arc: 29,
            end: true,
            forward: false,
            len: 0,
        },
        Op::Step {
            from: 48,
            angle: 7,
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
    12833178826427211196,
);
    let offset = DVec2::new(
    0.0,
    -16.848940102060517,
);
    let steps = 10;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -301.8742106640645,
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
            angle: 12,
            radius: 1,
            sweep: 0,
            ccw: false,
        },
        Op::Arc {
            from: 42,
            angle: 1,
            radius: 1,
            sweep: 2,
            ccw: false,
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
        54,
    ),
    length: None,
};
    let pick = Pick::Curves(
    6328875866429277848,
);
    let offset = DVec2::new(
    13.932255692653552,
    -10.558395796640877,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        65.51783020465035,
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
            angle: 2,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
        Op::Dup {
            from: 26,
            angle: 18,
            len: 0,
        },
        Op::Arc {
            from: 16,
            angle: 3,
            radius: 1,
            sweep: 3,
            ccw: false,
        },
        Op::Step {
            from: 6,
            angle: 5,
            len: 1,
        },
        Op::Arc {
            from: 26,
            angle: 10,
            radius: 1,
            sweep: 2,
            ccw: true,
        },
        Op::Arc {
            from: 0,
            angle: 11,
            radius: 0,
            sweep: 1,
            ccw: false,
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
    7191583418865756395,
);
    let offset = DVec2::new(
    -19.86607096430728,
    0.0,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        392.7067819419172,
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
            angle: 2,
            len: 0,
        },
        Op::Arc {
            from: 0,
            angle: 7,
            radius: 0,
            sweep: 4,
            ccw: false,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: false,
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
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    10936582848978100045,
);
    let offset = DVec2::new(
    0.0,
    13.180315347014067,
);
    let steps = 1;
    let t = Similarity {
    scale: 2.7587318482912524,
    turn: 0.0,
    shift: DVec2::new(
        -34.75871007751519,
        -31.429968887897854,
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
            angle: 13,
            radius: 2,
            sweep: 2,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: true,
            len: 1,
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
        11,
    ),
    length: None,
};
    let pick = Pick::Point(
    21,
);
    let offset = DVec2::new(
    -14.356874404974034,
    0.0,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        130.73708165692838,
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
            angle: 14,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::OnArc {
            arc: 0,
            at: 19,
            angle: 10,
            len: 3,
        },
        Op::OnArc {
            arc: 0,
            at: 18,
            angle: 0,
            len: 2,
        },
        Op::Dup {
            from: 11,
            angle: 18,
            len: 2,
        },
        Op::Step {
            from: 6,
            angle: 16,
            len: 2,
        },
        Op::Arc {
            from: 10,
            angle: 0,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
    ],
    extras: vec![
        false,
        false,
        false,
        true,
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
    ],
    fix: None,
    length: Some(
        22,
    ),
};
    let pick = Pick::Curves(
    11422869152310651650,
);
    let offset = DVec2::new(
    11.477088119349938,
    -6.1217371714278555,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -67.39516998388069,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_6() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 1,
            len: 0,
        },
        Op::Step {
            from: 0,
            angle: 4,
            len: 0,
        },
        Op::Arc {
            from: 3,
            angle: 22,
            radius: 1,
            sweep: 0,
            ccw: false,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: false,
            len: 1,
        },
        Op::Arc {
            from: 57,
            angle: 0,
            radius: 1,
            sweep: 0,
            ccw: false,
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
    fix: Some(
        62,
    ),
    length: None,
};
    let pick = Pick::Curves(
    1128139025625961759,
);
    let offset = DVec2::new(
    -15.139956099429632,
    -12.491835200828582,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -55.56914557490204,
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
            len: 2,
        },
        Op::Step {
            from: 0,
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
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -7.8753507117796655,
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
            angle: 14,
            radius: 1,
            sweep: 3,
            ccw: true,
        },
        Op::Step {
            from: 52,
            angle: 2,
            len: 1,
        },
        Op::Step {
            from: 27,
            angle: 15,
            len: 1,
        },
        Op::Arc {
            from: 13,
            angle: 17,
            radius: 1,
            sweep: 2,
            ccw: true,
        },
        Op::Arc {
            from: 53,
            angle: 5,
            radius: 0,
            sweep: 1,
            ccw: true,
        },
        Op::Close {
            from: 6,
            to: 58,
        },
    ],
    extras: vec![
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
        false,
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    16827383550004596610,
);
    let offset = DVec2::new(
    0.0,
    13.158608658614895,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -320.7714281583088,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_9() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 22,
            radius: 1,
            sweep: 1,
            ccw: false,
        },
        Op::Step {
            from: 9,
            angle: 5,
            len: 2,
        },
        Op::Dup {
            from: 1,
            angle: 9,
            len: 0,
        },
        Op::Step {
            from: 32,
            angle: 13,
            len: 3,
        },
        Op::Arc {
            from: 38,
            angle: 12,
            radius: 1,
            sweep: 0,
            ccw: false,
        },
        Op::OnLine {
            line: 15,
            at: 0,
            angle: 5,
            len: 1,
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
    9535965612356046434,
);
    let offset = DVec2::new(
    2.97685558995395,
    19.259529728512973,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        15.65443588826399,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_10() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 13,
            len: 0,
        },
        Op::OnLine {
            line: 0,
            at: 2,
            angle: 6,
            len: 1,
        },
        Op::Mid {
            line: 6,
            angle: 2,
            len: 0,
        },
        Op::Arc {
            from: 0,
            angle: 7,
            radius: 0,
            sweep: 0,
            ccw: false,
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
    12627255342864666180,
);
    let offset = DVec2::new(
    -17.99131491084439,
    -8.791946104127675,
);
    let steps = 16;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_11() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 7,
            len: 0,
        },
        Op::Arc {
            from: 14,
            angle: 4,
            radius: 0,
            sweep: 0,
            ccw: true,
        },
        Op::Arc {
            from: 52,
            angle: 1,
            radius: 1,
            sweep: 4,
            ccw: false,
        },
        Op::OnArc {
            arc: 1,
            at: 1,
            angle: 2,
            len: 1,
        },
        Op::Arc {
            from: 2,
            angle: 2,
            radius: 1,
            sweep: 0,
            ccw: false,
        },
        Op::Mid {
            line: 0,
            angle: 12,
            len: 2,
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
    5516951922033952777,
);
    let offset = DVec2::new(
    0.0,
    -1.2833679470914074,
);
    let steps = 4;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_12() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 19,
            radius: 2,
            sweep: 2,
            ccw: true,
        },
        Op::Step {
            from: 0,
            angle: 5,
            len: 2,
        },
        Op::Arc {
            from: 5,
            angle: 16,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::Arc {
            from: 29,
            angle: 12,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
        Op::Arc {
            from: 7,
            angle: 15,
            radius: 1,
            sweep: 0,
            ccw: true,
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
        0,
    ),
};
    let pick = Pick::Curves(
    10201672322576563460,
);
    let offset = DVec2::new(
    -18.521334946423842,
    5.02290630595195,
);
    let steps = 1;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_13() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 19,
            len: 0,
        },
        Op::Close {
            from: 0,
            to: 0,
        },
        Op::OnLine {
            line: 0,
            at: 3,
            angle: 1,
            len: 0,
        },
        Op::Arc {
            from: 11,
            angle: 5,
            radius: 1,
            sweep: 3,
            ccw: false,
        },
        Op::Mid {
            line: 0,
            angle: 5,
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
        31,
    ),
};
    let pick = Pick::Curves(
    2794052978846529328,
);
    let offset = DVec2::new(
    18.273769071517684,
    1.1548929265517232,
);
    let steps = 17;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_14() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 5,
            len: 0,
        },
        Op::Arc {
            from: 2,
            angle: 18,
            radius: 1,
            sweep: 2,
            ccw: false,
        },
        Op::Arc {
            from: 11,
            angle: 15,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
        Op::Step {
            from: 62,
            angle: 1,
            len: 2,
        },
        Op::Dup {
            from: 53,
            angle: 11,
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
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
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
    8084063589943745168,
);
    let offset = DVec2::new(
    15.95219859373592,
    -15.75470160524986,
);
    let steps = 1;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_16() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 0,
            len: 2,
        },
        Op::Arc {
            from: 20,
            angle: 12,
            radius: 1,
            sweep: 0,
            ccw: false,
        },
        Op::Step {
            from: 54,
            angle: 1,
            len: 0,
        },
        Op::OnLine {
            line: 1,
            at: 0,
            angle: 6,
            len: 0,
        },
        Op::Step {
            from: 5,
            angle: 2,
            len: 1,
        },
        Op::Step {
            from: 0,
            angle: 5,
            len: 0,
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
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
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
    8439892703005323464,
);
    let offset = DVec2::new(
    -6.856511010721301,
    -11.393471313184971,
);
    let steps = 8;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_17() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 11,
            len: 3,
        },
        Op::Step {
            from: 31,
            angle: 5,
            len: 0,
        },
        Op::Step {
            from: 11,
            angle: 8,
            len: 0,
        },
        Op::OnLine {
            line: 9,
            at: 1,
            angle: 17,
            len: 0,
        },
        Op::Step {
            from: 47,
            angle: 17,
            len: 3,
        },
        Op::Arc {
            from: 6,
            angle: 8,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
    ],
    extras: vec![
        false,
        true,
        true,
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
        false,
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    5049026713141105538,
);
    let offset = DVec2::new(
    0.0,
    10.796545212589107,
);
    let steps = 12;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_18() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 0,
            len: 0,
        },
        Op::Arc {
            from: 0,
            angle: 10,
            radius: 0,
            sweep: 3,
            ccw: false,
        },
        Op::Arc {
            from: 27,
            angle: 0,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::Arc {
            from: 6,
            angle: 0,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::Arc {
            from: 49,
            angle: 0,
            radius: 0,
            sweep: 0,
            ccw: false,
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
    ],
    fix: None,
    length: Some(
        0,
    ),
};
    let pick = Pick::Curves(
    265370227558375624,
);
    let offset = DVec2::new(
    0.0,
    9.084806060733136,
);
    let steps = 17;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_19() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 3,
            len: 0,
        },
        Op::Step {
            from: 6,
            angle: 10,
            len: 3,
        },
        Op::Arc {
            from: 8,
            angle: 14,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
        Op::OnLine {
            line: 11,
            at: 0,
            angle: 9,
            len: 0,
        },
        Op::Close {
            from: 14,
            to: 3,
        },
        Op::Mid {
            line: 1,
            angle: 19,
            len: 0,
        },
        Op::Step {
            from: 5,
            angle: 2,
            len: 0,
        },
    ],
    extras: vec![
        true,
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
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    10473991341903593568,
);
    let offset = DVec2::new(
    14.040497250204181,
    0.0,
);
    let steps = 16;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_2() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 21,
            len: 0,
        },
        Op::Arc {
            from: 19,
            angle: 15,
            radius: 1,
            sweep: 1,
            ccw: false,
        },
        Op::OnLine {
            line: 0,
            at: 0,
            angle: 9,
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
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    14256598342702470608,
);
    let offset = DVec2::new(
    -19.35341121281802,
    16.867886011438923,
);
    let steps = 1;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_20() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 21,
            radius: 2,
            sweep: 1,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: true,
            len: 1,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: true,
            len: 2,
        },
        Op::Step {
            from: 3,
            angle: 3,
            len: 0,
        },
        Op::OnLine {
            line: 25,
            at: 0,
            angle: 0,
            len: 2,
        },
        Op::Step {
            from: 6,
            angle: 1,
            len: 1,
        },
        Op::OnLine {
            line: 25,
            at: 3,
            angle: 14,
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
    ],
    fix: None,
    length: Some(
        6,
    ),
};
    let pick = Pick::Curves(
    12643302001501943080,
);
    let offset = DVec2::new(
    0.0,
    13.532395388807604,
);
    let steps = 4;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_3() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 10,
            radius: 1,
            sweep: 4,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: false,
            len: 0,
        },
        Op::OnLine {
            line: 0,
            at: 2,
            angle: 5,
            len: 2,
        },
        Op::OnArc {
            arc: 0,
            at: 6,
            angle: 10,
            len: 1,
        },
        Op::Step {
            from: 56,
            angle: 15,
            len: 1,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: false,
            len: 0,
        },
        Op::Step {
            from: 46,
            angle: 6,
            len: 0,
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
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    17081623590518978804,
);
    let offset = DVec2::new(
    -18.294616183931822,
    -11.375128908909549,
);
    let steps = 9;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_4() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 3,
            len: 3,
        },
        Op::Arc {
            from: 44,
            angle: 2,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
        Op::Step {
            from: 12,
            angle: 14,
            len: 3,
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
    11002469908369975697,
);
    let offset = DVec2::new(
    17.253992557603112,
    18.294368927708504,
);
    let steps = 1;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_5() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 2,
            radius: 1,
            sweep: 0,
            ccw: true,
        },
        Op::Close {
            from: 21,
            to: 1,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: false,
            len: 0,
        },
        Op::Arc {
            from: 19,
            angle: 13,
            radius: 0,
            sweep: 2,
            ccw: false,
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
    length: Some(
        1,
    ),
};
    let pick = Pick::Curves(
    9722073638275638032,
);
    let offset = DVec2::new(
    0.0,
    16.274127363274737,
);
    let steps = 8;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_6() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 14,
            radius: 1,
            sweep: 0,
            ccw: true,
        },
        Op::Step {
            from: 21,
            angle: 14,
            len: 2,
        },
        Op::Step {
            from: 4,
            angle: 13,
            len: 1,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: true,
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
    12231441010695515532,
);
    let offset = DVec2::new(
    -15.524642573569922,
    -17.121216759015404,
);
    let steps = 11;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_7() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 10,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::Step {
            from: 0,
            angle: 19,
            len: 0,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: false,
            len: 0,
        },
        Op::Dup {
            from: 0,
            angle: 0,
            len: 0,
        },
        Op::Arc {
            from: 53,
            angle: 2,
            radius: 1,
            sweep: 2,
            ccw: true,
        },
        Op::Step {
            from: 8,
            angle: 0,
            len: 1,
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
    length: Some(
        59,
    ),
};
    let pick = Pick::Curves(
    12757746845559378145,
);
    let offset = DVec2::new(
    -10.485923366426523,
    -19.98814570540301,
);
    let steps = 1;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_9() {
    let plan = Plan {
    ops: vec![
        Op::Dup {
            from: 0,
            angle: 13,
            len: 1,
        },
        Op::Step {
            from: 0,
            angle: 1,
            len: 2,
        },
        Op::Arc {
            from: 0,
            angle: 17,
            radius: 1,
            sweep: 2,
            ccw: false,
        },
        Op::Arc {
            from: 47,
            angle: 18,
            radius: 2,
            sweep: 1,
            ccw: true,
        },
        Op::Step {
            from: 4,
            angle: 7,
            len: 2,
        },
        Op::Step {
            from: 4,
            angle: 7,
            len: 3,
        },
        Op::Tangent {
            arc: 5,
            end: false,
            forward: false,
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
    ],
    fix: None,
    length: Some(
        2,
    ),
};
    let pick = Pick::Curves(
    5619395973840135329,
);
    let offset = DVec2::new(
    7.205639301388648,
    -11.284915483250655,
);
    let steps = 11;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_lands_exactly_1() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 0,
            radius: 0,
            sweep: 4,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: false,
            len: 1,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: true,
            len: 1,
        },
    ],
    extras: vec![
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
        false,
        false,
        false,
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    15663209691284564886,
);
    let offset = DVec2::new(
    0.0,
    -16.33629351607285,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_lands_exactly_12() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 7,
            len: 2,
        },
        Op::Step {
            from: 1,
            angle: 13,
            len: 2,
        },
        Op::Arc {
            from: 54,
            angle: 7,
            radius: 1,
            sweep: 2,
            ccw: true,
        },
        Op::Step {
            from: 3,
            angle: 13,
            len: 3,
        },
        Op::Dup {
            from: 5,
            angle: 0,
            len: 1,
        },
        Op::Step {
            from: 13,
            angle: 6,
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
    ],
    fix: None,
    length: Some(
        26,
    ),
};
    let pick = Pick::Curves(
    17712052727690060107,
);
    let offset = DVec2::new(
    16.175402890323834,
    2.9543768057123887,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_lands_exactly_15() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 0,
            radius: 1,
            sweep: 2,
            ccw: true,
        },
        Op::Dup {
            from: 3,
            angle: 3,
            len: 1,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: true,
            len: 0,
        },
        Op::Step {
            from: 35,
            angle: 21,
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
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
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
        0,
    ),
};
    let pick = Pick::Curves(
    14132326385741017380,
);
    let offset = DVec2::new(
    -18.48669643854186,
    13.380105539490623,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_lands_exactly_16() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 8,
            radius: 1,
            sweep: 3,
            ccw: true,
        },
        Op::Arc {
            from: 28,
            angle: 5,
            radius: 2,
            sweep: 2,
            ccw: false,
        },
        Op::Close {
            from: 20,
            to: 17,
        },
        Op::Arc {
            from: 5,
            angle: 6,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
        Op::Step {
            from: 2,
            angle: 1,
            len: 0,
        },
        Op::Arc {
            from: 24,
            angle: 2,
            radius: 0,
            sweep: 1,
            ccw: true,
        },
    ],
    extras: vec![
        true,
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
    ],
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    17772483210852823002,
);
    let offset = DVec2::new(
    18.487533993936157,
    -5.140359274412093,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_lands_exactly_17() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 0,
            radius: 0,
            sweep: 4,
            ccw: true,
        },
        Op::Arc {
            from: 3,
            angle: 16,
            radius: 1,
            sweep: 0,
            ccw: true,
        },
        Op::Step {
            from: 13,
            angle: 8,
            len: 0,
        },
        Op::Dup {
            from: 6,
            angle: 8,
            len: 1,
        },
        Op::Close {
            from: 3,
            to: 41,
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
    length: None,
};
    let pick = Pick::Curves(
    2400626280698094356,
);
    let offset = DVec2::new(
    -17.689647163214243,
    -13.080805407370635,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_lands_exactly_19() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 1,
            len: 2,
        },
        Op::Arc {
            from: 30,
            angle: 4,
            radius: 2,
            sweep: 0,
            ccw: true,
        },
        Op::Dup {
            from: 12,
            angle: 18,
            len: 1,
        },
        Op::OnArc {
            arc: 0,
            at: 10,
            angle: 0,
            len: 1,
        },
        Op::Step {
            from: 3,
            angle: 3,
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
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    11768645331360729312,
);
    let offset = DVec2::new(
    9.100391438533743,
    16.552811334544778,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_lands_exactly_2() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 17,
            radius: 1,
            sweep: 4,
            ccw: false,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: false,
            len: 1,
        },
        Op::Close {
            from: 20,
            to: 1,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: false,
            len: 0,
        },
        Op::Mid {
            line: 21,
            angle: 2,
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
    8965850577055103864,
);
    let offset = DVec2::new(
    0.0,
    14.136575126968191,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_lands_exactly_20() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 16,
            len: 1,
        },
        Op::Step {
            from: 6,
            angle: 5,
            len: 0,
        },
        Op::Arc {
            from: 28,
            angle: 6,
            radius: 0,
            sweep: 0,
            ccw: false,
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
    length: Some(
        0,
    ),
};
    let pick = Pick::Curves(
    16362910674616911799,
);
    let offset = DVec2::new(
    -7.575187903144644,
    -11.733754204842114,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_lands_exactly_3() {
    let plan = Plan {
    ops: vec![
        Op::Dup {
            from: 0,
            angle: 10,
            len: 0,
        },
        Op::Step {
            from: 0,
            angle: 4,
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
    length: None,
};
    let pick = Pick::Curves(
    4121368605459559768,
);
    let offset = DVec2::new(
    0.0,
    19.991456814470236,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_lands_exactly_4() {
    let plan = Plan {
    ops: vec![
        Op::Dup {
            from: 0,
            angle: 2,
            len: 1,
        },
        Op::Arc {
            from: 3,
            angle: 8,
            radius: 1,
            sweep: 2,
            ccw: false,
        },
        Op::Arc {
            from: 25,
            angle: 13,
            radius: 0,
            sweep: 1,
            ccw: true,
        },
        Op::Tangent {
            arc: 44,
            end: true,
            forward: true,
            len: 2,
        },
        Op::Tangent {
            arc: 61,
            end: true,
            forward: false,
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
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    12742269905140044419,
);
    let offset = DVec2::new(
    15.420487011535617,
    13.410178116305747,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
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
    13061032330017172868,
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
            angle: 1,
            len: 3,
        },
        Op::Arc {
            from: 47,
            angle: 11,
            radius: 2,
            sweep: 0,
            ccw: false,
        },
        Op::Arc {
            from: 4,
            angle: 2,
            radius: 1,
            sweep: 0,
            ccw: false,
        },
        Op::Arc {
            from: 32,
            angle: 15,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::Step {
            from: 1,
            angle: 20,
            len: 2,
        },
        Op::Step {
            from: 25,
            angle: 3,
            len: 0,
        },
        Op::Mid {
            line: 45,
            angle: 21,
            len: 3,
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
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    10972266449777133058,
);
    let offset = DVec2::new(
    -16.769037748936274,
    -13.64248343666942,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}
