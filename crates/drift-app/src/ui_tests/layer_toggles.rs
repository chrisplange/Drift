//! Alt-click a layer's lock or snap toggle to set all the other layers, and again to put them
//! back, as the eye's Alt-click isolates (drift-releases#10).

use drift_core::LayerId;

use super::*;

pub(super) const LOCK: &str = "Lock or unlock (locked geometry can be snapped to but not edited) · \
                    Alt-click: lock every other layer";
const LOCK_BACK: &str = "Lock or unlock (locked geometry can be snapped to but not edited) · \
                         Alt-click: put the other layers' locks back";
pub(super) const SNAP: &str = "Snap to this layer's geometry (off: it can still be picked and edited) · \
                    Alt-click: snap to this layer only";
const SNAP_BACK: &str = "Snap to this layer's geometry (off: it can still be picked and \
                         edited) · Alt-click: put the other layers' snapping back";

/// The centre of the toggle labelled `tip` on the row of the layer named `name`.
fn toggle(h: &Harness<'_, DriftApp>, name: &str, tip: &str) -> Pos2 {
    let row = h.get_by_label(name).rect();
    h.query_all_by_label(tip)
        .map(|n| n.rect())
        .find(|r| (r.center().y - row.center().y).abs() < 4.0)
        .unwrap_or_else(|| panic!("{tip} on {name}"))
        .center()
}

/// Alt-click the toggle labelled `tip` on the row of the layer named `name`.
pub(super) fn alt_toggle(h: &mut Harness<'_, DriftApp>, name: &str, tip: &str) {
    let at = toggle(h, name, tip);
    alt_click(h, at);
}

fn alt_click(h: &mut Harness<'_, DriftApp>, at: Pos2) {
    h.hover_at(at);
    h.run();
    h.event(Event::ModifiersChanged(EguiModifiers::ALT));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: EguiModifiers::ALT,
        });
        h.run();
    }
    h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    h.run();
}

/// The layers (bottom up) with the setting on.
fn with(h: &Harness<'_, DriftApp>, f: impl Fn(&drift_core::Layer) -> bool) -> Vec<LayerId> {
    h.state()
        .drawing
        .editor
        .doc
        .layers()
        .filter(|(_, l)| f(l))
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn alt_click_on_lock_or_snap_sets_the_other_layers_and_puts_them_back() {
    let Some(mut h) = harness() else { return };
    let ids = many_layers(&mut h, 6);
    let all: Vec<LayerId> = h
        .state()
        .drawing
        .editor
        .doc
        .layers()
        .map(|(id, _)| id)
        .collect();
    // One layer locked and one not snapped beforehand, to come back as they were.
    {
        let ed = &mut h.state_mut().drawing.editor;
        let mut l = ed.doc.layer(ids[1]).cloned().unwrap();
        l.locked = true;
        ed.update_layer(ids[1], l);
        let mut l = ed.doc.layer(ids[4]).cloned().unwrap();
        l.snappable = false;
        ed.update_layer(ids[4], l);
    }
    h.run();
    let locked_before = with(&h, |l| l.locked);
    let snapped_before = with(&h, |l| l.snappable);
    let target = ids[3]; // A-FURN-003
    let last = |h: &Harness<'_, DriftApp>| {
        h.state()
            .drawing
            .editor
            .history
            .undo_label()
            .map(str::to_owned)
    };

    // Alt-click its lock: every other layer locked, this one not, and made current.
    alt_toggle(&mut h, "A-FURN-003", LOCK);
    let others: Vec<LayerId> = all.iter().copied().filter(|l| *l != target).collect();
    assert_eq!(with(&h, |l| l.locked), others);
    assert_eq!(h.state().drawing.editor.doc.current_layer(), target);
    assert_eq!(
        last(&h).as_deref(),
        Some("Lock other layers"),
        "one undo step"
    );
    // The tooltip now says what Alt-click does next.
    let back = toggle(&h, "A-FURN-003", LOCK_BACK);
    save_png("drift_ui_layer_locks_solo.png", &mut h);
    // Alt-click again: the others' locks are as they were.
    alt_click(&mut h, back);
    assert_eq!(with(&h, |l| l.locked), locked_before);
    assert_eq!(last(&h).as_deref(), Some("Put layer locks back"));
    // Undone, the solo is back; undone again, as at first.
    h.state_mut().undo();
    h.run();
    assert_eq!(with(&h, |l| l.locked), others);
    h.state_mut().undo();
    h.run();
    assert_eq!(with(&h, |l| l.locked), locked_before);
    // The toggle offers the solo again (the undo ended it).
    toggle(&h, "A-FURN-003", LOCK);

    // Snap: Alt-click makes this layer the only one snapped to; again puts the rest back.
    alt_toggle(&mut h, "A-WALL-005", SNAP);
    assert_eq!(with(&h, |l| l.snappable), [ids[5]]);
    alt_toggle(&mut h, "A-WALL-005", SNAP_BACK);
    assert_eq!(with(&h, |l| l.snappable), snapped_before);
    // Alt-click on one layer, then another: the second takes over, and putting back
    // restores how they were before the first.
    alt_toggle(&mut h, "A-WALL-005", SNAP);
    alt_toggle(&mut h, "A-DOOR-001", SNAP);
    assert_eq!(with(&h, |l| l.snappable), [ids[1]]);
    alt_toggle(&mut h, "A-DOOR-001", SNAP_BACK);
    assert_eq!(with(&h, |l| l.snappable), snapped_before);
    // A plain click still toggles just its own layer.
    let at = toggle(&h, "A-DOOR-001", SNAP);
    h.hover_at(at);
    h.run();
    button(&h, at, true);
    h.run();
    button(&h, at, false);
    h.run();
    assert!(
        !h.state()
            .drawing
            .editor
            .doc
            .layer(ids[1])
            .unwrap()
            .snappable
    );
    assert_eq!(with(&h, |l| l.snappable).len(), snapped_before.len() - 1);
}
