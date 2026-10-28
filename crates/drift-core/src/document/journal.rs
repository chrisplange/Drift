//! The change journal: what an open transaction touched, undo/redo of committed
//! transactions, and the history that holds them.

use super::*;

/// Something the journal tracks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Key {
    Entity(EntityId),
    Constraint(ConstraintId),
    Placement(ConstraintId),
    ExtensionStarts(ConstraintId),
    /// Present (with `Record::Driven`) when the dimension is driven.
    Driven(ConstraintId),
    Variable(VariableId),
    /// The expression a dimension's value is linked to.
    Expression(ConstraintId),
    Variant(VariantId),
    Measurement(MeasurementId),
    Units,
    Layer(LayerId),
    CurrentLayer,
    Fill(FillId),
    Annotation(AnnotationId),
    DimNote(ConstraintId),
    Shown(crate::AnnotationRef),
    OnlyIn(crate::AnnotationRef),
    Style(crate::AnnotationRef),
    Draft,
    Patterns,
    Logo,
    Rules,
    Project,
    Fonts,
    Sheet(SheetId),
    View(ViewId),
    Group(GroupId),
    Path(PathId),
    Block(BlockId),
    Instance(InstanceId),
    Underlay(UnderlayId),
    Text(TextId),
    Wall(WallId),
    PropertyDim(PropertyDimId),
    Opening(OpeningId),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Record {
    Entity(Entity),
    Constraint(Constraint),
    Placement(DVec2),
    ExtensionStarts([Option<f64>; 2]),
    Driven,
    Variable(Variable),
    Expression(String),
    Variant(Variant),
    Measurement(Constraint),
    Units(Units),
    Layer(Layer),
    CurrentLayer(LayerId),
    Fill(Fill),
    Annotation(Annotation),
    DimNote(DimNote),
    Shown(crate::Shown),
    OnlyIn(BTreeSet<ViewId>),
    Style(crate::StyleOverride),
    Draft(DraftStyle),
    Patterns(Shared<String>),
    Logo(Shared<String>),
    Rules(crate::DrawingRules),
    Project(crate::Project),
    Fonts(Shared<crate::fonts::FontSet>),
    Sheet(Sheet),
    View(View),
    Group(Group),
    Path(Path),
    Block(Block),
    Instance(Instance),
    Underlay(Underlay),
    Text(SketchText),
    Wall(Wall),
    PropertyDim(PropertyDim),
    Opening(Opening),
}

/// A constraint left out when opening a document, and why.
pub type Dropped = (ConstraintId, &'static str);

/// One undoable step: the before and after state of everything it touched.
#[derive(Clone, Debug, PartialEq)]
pub struct Transaction {
    pub label: String,
    changes: Vec<(Key, Option<Record>, Option<Record>)>,
}

impl Transaction {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// The entities it added or changed (not those it removed).
    pub fn entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.changes
            .iter()
            .filter_map(|(key, _, after)| match (key, after) {
                (Key::Entity(id), Some(_)) => Some(*id),
                _ => None,
            })
    }
}

impl Document {
    /// Start recording changes. Nested calls are ignored; the outer transaction wins.
    pub fn begin(&mut self) {
        if self.journal.is_none() {
            self.journal = Some(HashMap::new());
            self.transaction_serial += 1;
            self.begun_at = self.revision;
        }
    }

    /// The revision when the open (or last) transaction began: what a cache made then was
    /// made of, before any of the transaction's changes.
    pub fn revision_at_begin(&self) -> u64 {
        self.begun_at
    }

    /// Which transaction is open (or was last): a new number for each [`begin`](Self::begin)
    /// that opens one.
    pub fn transaction_serial(&self) -> u64 {
        self.transaction_serial
    }

    pub fn in_transaction(&self) -> bool {
        self.journal.is_some()
    }

