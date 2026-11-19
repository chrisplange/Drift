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
            from: 2,
            angle: 13,
            len: 1,
        },
        Op::OnArc {
            arc: 0,
            at: 7,
            angle: 5,
            len: 0,
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
    8907892275937324313,
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
fn probe_drag_is_invariant_under_similarity_12() {
    let plan = Plan {
    ops: vec![
        Op::Arc {
            from: 0,
            angle: 23,
            radius: 0,
            sweep: 4,
            ccw: true,
        },
        Op::Step {
            from: 53,
            angle: 23,
            len: 1,
        },
        Op::Close {
            from: 2,
            to: 36,
        },
        Op::Step {
            from: 5,
            angle: 0,
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
        8,
    ),
    length: None,
};
    let pick = Pick::Curves(
    13422321940316738344,
);
    let offset = DVec2::new(
    13.755129117721863,
    0.0,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        186.98370704747123,
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
            radius: 2,
            sweep: 1,
            ccw: true,
        },
        Op::Close {
            from: 24,
            to: 53,
        },
        Op::Arc {
            from: 33,
            angle: 17,
            radius: 1,
            sweep: 3,
            ccw: false,
        },
        Op::Step {
            from: 0,
            angle: 3,
            len: 2,
        },
        Op::Close {
            from: 9,
            to: 58,
        },
        Op::Dup {
            from: 10,
            angle: 8,
            len: 1,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: true,
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
        56,
    ),
    length: Some(
        52,
    ),
};
    let pick = Pick::Point(
    60,
);
    let offset = DVec2::new(
    7.164923455742015,
    17.546300846956495,
);
    let steps = 1;
    let t = Similarity {
    scale: 3.838692126372308,
    turn: 0.0,
    shift: DVec2::new(
        490.17111694689567,
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
            angle: 7,
            radius: 0,
            sweep: 0,
            ccw: true,
        },
        Op::Close {
            from: 8,
            to: 34,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: true,
            len: 3,
        },
        Op::OnLine {
            line: 0,
            at: 0,
            angle: 6,
            len: 0,
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
    length: Some(
        3,
    ),
};
    let pick = Pick::Curves(
    2948942412519361293,
);
    let offset = DVec2::new(
    -17.838780519226653,
    -15.774533488702177,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -145.35376765632466,
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
            angle: 21,
            radius: 0,
            sweep: 3,
            ccw: false,
        },
        Op::Step {
            from: 24,
            angle: 9,
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
    fix: Some(
        53,
    ),
    length: None,
};
    let pick = Pick::Curves(
    11059704920171063312,
);
    let offset = DVec2::new(
    15.356516545878655,
    -14.111777591849263,
);
    let steps = 1;
    let t = Similarity {
    scale: 2.9877068064591144,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -23.271358444630927,
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
            angle: 0,
            radius: 1,
            sweep: 4,
            ccw: false,
        },
        Op::Step {
            from: 46,
            angle: 18,
            len: 2,
        },
        Op::Step {
            from: 46,
            angle: 18,
            len: 0,
        },
        Op::Arc {
            from: 24,
            angle: 3,
            radius: 1,
            sweep: 4,
            ccw: true,
        },
        Op::OnLine {
            line: 20,
            at: 0,
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
        true,
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
    ],
    fix: Some(
        60,
    ),
    length: None,
};
    let pick = Pick::Curves(
    1384181044010492290,
);
    let offset = DVec2::new(
    -14.668959913533824,
    0.0,
);
    let steps = 1;
    let t = Similarity {
    scale: 2.418885377995435,
    turn: 0.0,
    shift: DVec2::new(
        -398.7733498599033,
        -174.26303150165532,
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
            angle: 13,
            radius: 2,
            sweep: 2,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: true,
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
        11,
    ),
    length: None,
};
    let pick = Pick::Point(
    21,
);
    let offset = DVec2::new(
    -14.356874404974034,
    -10.799395503407696,
);
    let steps = 1;
    let t = Similarity {
    scale: 2.9182379202940933,
    turn: 256.46238183877944,
    shift: DVec2::new(
        29.953977707507867,
        0.0,
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
            angle: 7,
            radius: 2,
            sweep: 2,
            ccw: true,
        },
        Op::Tangent {
            arc: 0,
            end: true,
            forward: true,
            len: 0,
        },
        Op::OnLine {
            line: 0,
            at: 2,
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
        61,
    ),
    length: None,
};
    let pick = Pick::Curves(
    272691448529772080,
);
    let offset = DVec2::new(
    7.956844869137463,
    -11.458805684608038,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        -186.44298537021308,
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
fn probe_drag_is_invariant_under_similarity_9() {
    let plan = Plan {
    ops: vec![
        Op::Dup {
            from: 0,
            angle: 17,
            len: 0,
        },
        Op::Arc {
            from: 2,
            angle: 11,
            radius: 2,
            sweep: 2,
            ccw: false,
        },
        Op::Tangent {
            arc: 0,
            end: false,
            forward: true,
            len: 3,
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
        18,
    ),
    length: None,
};
    let pick = Pick::Curves(
    17906901497116344696,
);
    let offset = DVec2::new(
    4.264939958895797,
    19.793546931109862,
);
    let steps = 1;
    let t = Similarity {
    scale: 0.25,
    turn: 0.0,
    shift: DVec2::new(
        0.0,
        162.03989851526214,
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
            angle: 14,
            len: 2,
        },
        Op::Step {
            from: 19,
            angle: 23,
            len: 3,
        },
        Op::Step {
            from: 2,
            angle: 11,
            len: 2,
        },
        Op::Dup {
            from: 58,
            angle: 7,
            len: 0,
        },
        Op::Arc {
            from: 14,
            angle: 17,
            radius: 0,
            sweep: 0,
            ccw: true,
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
    fix: None,
    length: None,
};
    let pick = Pick::Curves(
    16394186515792174284,
);
    let offset = DVec2::new(
    0.6931347213607743,
    -19.189392075459143,
);
    let steps = 1;
    explained(moves_land(&plan, &pick, offset, steps)).unwrap();
}
