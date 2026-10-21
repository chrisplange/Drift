//! Paths: curves drawn as one shape, in order round it. A closed path is a shape of its
//! own, as a drawing program's object is: the fill inside it is the shape's, found from
//! its curves alone, whatever else crosses or lies inside it.

use crate::document::EntityId;

/// Stable identifier of a path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathId(pub u32);

impl std::fmt::Display for PathId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "p{}", self.0)
    }
}

/// Curves drawn as one shape, in order round it. A curve belongs to at most one path.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub curves: Vec<EntityId>,
    /// The last curve's end meets the first's start (a lone circle or whole ellipse is
    /// closed on its own). Open: a side was deleted or trimmed away, or the path was never
    /// closed.
    pub closed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DVec2, Document, Fill, FillKind, History};

    #[test]
    fn paths_follow_deletes_merges_and_undo() {
        let mut doc = Document::new();
        let mut history = History::default();
        let p = [(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0)]
            .map(|(x, y)| doc.add_point(DVec2::new(x, y)));
        let l: Vec<EntityId> = (0..4)
            .map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
            .collect();
        doc.begin();
        let id = doc
            .add_path(Path {
                curves: l.clone(),
                closed: true,
            })
            .unwrap();
        let mut fill = Fill::new(l.clone(), DVec2::new(5.0, 2.5), FillKind::Solid);
        fill.path = Some(id);
        let f = doc.add_fill(fill);
        history.push(doc.commit("Shape"));
        assert_eq!(doc.path_of(l[2]), Some(id));
        // A curve is in one path: a new path takes it.
        doc.begin();
        let other = doc
            .add_path(Path {
                curves: vec![l[0]],
                closed: false,
            })
            .unwrap();
        assert_eq!(doc.path(id).unwrap().curves, l[1..]);
        history.push(doc.commit("Path"));
        history.undo(&mut doc);
        assert_eq!(doc.path(id).unwrap().curves, l);
        assert!(doc.path(other).is_none());
        // Deleting a side opens it; deleting the rest removes it and unbinds its fill.
        doc.begin();
        doc.delete([l[1]]);
        let path = doc.path(id).unwrap();
        assert!(!path.closed);
        assert_eq!(path.curves, vec![l[0], l[2], l[3]]);
        doc.delete([l[0], l[2], l[3]]);
        assert!(doc.path(id).is_none());
        // (The fill went with its outline.)
        assert!(doc.fill(f).is_none());
        history.push(doc.commit("Delete"));
        history.undo(&mut doc);
        assert!(doc.path(id).unwrap().closed);
        assert_eq!(doc.fill(f).unwrap().path, Some(id));
        assert!(
            doc.add_path(Path {
                curves: vec![EntityId(999)],
                closed: true,
            })
            .is_err()
        );
        let mut fresh = Document::new();
        fresh.restore_from(&doc);
        assert_eq!(fresh.paths().count(), 1);
    }
}
