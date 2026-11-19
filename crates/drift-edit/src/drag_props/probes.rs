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
            from: 13,
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
    8677859407171547149,
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
        Op::Step {
            from: 0,
            angle: 9,
            len: 1,
        },
        Op::Arc {
            from: 2,
            angle: 2,
            radius: 0,
            sweep: 3,
            ccw: false,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: false,
            len: 2,
        },
        Op::Step {
            from: 13,
            angle: 8,
            len: 3,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: true,
            len: 1,
        },
        Op::Dup {
            from: 63,
            angle: 15,
            len: 3,
        },
        Op::Arc {
            from: 31,
            angle: 23,
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
        27,
    ),
    length: None,
};
    let pick = Pick::Curves(
    3375641517027564988,
);
    let offset = DVec2::new(
    8.207238067607742,
    15.038628519987087,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        161.47756374440883,
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
            angle: 11,
            len: 2,
        },
        Op::OnLine {
            line: 0,
            at: 0,
            angle: 23,
            len: 2,
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
    fix: Some(
        20,
    ),
    length: None,
};
    let pick = Pick::Curves(
    3699881228236730230,
);
    let offset = DVec2::new(
    -2.9622853593258753,
    5.770012386952676,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        152.05423999816963,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_14() {
    let plan = Plan {
    ops: vec![
        Op::Dup {
            from: 0,
            angle: 10,
            len: 1,
        },
        Op::Step {
            from: 20,
            angle: 23,
            len: 1,
        },
        Op::Mid {
            line: 19,
            angle: 0,
            len: 2,
        },
        Op::OnLine {
            line: 3,
            at: 1,
            angle: 23,
            len: 2,
        },
        Op::Arc {
            from: 10,
            angle: 0,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::Arc {
            from: 54,
            angle: 1,
            radius: 1,
            sweep: 0,
            ccw: false,
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
        14,
    ),
    length: None,
};
    let pick = Pick::Curves(
    15539866604508292607,
);
    let offset = DVec2::new(
    0.0,
    10.194471850182088,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 243.41777157809884,
    shift: DVec2::new(
        0.0,
        160.07452242284506,
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
            angle: 3,
            radius: 0,
            sweep: 1,
            ccw: false,
        },
        Op::Arc {
            from: 10,
            angle: 0,
            radius: 0,
            sweep: 2,
            ccw: false,
        },
        Op::Step {
            from: 35,
            angle: 7,
            len: 2,
        },
        Op::OnLine {
            line: 0,
            at: 2,
            angle: 0,
            len: 0,
        },
        Op::OnLine {
            line: 30,
            at: 0,
            angle: 18,
            len: 0,
        },
        Op::Step {
            from: 12,
            angle: 0,
            len: 2,
        },
        Op::Step {
            from: 1,
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
        37,
    ),
    length: None,
};
    let pick = Pick::Curves(
    6584590853270152696,
);
    let offset = DVec2::new(
    -13.878601759235739,
    -13.215881845791053,
);
    let steps = 5;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -214.9554112350735,
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
            angle: 4,
            radius: 1,
            sweep: 1,
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
    fix: Some(
        3,
    ),
    length: None,
};
    let pick = Pick::Curves(
    0,
);
    let offset = DVec2::new(
    14.732052705124934,
    -16.440665425120365,
);
    let steps = 1;
    let t = Similarity {
    scale: 2.2564949312217255,
    turn: 97.95519877876312,
    shift: DVec2::new(
        0.0,
        298.61780750620625,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_17() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 0,
            len: 0,
        },
        Op::Step {
            from: 0,
            angle: 0,
            len: 0,
        },
        Op::Mid {
            line: 0,
            angle: 5,
            len: 0,
        },
        Op::Mid {
            line: 0,
            angle: 6,
            len: 0,
        },
    ],
    extras: vec![
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
    ],
    fix: Some(
        3,
    ),
    length: None,
};
    let pick = Pick::Curves(
    12713357868288821239,
);
    let offset = DVec2::new(
    0.0,
    -8.014286672715018,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 261.78434507519836,
    shift: DVec2::new(
        0.0,
        0.0,
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
            radius: 1,
            sweep: 3,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: false,
            len: 1,
        },
        Op::Mid {
            line: 0,
            angle: 2,
            len: 1,
        },
        Op::Arc {
            from: 0,
            angle: 0,
            radius: 0,
            sweep: 2,
            ccw: false,
        },
        Op::Step {
            from: 59,
            angle: 6,
            len: 1,
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
    fix: Some(
        11,
    ),
    length: None,
};
    let pick = Pick::Curves(
    5537616607932059057,
);
    let offset = DVec2::new(
    0.0,
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
            sweep: 2,
            ccw: false,
        },
        Op::Arc {
            from: 0,
            angle: 16,
            radius: 1,
            sweep: 0,
            ccw: false,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: false,
            len: 0,
        },
        Op::Tangent {
            arc: 29,
            end: true,
            forward: false,
            len: 0,
        },
        Op::Step {
            from: 0,
            angle: 1,
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
    11834255120470739687,
);
    let offset = DVec2::new(
    -6.095211569675232,
    -16.848940102060517,
);
    let steps = 1;
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
            angle: 9,
            radius: 0,
            sweep: 2,
            ccw: false,
        },
        Op::Arc {
            from: 3,
            angle: 13,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::Step {
            from: 15,
            angle: 2,
            len: 0,
        },
        Op::Step {
            from: 61,
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
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
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
        10,
    ),
    length: None,
};
    let pick = Pick::Curves(
    2268841574146122888,
);
    let offset = DVec2::new(
    0.0,
    -19.870537599480965,
);
    let steps = 8;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -124.96573687083355,
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
            angle: 8,
            radius: 0,
            sweep: 2,
            ccw: false,
        },
        Op::Arc {
            from: 2,
            angle: 17,
            radius: 0,
            sweep: 3,
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
        4,
    ),
    length: None,
};
    let pick = Pick::Curves(
    5002248497496505048,
);
    let offset = DVec2::new(
    6.294184107805657,
    0.0,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -483.7583213425879,
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
            angle: 18,
            len: 1,
        },
        Op::Mid {
            line: 0,
            angle: 18,
            len: 2,
        },
        Op::Arc {
            from: 4,
            angle: 0,
            radius: 0,
            sweep: 0,
            ccw: false,
        },
        Op::Step {
            from: 32,
            angle: 7,
            len: 2,
        },
        Op::Mid {
            line: 0,
            angle: 0,
            len: 0,
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
    fix: Some(
        21,
    ),
    length: None,
};
    let pick = Pick::Curves(
    4177348714630050836,
);
    let offset = DVec2::new(
    0.0,
    6.313990083804955,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        451.52345741398483,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_5() {
    let plan = Plan {
    ops: vec![
        Op::Dup {
            from: 0,
            angle: 14,
            len: 1,
        },
        Op::Arc {
            from: 0,
            angle: 21,
            radius: 2,
            sweep: 4,
            ccw: true,
        },
        Op::Arc {
            from: 0,
            angle: 14,
            radius: 2,
            sweep: 0,
            ccw: false,
        },
        Op::Close {
            from: 0,
            to: 12,
        },
        Op::Step {
            from: 44,
            angle: 3,
            len: 1,
        },
        Op::Step {
            from: 46,
            angle: 14,
            len: 3,
        },
        Op::OnArc {
            arc: 13,
            at: 2,
            angle: 11,
            len: 2,
        },
    ],
    extras: vec![
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
        false,
        false,
        false,
    ],
    fix: Some(
        14,
    ),
    length: None,
};
    let pick = Pick::Curves(
    8765804352647590655,
);
    let offset = DVec2::new(
    0.0,
    11.148095283031372,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 294.9309333646824,
    shift: DVec2::new(
        -245.90626691096818,
        0.0,
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
            angle: 1,
            radius: 0,
            sweep: 1,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: false,
            len: 3,
        },
        Op::Step {
            from: 27,
            angle: 2,
            len: 1,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: false,
            len: 0,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: false,
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
    fix: Some(
        38,
    ),
    length: None,
};
    let pick = Pick::Curves(
    5683883935645075800,
);
    let offset = DVec2::new(
    -7.496418308131739,
    0.0,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        22.17076101701601,
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
            angle: 8,
            radius: 0,
            sweep: 3,
            ccw: false,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: true,
            len: 2,
        },
        Op::Step {
            from: 28,
            angle: 8,
            len: 0,
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
    fix: Some(
        37,
    ),
    length: None,
};
    let pick = Pick::Curves(
    73571971278899415,
);
    let offset = DVec2::new(
    0.0,
    11.044542042766906,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        199.63755150799628,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_drag_is_invariant_under_similarity_8() {
    let plan = Plan {
    ops: vec![
        Op::Step {
            from: 0,
            angle: 14,
            len: 0,
        },
        Op::Step {
            from: 1,
            angle: 5,
            len: 2,
        },
        Op::Arc {
            from: 28,
            angle: 0,
            radius: 2,
            sweep: 3,
            ccw: true,
        },
        Op::OnArc {
            arc: 0,
            at: 4,
            angle: 5,
            len: 1,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: false,
            len: 0,
        },
        Op::Dup {
            from: 42,
            angle: 11,
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
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
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
        14,
    ),
    length: None,
};
    let pick = Pick::Curves(
    9578123040320514520,
);
    let offset = DVec2::new(
    10.177377648502794,
    -16.171601341860427,
);
    let steps = 4;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        57.80230280595827,
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
            angle: 7,
            radius: 0,
            sweep: 0,
            ccw: true,
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
        2,
    ),
    length: None,
};
    let pick = Pick::Curves(
    0,
);
    let offset = DVec2::new(
    4.720099495091058,
    19.557101462307553,
);
    let steps = 13;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -359.0225462604133,
    ),
};
    explained(invariant(&plan, &pick, offset, steps, t)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_free_move_is_accepted_8() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 16,
            radius: 0,
            sweep: 0,
            ccw: true,
        },
        Op::Dup {
            from: 3,
            angle: 13,
            len: 0,
        },
        Op::OnArc {
            arc: 0,
            at: 5,
            angle: 14,
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
    3698276435161877810,
);
    let offset = DVec2::new(
    -6.6689187292184515,
    -5.239314890682691,
);
    let steps = 1;
    explained(free_moves_go(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_keeps_untouched_lengths_1() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 8,
            radius: 1,
            sweep: 0,
            ccw: false,
        },
        Op::Step {
            from: 53,
            angle: 1,
            len: 0,
        },
        Op::Arc {
            from: 12,
            angle: 9,
            radius: 0,
            sweep: 3,
            ccw: false,
        },
        Op::Step {
            from: 5,
            angle: 2,
            len: 1,
        },
        Op::Arc {
            from: 8,
            angle: 0,
            radius: 0,
            sweep: 0,
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
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    15397916954471840088,
);
    let offset = DVec2::new(
    -11.338052095051577,
    0.0,
);
    let steps = 1;
    explained(no_unforced_stretch(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_keeps_untouched_lengths_20() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 14,
            radius: 1,
            sweep: 0,
            ccw: false,
        },
        Op::Arc {
            from: 20,
            angle: 16,
            radius: 0,
            sweep: 0,
            ccw: true,
        },
        Op::OnArc {
            arc: 0,
            at: 0,
            angle: 0,
            len: 0,
        },
        Op::Tangent {
            arc: 6,
            end: true,
            forward: true,
            len: 0,
        },
        Op::Mid {
            line: 0,
            angle: 0,
            len: 0,
        },
        Op::Step {
            from: 53,
            angle: 0,
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
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    13404712171624543208,
);
    let offset = DVec2::new(
    -15.878692424436053,
    0.0,
);
    let steps = 1;
    explained(no_unforced_stretch(&plan, &pick, offset, steps)).unwrap();
}

#[test]
#[ignore = "probe"]
fn probe_move_lands_exactly_8() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 16,
            radius: 0,
            sweep: 0,
            ccw: true,
        },
        Op::Dup {
            from: 3,
            angle: 13,
            len: 0,
        },
        Op::OnArc {
            arc: 0,
            at: 5,
            angle: 14,
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
    3698276435161877810,
);
    let offset = DVec2::new(
    -6.6689187292184515,
    -5.239314890682691,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}