    /// True if curve `id` was deleted, or given other points or another kind (split, trimmed,
    /// extended), in the open transaction.
    pub fn reshaped_in_transaction(&self, id: EntityId) -> bool {
        let Some(journal) = &self.journal else {
            return false;
        };
        match journal.get(&Key::Entity(id)) {
            Some(Some(Record::Entity(before))) => self
                .entities
                .get(&id)
                .is_none_or(|now| now.kind != before.kind),
            _ => false,
        }
    }

    /// True if the open transaction touched entity `id` (moved, reshaped, restyled, added or
    /// deleted it).
    pub fn touched_in_transaction(&self, id: EntityId) -> bool {
        self.journal
            .as_ref()
            .is_some_and(|j| j.contains_key(&Key::Entity(id)))
    }

    /// The entities the open transaction touched (sorted).
    pub fn entities_touched_in_transaction(&self) -> Vec<EntityId> {
        let Some(journal) = &self.journal else {
            return Vec::new();
        };
        let mut ids: Vec<EntityId> = journal
            .keys()
            .filter_map(|k| match k {
                Key::Entity(id) => Some(*id),
                _ => None,
            })
            .collect();
        ids.sort_unstable();
        ids
    }

    /// The walls the open transaction added, changed or removed, and the paths it touched
    /// (sorted).
    pub fn walls_and_paths_touched_in_transaction(&self) -> (Vec<WallId>, Vec<PathId>) {
        let Some(journal) = &self.journal else {
            return (Vec::new(), Vec::new());
        };
        let (mut walls, mut paths) = (Vec::new(), Vec::new());
        for k in journal.keys() {
            match k {
                Key::Wall(id) => walls.push(*id),
                Key::Path(id) => paths.push(*id),
                _ => {}
            }
        }
        walls.sort_unstable();
        paths.sort_unstable();
        (walls, paths)
    }

    /// The openings the open transaction added, changed or removed (sorted).
    pub fn openings_touched_in_transaction(&self) -> Vec<OpeningId> {
        let Some(journal) = &self.journal else {
            return Vec::new();
        };
        let mut out: Vec<OpeningId> = journal
            .keys()
            .filter_map(|k| match k {
                Key::Opening(id) => Some(*id),
                _ => None,
            })
            .collect();
        out.sort_unstable();
        out
    }

    /// Opening `id` as it was when the open transaction began.
    pub fn opening_before_transaction(&self, id: OpeningId) -> Option<Opening> {
        match self.journal.as_ref().and_then(|j| j.get(&Key::Opening(id))) {
            Some(Some(Record::Opening(o))) => Some(o.clone()),
            Some(_) => None,
            None => self.openings.get(&id).cloned(),
        }
    }

    /// Wall `id` as it was when the open transaction began (see
    /// [`entity_before_transaction`](Self::entity_before_transaction)).
    pub fn wall_before_transaction(&self, id: WallId) -> Option<Wall> {
        match self.journal.as_ref().and_then(|j| j.get(&Key::Wall(id))) {
            Some(Some(Record::Wall(w))) => Some(w.clone()),
            Some(_) => None,
            None => self.walls.get(&id).cloned(),
        }
    }

    /// Path `id` as it was when the open transaction began.
    pub fn path_before_transaction(&self, id: PathId) -> Option<Path> {
        match self.journal.as_ref().and_then(|j| j.get(&Key::Path(id))) {
            Some(Some(Record::Path(p))) => Some(p.clone()),
            Some(_) => None,
            None => self.paths.get(&id).cloned(),
        }
    }

    /// True if path `id` was added in the open transaction.
    pub fn path_created_in_transaction(&self, id: PathId) -> bool {
        self.journal
            .as_ref()
            .is_some_and(|j| matches!(j.get(&Key::Path(id)), Some(None)))
    }

    /// True if fill `id` was added in the open transaction.
    pub fn fill_created_in_transaction(&self, id: FillId) -> bool {
        self.journal
            .as_ref()
            .is_some_and(|j| matches!(j.get(&Key::Fill(id)), Some(None)))
    }

