//! Groups: named sets of entities that are picked, moved and rotated together.

use std::collections::BTreeSet;

use crate::document::EntityId;

/// Stable identifier of a group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GroupId(pub u32);

impl std::fmt::Display for GroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "g{}", self.0)
    }
}

/// A named set of curves and standalone points. An entity belongs to at most one group.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub name: String,
    pub members: BTreeSet<EntityId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DVec2, Document, History};

    #[test]
    fn groups_follow_deletes_merges_and_undo() {
        let mut doc = Document::new();
        let mut history = History::default();
        let a = doc.add_point(DVec2::ZERO);
        let b = doc.add_point(DVec2::new(10.0, 0.0));
        let c = doc.add_point(DVec2::new(10.0, 5.0));
        let l1 = doc.add_line(a, b).unwrap();
        let l2 = doc.add_line(b, c).unwrap();
        doc.begin();
        let g = doc
            .add_group(Group {
                name: "Desk".into(),
                members: [l1, l2].into(),
            })
            .unwrap();
        history.push(doc.commit("Group"));
        assert_eq!(doc.group_of(l2), Some(g));
        // A new group takes members from the old one.
        let g2 = doc
            .add_group(Group {
                name: "Top".into(),
                members: [l1].into(),
            })
            .unwrap();
        assert_eq!(doc.group_of(l1), Some(g2));
        assert_eq!(doc.group(g).unwrap().members.len(), 1);
        // Deleting the last member dissolves the group.
        doc.delete([l2]);
        assert!(doc.group(g).is_none());
        assert!(doc.add_group(Group::default()).is_ok());
        assert!(
            doc.add_group(Group {
                name: "Ghost".into(),
                members: [EntityId(999)].into(),
            })
            .is_err()
        );
        history.undo(&mut doc);
        let mut fresh = Document::new();
        fresh.restore_from(&doc);
        assert_eq!(fresh.groups().count(), doc.groups().count());
    }
}