    /// Entity `id` as it was when the open transaction began (as it is now outside one, or
    /// if the transaction hasn't touched it). None if it didn't exist then.
    pub fn entity_before_transaction(&self, id: EntityId) -> Option<Entity> {
        match self.journal.as_ref().and_then(|j| j.get(&Key::Entity(id))) {
            Some(Some(Record::Entity(e))) => Some(*e),
            Some(_) => None,
            None => self.entities.get(&id).copied(),
        }
    }

    /// Fill `id` as it was when the open transaction began (see
    /// [`entity_before_transaction`](Self::entity_before_transaction)).
    pub fn fill_before_transaction(&self, id: FillId) -> Option<Fill> {
        match self.journal.as_ref().and_then(|j| j.get(&Key::Fill(id))) {
            Some(Some(Record::Fill(f))) => Some(f.clone()),
            Some(_) => None,
            None => self.fills.get(&id).cloned(),
        }
    }

    /// Entities created in the open transaction (that still exist).
    pub fn created_in_transaction(&self) -> Vec<EntityId> {
        let Some(journal) = &self.journal else {
            return Vec::new();
        };
        let mut ids: Vec<EntityId> = journal
            .iter()
            .filter_map(|(key, before)| match (key, before) {
                (Key::Entity(id), None) if self.entities.contains_key(id) => Some(*id),
                _ => None,
            })
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Finish recording and return the changes made since [`begin`](Self::begin).
    pub fn commit(&mut self, label: impl Into<String>) -> Transaction {
        let journal = self.journal.take().unwrap_or_default();
        let mut changes: Vec<_> = journal
            .into_iter()
            .filter_map(|(key, before)| {
                let after = self.record(key);
                (before != after).then_some((key, before, after))
            })
            .collect();
        changes.sort_by_key(|(key, ..)| *key);
        Transaction {
            label: label.into(),
            changes,
        }
    }

    /// Abandon the open transaction, restoring every touched entity.
    pub fn rollback(&mut self) {
        if let Some(journal) = self.journal.take() {
            for (key, before) in journal {
                self.restore(key, before);
            }
            self.revision += 1;
            self.structure = next_structure();
        }
    }

    pub(super) fn record(&self, key: Key) -> Option<Record> {
        match key {
            Key::Entity(id) => self.entities.get(&id).copied().map(Record::Entity),
            Key::Constraint(id) => self.constraints.get(&id).copied().map(Record::Constraint),
            Key::Placement(id) => self.placements.get(&id).copied().map(Record::Placement),
            Key::ExtensionStarts(id) => self
                .extension_starts
                .get(&id)
                .copied()
                .map(Record::ExtensionStarts),
            Key::Driven(id) => self.driven.contains(&id).then_some(Record::Driven),
            Key::Variable(id) => self.variables.get(&id).cloned().map(Record::Variable),
            Key::Expression(id) => self.expressions.get(&id).cloned().map(Record::Expression),
            Key::Variant(id) => self.variants.get(&id).cloned().map(Record::Variant),
            Key::Measurement(id) => self.measurements.get(&id).copied().map(Record::Measurement),
            Key::Units => Some(Record::Units(self.units)),
            Key::Layer(id) => self.layers.get(&id).cloned().map(Record::Layer),
            Key::CurrentLayer => Some(Record::CurrentLayer(self.current_layer)),
            Key::Fill(id) => self.fills.get(&id).cloned().map(Record::Fill),
            Key::Annotation(id) => self.annotations.get(&id).cloned().map(Record::Annotation),
            Key::DimNote(id) => self.dim_notes.get(&id).cloned().map(Record::DimNote),
            Key::Shown(r) => self.shown.get(&r).copied().map(Record::Shown),
            Key::OnlyIn(r) => self.only_in.get(&r).cloned().map(Record::OnlyIn),
            Key::Style(r) => self.styles.get(&r).copied().map(Record::Style),
            Key::Draft => Some(Record::Draft(self.draft)),
            Key::Patterns => Some(Record::Patterns(self.patterns.clone())),
            Key::Logo => Some(Record::Logo(self.logo.clone())),
            Key::Rules => Some(Record::Rules(self.rules)),
            Key::Project => Some(Record::Project(self.project.clone())),
            Key::Fonts => Some(Record::Fonts(self.fonts.clone())),
            Key::Sheet(id) => self.sheets.get(&id).cloned().map(Record::Sheet),
            Key::View(id) => self.views.get(&id).cloned().map(Record::View),
            Key::Group(id) => self.groups.get(&id).cloned().map(Record::Group),
            Key::Path(id) => self.paths.get(&id).cloned().map(Record::Path),
            Key::Block(id) => self.blocks.get(&id).cloned().map(Record::Block),
            Key::Instance(id) => self.instances.get(&id).copied().map(Record::Instance),
            Key::Underlay(id) => self.underlays.get(&id).cloned().map(Record::Underlay),
            Key::Text(id) => self.texts.get(&id).cloned().map(Record::Text),
            Key::Wall(id) => self.walls.get(&id).cloned().map(Record::Wall),
            Key::PropertyDim(id) => self
                .property_dims
                .get(&id)
                .copied()
                .map(Record::PropertyDim),
            Key::Opening(id) => self.openings.get(&id).cloned().map(Record::Opening),
        }
    }

    pub(super) fn touch_key(&mut self, key: Key) {
        if !matches!(key, Key::Placement(_)) {
            self.structure = next_structure();
        }
        self.record_touch(key);
    }

    /// Note a change to `key` for the open transaction without changing the structure.
    pub(super) fn record_touch(&mut self, key: Key) {
        self.note_change(key);
        if self.journal.is_some() {
            let current = self.record(key);
            if let Some(journal) = &mut self.journal {
                journal.entry(key).or_insert(current);
            }
        }
        self.revision += 1;
    }

    pub(super) fn touch(&mut self, id: EntityId) {
        self.touch_key(Key::Entity(id));
    }

    /// [`touch`](Self::touch) for geometry that only moves: same entity, same points.
    pub(super) fn touch_moved(&mut self, id: EntityId) {
        self.record_touch(Key::Entity(id));
    }

    pub(super) fn touch_constraint(&mut self, id: ConstraintId) {
        self.touch_key(Key::Constraint(id));
    }

    /// Note a change to `key` for caches that follow entities and layers.
    fn note_change(&mut self, key: Key) {
        match key {
            Key::Entity(id) => {
                self.geometry = next_structure();
                self.changes.note(self.revision, Some(id));
            }
            Key::Layer(_) => self.changes.note(self.revision, None),
            _ => {}
        }
    }

    pub(super) fn restore(&mut self, key: Key, record: Option<Record>) {
        self.note_change(key);
        match (key, record) {
            (Key::Entity(id), Some(Record::Entity(e))) => {
                self.entities.insert(id, e);
            }
            (Key::Entity(id), _) => {
                self.entities.remove(&id);
            }
            (Key::Constraint(id), Some(Record::Constraint(c))) => {
                self.constraints.insert(id, c);
            }
            (Key::Constraint(id), _) => {
                self.constraints.remove(&id);
            }
            (Key::Placement(id), Some(Record::Placement(p))) => {
                self.placements.insert(id, p);
            }
            (Key::Placement(id), _) => {
                self.placements.remove(&id);
            }
            (Key::ExtensionStarts(id), Some(Record::ExtensionStarts(s))) => {
                self.extension_starts.insert(id, s);
            }
            (Key::ExtensionStarts(id), _) => {
                self.extension_starts.remove(&id);
            }
            (Key::Driven(id), Some(_)) => {
                self.driven.insert(id);
            }
            (Key::Driven(id), None) => {
                self.driven.remove(&id);
            }
            (Key::Variable(id), Some(Record::Variable(v))) => {
                self.variables.insert(id, v);
            }
            (Key::Variable(id), _) => {
                self.variables.remove(&id);
            }
            (Key::Expression(id), Some(Record::Expression(e))) => {
                self.expressions.insert(id, e);
            }
            (Key::Expression(id), _) => {
                self.expressions.remove(&id);
            }
            (Key::Variant(id), Some(Record::Variant(v))) => {
                self.variants.insert(id, v);
            }
            (Key::Variant(id), _) => {
                self.variants.remove(&id);
            }
            (Key::Measurement(id), Some(Record::Measurement(m))) => {
                self.measurements.insert(id, m);
            }
            (Key::Measurement(id), _) => {
                self.measurements.remove(&id);
            }
            (Key::Units, Some(Record::Units(u))) => self.units = u,
            (Key::Units, _) => {}
            (Key::Layer(id), Some(Record::Layer(l))) => {
                self.layers.insert(id, l);
            }
            (Key::Layer(id), _) => {
                self.layers.remove(&id);
            }
            (Key::CurrentLayer, Some(Record::CurrentLayer(l))) => self.current_layer = l,
            (Key::CurrentLayer, _) => {}
            (Key::Fill(id), Some(Record::Fill(f))) => {
                self.fills.insert(id, f);
            }
            (Key::Fill(id), _) => {
                self.fills.remove(&id);
            }
            (Key::Annotation(id), Some(Record::Annotation(a))) => {
                self.annotations.insert(id, a);
            }
            (Key::Annotation(id), _) => {
                self.annotations.remove(&id);
            }
            (Key::DimNote(id), Some(Record::DimNote(n))) => {
                self.dim_notes.insert(id, n);
            }
            (Key::DimNote(id), _) => {
                self.dim_notes.remove(&id);
            }
            (Key::Shown(r), Some(Record::Shown(s))) => {
                self.shown.insert(r, s);
            }
            (Key::Shown(r), _) => {
                self.shown.remove(&r);
            }
            (Key::OnlyIn(r), Some(Record::OnlyIn(v))) => {
                self.only_in.insert(r, v);
            }
            (Key::OnlyIn(r), _) => {
                self.only_in.remove(&r);
            }
            (Key::Style(r), Some(Record::Style(s))) => {
                self.styles.insert(r, s);
            }
            (Key::Style(r), _) => {
                self.styles.remove(&r);
            }
            (Key::Draft, Some(Record::Draft(d))) => self.draft = d,
            (Key::Draft, _) => {}
            (Key::Patterns, Some(Record::Patterns(p))) => self.patterns = p,
            (Key::Patterns, _) => {}
            (Key::Logo, Some(Record::Logo(l))) => self.logo = l,
            (Key::Logo, _) => {}
            (Key::Rules, Some(Record::Rules(r))) => self.rules = r,
            (Key::Rules, _) => {}
            (Key::Project, Some(Record::Project(p))) => self.project = p,
            (Key::Project, _) => {}
            (Key::Fonts, Some(Record::Fonts(f))) => self.fonts = f,
            (Key::Fonts, _) => {}
            (Key::Text(id), Some(Record::Text(t))) => {
                self.texts.insert(id, t);
            }
            (Key::Text(id), _) => {
                self.texts.remove(&id);
            }
            (Key::Underlay(id), Some(Record::Underlay(u))) => {
                self.underlays.insert(id, u);
            }
            (Key::Underlay(id), _) => {
                self.underlays.remove(&id);
            }
            (Key::Sheet(id), Some(Record::Sheet(s))) => {
                self.sheets.insert(id, s);
            }
            (Key::Sheet(id), _) => {
                self.sheets.remove(&id);
            }
            (Key::View(id), Some(Record::View(v))) => {
                self.views.insert(id, v);
            }
            (Key::View(id), _) => {
                self.views.remove(&id);
            }
            (Key::Group(id), Some(Record::Group(g))) => {
                self.groups.insert(id, g);
            }
            (Key::Group(id), _) => {
                self.groups.remove(&id);
            }
            (Key::Path(id), Some(Record::Path(p))) => {
                self.paths.insert(id, p);
            }
            (Key::Path(id), _) => {
                self.paths.remove(&id);
            }
            (Key::Block(id), Some(Record::Block(b))) => {
                self.blocks.insert(id, b);
            }
            (Key::Block(id), _) => {
                self.blocks.remove(&id);
            }
            (Key::Instance(id), Some(Record::Instance(i))) => {
                self.instances.insert(id, i);
            }
            (Key::Instance(id), _) => {
                self.instances.remove(&id);
            }
            (Key::Wall(id), Some(Record::Wall(w))) => {
                self.walls.insert(id, w);
            }
            (Key::Wall(id), _) => {
                self.walls.remove(&id);
            }
            (Key::PropertyDim(id), Some(Record::PropertyDim(d))) => {
                self.property_dims.insert(id, d);
            }
            (Key::PropertyDim(id), _) => {
                self.property_dims.remove(&id);
            }
            (Key::Opening(id), Some(Record::Opening(o))) => {
                self.openings.insert(id, o);
            }
            (Key::Opening(id), _) => {
                self.openings.remove(&id);
            }
        }
    }

    pub(super) fn apply(&mut self, tx: &Transaction, forward: bool) {
        for (key, before, after) in &tx.changes {
            self.restore(
                *key,
                if forward {
                    after.clone()
                } else {
                    before.clone()
                },
            );
        }
        self.revision += 1;
        self.structure = next_structure();
    }

    // ----- Edits -----------------------------------------------------------------------------

    /// Every journal key with content in this document.
    pub(super) fn keys(&self) -> Vec<Key> {
        let mut keys: Vec<Key> = Vec::new();
        keys.extend(self.entities.keys().map(|id| Key::Entity(*id)));
        keys.extend(self.constraints.keys().map(|id| Key::Constraint(*id)));
        keys.extend(self.placements.keys().map(|id| Key::Placement(*id)));
        keys.extend(
            self.extension_starts
                .keys()
                .map(|id| Key::ExtensionStarts(*id)),
        );
        keys.extend(self.driven.iter().map(|id| Key::Driven(*id)));
        keys.extend(self.variables.keys().map(|id| Key::Variable(*id)));
        keys.extend(self.expressions.keys().map(|id| Key::Expression(*id)));
        keys.extend(self.variants.keys().map(|id| Key::Variant(*id)));
        keys.extend(self.measurements.keys().map(|id| Key::Measurement(*id)));
        keys.push(Key::Units);
        keys.extend(self.layers.keys().map(|id| Key::Layer(*id)));
        keys.push(Key::CurrentLayer);
        keys.extend(self.fills.keys().map(|id| Key::Fill(*id)));
        keys.extend(self.annotations.keys().map(|id| Key::Annotation(*id)));
        keys.extend(self.dim_notes.keys().map(|id| Key::DimNote(*id)));
        keys.extend(self.shown.keys().map(|r| Key::Shown(*r)));
        keys.extend(self.only_in.keys().map(|r| Key::OnlyIn(*r)));
        keys.extend(self.styles.keys().map(|r| Key::Style(*r)));
        keys.push(Key::Draft);
        keys.push(Key::Patterns);
        keys.push(Key::Logo);
        keys.push(Key::Rules);
        keys.push(Key::Project);
        keys.push(Key::Fonts);
        keys.extend(self.sheets.keys().map(|id| Key::Sheet(*id)));
        keys.extend(self.views.keys().map(|id| Key::View(*id)));
        keys.extend(self.groups.keys().map(|id| Key::Group(*id)));
        keys.extend(self.paths.keys().map(|id| Key::Path(*id)));
        keys.extend(self.blocks.keys().map(|id| Key::Block(*id)));
        keys.extend(self.instances.keys().map(|id| Key::Instance(*id)));
        keys.extend(self.underlays.keys().map(|id| Key::Underlay(*id)));
        keys.extend(self.texts.keys().map(|id| Key::Text(*id)));
        keys.extend(self.walls.keys().map(|id| Key::Wall(*id)));
        keys.extend(self.property_dims.keys().map(|id| Key::PropertyDim(*id)));
        keys.extend(self.openings.keys().map(|id| Key::Opening(*id)));
        keys
    }
}

/// Undo/redo stacks of [`Transaction`]s.
#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    /// Steps recorded so far and still steps of their own: unlike the undo stack, which
    /// forgets old ones, this only goes down when steps are squashed together or discarded.
    recorded: u64,
}

impl History {
    /// How many steps can be undone; older ones are forgotten, so a long session's history
    /// can't grow without bound.
    pub const MAX_STEPS: usize = 500;

    /// Record a committed transaction. Empty transactions are dropped.
    pub fn push(&mut self, tx: Transaction) {
        if !tx.is_empty() {
            self.undo.push(tx);
            self.recorded += 1;
            self.redo.clear();
            if self.undo.len() > Self::MAX_STEPS {
                let extra = self.undo.len() - Self::MAX_STEPS;
                self.undo.drain(..extra);
            }
        }
    }

    /// Append a transaction's changes to the most recent undo step (so they undo together).
    /// With nothing to merge into, it is pushed as its own step.
    pub fn merge_into_last(&mut self, tx: Transaction) {
        if tx.is_empty() {
            return;
        }
        // A change made now can't be redone over.
        self.redo.clear();
        match self.undo.last_mut() {
            Some(last) => {
                for (key, before, after) in tx.changes {
                    match last.changes.iter_mut().find(|(k, ..)| *k == key) {
                        Some(existing) => existing.2 = after,
                        None => last.changes.push((key, before, after)),
                    }
                }
            }
            None => {
                self.undo.push(tx);
                self.recorded += 1;
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|t| t.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|t| t.label.as_str())
    }

    pub fn undo(&mut self, doc: &mut Document) -> bool {
        let Some(tx) = self.undo.pop() else {
            return false;
        };
        doc.apply(&tx, false);
        self.redo.push(tx);
        true
    }

    pub fn redo(&mut self, doc: &mut Document) -> bool {
        let Some(tx) = self.redo.pop() else {
            return false;
        };
        doc.apply(&tx, true);
        self.undo.push(tx);
        true
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// How many steps have been recorded: the difference between two readings is how many
    /// steps were added between them (steps squashed into one count as one, so nested
    /// squashes count right).
    pub fn recorded(&self) -> u64 {
        self.recorded
    }

    /// How many steps can be undone.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Fold the last `n` undo steps into one labelled `label`, so several edits undo
    /// together.
    pub fn squash_last(&mut self, n: usize, label: &str) {
        let n = n.min(self.undo.len());
        if n == 0 {
            return;
        }
        let mut steps = self.undo.split_off(self.undo.len() - n);
        let mut first = steps.remove(0);
        first.label = label.to_owned();
        self.undo.push(first);
        for tx in steps {
            self.merge_into_last(tx);
        }
        self.recorded -= (n - 1) as u64;
    }

    /// Undo the last `n` steps and forget them (they can't be redone).
    pub fn discard_last(&mut self, n: usize, doc: &mut Document) {
        for _ in 0..n {
            if let Some(tx) = self.undo.pop() {
                doc.apply(&tx, false);
                self.recorded = self.recorded.saturating_sub(1);
            }
        }
    }
}
