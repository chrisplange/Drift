//! Units, variables, expressions, variants (options) and pinned measurements, and the
//! whole-sketch copies options are kept in.

use super::*;

/// Stable identifier of a variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VariableId(pub u32);

impl std::fmt::Display for VariableId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// A named value defined by an expression (`width = 120 mm`, `hole = width / 10`).
#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub name: String,
    pub expr: String,
    pub kind: Kind,
}

/// Stable identifier of a variant (configuration).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VariantId(pub u32);

impl std::fmt::Display for VariantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "k{}", self.0)
    }
}

/// A design option: a named state of the drawing to return to in one step. Either a set of
/// variable expressions ("values only", applied over the current sketch) or a copy of the
/// whole sketch (`sketch`; `values` is then empty).
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub name: String,
    pub values: BTreeMap<VariableId, String>,
    /// The whole sketch, without options of its own (a copy shares the document's parts).
    pub sketch: Option<Shared<Document>>,
}

impl Variant {
    /// An option holding variable values.
    pub fn values(name: impl Into<String>, values: BTreeMap<VariableId, String>) -> Self {
        Self {
            name: name.into(),
            values,
            sketch: None,
        }
    }

    /// An option holding a copy of the whole sketch.
    pub fn sketch(name: impl Into<String>, doc: &Document) -> Self {
        Self {
            name: name.into(),
            values: BTreeMap::new(),
            sketch: Some(Shared::new(doc.sketch_copy())),
        }
    }

    pub fn is_sketch(&self) -> bool {
        self.sketch.is_some()
    }
}

/// Stable identifier of a pinned measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MeasurementId(pub u32);

impl std::fmt::Display for MeasurementId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "m{}", self.0)
    }
}

impl Document {
    /// Make this document's content equal to `other`'s, as journalled changes (so it can be
    /// undone).
    #[cfg(test)]
    pub fn restore_from(&mut self, other: &Document) {
        self.restore_parts(other, true);
    }

    /// [`restore_from`](Self::restore_from), keeping the options if not `options`.
    pub(super) fn restore_parts(&mut self, other: &Document, options: bool) {
        let mut keys = self.keys();
        keys.extend(other.keys());
        if !options {
            keys.retain(|k| !matches!(k, Key::Variant(_)));
        }
        keys.sort();
        keys.dedup();
        for key in keys {
            let theirs = other.record(key);
            if self.record(key) != theirs {
                self.touch_key(key);
                self.restore(key, theirs);
            }
        }
        self.next_id = self.next_id.max(other.next_id);
        self.next_constraint_id = self.next_constraint_id.max(other.next_constraint_id);
        self.next_param_id = self.next_param_id.max(other.next_param_id);
        self.next_drafting_id = self.next_drafting_id.max(other.next_drafting_id);
        self.next_wall_id = self.next_wall_id.max(other.next_wall_id);
    }

    /// A copy of the drawing for a whole-sketch option: everything but its options (and no
    /// open transaction). Cheap: its parts are shared until changed.
    pub fn sketch_copy(&self) -> Document {
        let mut copy = self.clone();
        copy.variants = Shared::default();
        copy.journal = None;
        copy.last_anchor_pos.clear();
        copy
    }

    /// Make the drawing what `sketch` holds, keeping its options (inside a transaction, so it
    /// can be undone). Applies a whole-sketch option.
    pub fn restore_sketch(&mut self, sketch: &Document) {
        self.restore_parts(sketch, false);
    }

    /// True if the drawing is what `sketch` holds (its options aside).
    pub fn same_sketch(&self, sketch: &Document) -> bool {
        let mut me = self.clone();
        me.variants = Shared::default();
        me == *sketch
    }

    pub fn units(&self) -> Units {
        self.units
    }

    pub fn set_units(&mut self, units: Units) {
        if self.units != units {
            self.touch_key(Key::Units);
            self.units = units;
        }
    }

    pub(super) fn next_param(&mut self) -> u32 {
        let id = self.next_param_id;
        self.next_param_id += 1;
        id
    }

    /// Make sure later ids don't collide with `id` (when loading stored ids).
    pub(super) fn reserve_param(&mut self, id: u32) {
        self.next_param_id = self.next_param_id.max(id + 1);
    }

    /// All variables in id order.
    pub fn variables(&self) -> impl Iterator<Item = (VariableId, &Variable)> {
        self.variables.iter().map(|(id, v)| (*id, v))
    }

    pub fn variable(&self, id: VariableId) -> Option<&Variable> {
        self.variables.get(&id)
    }

    pub fn variable_named(&self, name: &str) -> Option<VariableId> {
        self.variables
            .iter()
            .find(|(_, v)| v.name == name)
            .map(|(id, _)| *id)
    }

    pub(super) fn check_name(
        &self,
        name: &str,
        except: Option<VariableId>,
    ) -> Result<(), DocumentError> {
        crate::expr::valid_name(name).map_err(|e| DocumentError::BadName(e.0))?;
        match self.variable_named(name) {
            Some(other) if Some(other) != except => Err(DocumentError::BadName(format!(
                "there is already a variable called `{name}`"
            ))),
            _ => Ok(()),
        }
    }

    /// Add a variable. The expression isn't evaluated here (see [`crate::params`]).
    pub fn add_variable(&mut self, variable: Variable) -> Result<VariableId, DocumentError> {
        let id = VariableId(self.next_param_id);
        self.insert_variable(id, variable)?;
        Ok(id)
    }

    /// Add a variable with a given id (when loading a file).
    pub fn insert_variable(
        &mut self,
        id: VariableId,
        variable: Variable,
    ) -> Result<(), DocumentError> {
        self.check_name(&variable.name, None)?;
        self.reserve_param(id.0);
        self.touch_key(Key::Variable(id));
        self.variables.insert(id, variable);
        Ok(())
    }

    pub fn set_variable_expr(&mut self, id: VariableId, expr: &str) -> Result<(), DocumentError> {
        let v = self
            .variables
            .get(&id)
            .ok_or(DocumentError::MissingVariable(id))?;
        if v.expr != expr {
            self.touch_key(Key::Variable(id));
            if let Some(v) = self.variables.get_mut(&id) {
                v.expr = expr.to_owned();
            }
        }
        Ok(())
    }

    pub fn set_variable_kind(&mut self, id: VariableId, kind: Kind) -> Result<(), DocumentError> {
        let v = self
            .variables
            .get(&id)
            .ok_or(DocumentError::MissingVariable(id))?;
        if v.kind != kind {
            self.touch_key(Key::Variable(id));
            if let Some(v) = self.variables.get_mut(&id) {
                v.kind = kind;
            }
        }
        Ok(())
    }

    /// Rename a variable, updating every expression that uses it.
    pub fn rename_variable(&mut self, id: VariableId, name: &str) -> Result<(), DocumentError> {
        let old = self
            .variables
            .get(&id)
            .ok_or(DocumentError::MissingVariable(id))?
            .name
            .clone();
        if old == name {
            return Ok(());
        }
        self.check_name(name, Some(id))?;
        let rename = |text: &str| crate::expr::rename(text, &old, name);
        let vars: Vec<_> = self.variables.keys().copied().collect();
        for v in vars {
            let mut var = self.variables[&v].clone();
            let expr = rename(&var.expr);
            if v == id || expr != var.expr {
                if v == id {
                    var.name = name.to_owned();
                }
                var.expr = expr;
                self.touch_key(Key::Variable(v));
                self.variables.insert(v, var);
            }
        }
        let exprs: Vec<_> = self
            .expressions
            .iter()
            .map(|(c, e)| (*c, rename(e)))
            .filter(|(c, e)| self.expressions[c] != *e)
            .collect();
        for (c, e) in exprs {
            self.touch_key(Key::Expression(c));
            self.expressions.insert(c, e);
        }
        // Walls' thickness expressions.
        let walls: Vec<(WallId, Wall)> = self
            .walls
            .iter()
            .filter_map(|(id, w)| {
                let e = w.thickness_expr.as_deref()?;
                let renamed = rename(e);
                (renamed != e).then(|| {
                    let mut w = w.clone();
                    w.thickness_expr = Some(renamed);
                    (*id, w)
                })
            })
            .collect();
        for (id, w) in walls {
            self.touch_key(Key::Wall(id));
            self.walls.insert(id, w);
        }
        // Openings' offset and width expressions.
        let openings: Vec<(OpeningId, Opening)> = self
            .openings
            .iter()
            .filter_map(|(id, o)| {
                let mut o2 = o.clone();
                if let Some(e) = &mut o2.width_expr {
                    *e = rename(e);
                }
                if let crate::OpeningAt::From { expr: Some(e), .. } = &mut o2.at {
                    *e = rename(e);
                }
                (o2 != *o).then_some((*id, o2))
            })
            .collect();
        for (id, o) in openings {
            self.touch_key(Key::Opening(id));
            self.openings.insert(id, o);
        }
        let variants: Vec<_> = self.variants.keys().copied().collect();
        for k in variants {
            let mut variant = self.variants[&k].clone();
            for e in variant.values.values_mut() {
                *e = rename(e);
            }
            if variant != self.variants[&k] {
                self.touch_key(Key::Variant(k));
                self.variants.insert(k, variant);
            }
        }
        Ok(())
    }

    /// What uses variable `id`: names of other variables and ids of dimensions.
    pub fn users_of_variable(&self, id: VariableId) -> Vec<String> {
        let Some(name) = self.variables.get(&id).map(|v| v.name.as_str()) else {
            return Vec::new();
        };
        let uses = |text: &str| {
            crate::expr::parse(text).is_ok_and(|e| e.identifiers().iter().any(|i| i == name))
        };
        let mut users: Vec<String> = self
            .variables
            .iter()
            .filter(|(v, var)| **v != id && uses(&var.expr))
            .map(|(_, var)| var.name.clone())
            .collect();
        users.extend(
            self.expressions
                .iter()
                .filter(|(_, e)| uses(e))
                .map(|(c, _)| format!("dimension {c}")),
        );
        users.extend(
            self.walls
                .iter()
                .filter(|(_, w)| w.thickness_expr.as_deref().is_some_and(uses))
                .map(|(w, _)| format!("wall {w}")),
        );
        users.extend(
            self.openings
                .iter()
                .filter(|(_, o)| {
                    o.width_expr.as_deref().is_some_and(uses)
                        || matches!(&o.at, crate::OpeningAt::From { expr: Some(e), .. } if uses(e))
                })
                .map(|(o, _)| format!("opening {o}")),
        );
        users
    }

    /// Remove a variable that nothing uses (variant values for it go too).
    pub fn remove_variable(&mut self, id: VariableId) -> Result<(), DocumentError> {
        if !self.variables.contains_key(&id) {
            return Err(DocumentError::MissingVariable(id));
        }
        let users = self.users_of_variable(id);
        if !users.is_empty() {
            return Err(DocumentError::InUse(users.join(", ")));
        }
        self.touch_key(Key::Variable(id));
        self.variables.remove(&id);
        let variants: Vec<_> = self
            .variants
            .iter()
            .filter(|(_, v)| v.values.contains_key(&id))
            .map(|(k, _)| *k)
            .collect();
        for k in variants {
            self.touch_key(Key::Variant(k));
            if let Some(v) = self.variants.get_mut(&k) {
                v.values.remove(&id);
            }
        }
        Ok(())
    }

    /// The expression a dimension's value is linked to, if any.
    pub fn expression(&self, id: ConstraintId) -> Option<&str> {
        self.expressions.get(&id).map(String::as_str)
    }

    /// All dimension expressions.
    pub fn expressions(&self) -> impl Iterator<Item = (ConstraintId, &str)> {
        self.expressions.iter().map(|(id, e)| (*id, e.as_str()))
    }

    /// Link a dimension to an expression, or unlink it (`None`).
    pub fn set_expression(&mut self, id: ConstraintId, expr: Option<String>) {
        if self.expressions.get(&id) != expr.as_ref() {
            self.touch_key(Key::Expression(id));
            match expr {
                Some(e) => {
                    self.expressions.insert(id, e);
                }
                None => {
                    self.expressions.remove(&id);
                }
            }
        }
    }

    pub fn variants(&self) -> impl Iterator<Item = (VariantId, &Variant)> {
        self.variants.iter().map(|(id, v)| (*id, v))
    }

    pub fn variant(&self, id: VariantId) -> Option<&Variant> {
        self.variants.get(&id)
    }

    pub fn add_variant(&mut self, variant: Variant) -> VariantId {
        let id = VariantId(self.next_param());
        self.touch_key(Key::Variant(id));
        self.variants.insert(id, variant);
        id
    }

    /// Add or replace a variant with a given id.
    pub fn set_variant(&mut self, id: VariantId, variant: Variant) {
        self.reserve_param(id.0);
        if self.variants.get(&id) != Some(&variant) {
            self.touch_key(Key::Variant(id));
            self.variants.insert(id, variant);
        }
    }

    pub fn remove_variant(&mut self, id: VariantId) {
        if self.variants.contains_key(&id) {
            self.touch_key(Key::Variant(id));
            self.variants.remove(&id);
        }
    }

    pub fn measurements(&self) -> impl Iterator<Item = (MeasurementId, &Constraint)> {
        self.measurements.iter().map(|(id, c)| (*id, c))
    }

    /// Pin a measurement (a dimension definition that is only measured).
    pub fn add_measurement(&mut self, c: Constraint) -> Result<MeasurementId, DocumentError> {
        let id = MeasurementId(self.next_param_id);
        self.insert_measurement(id, c)?;
        Ok(id)
    }

    pub fn insert_measurement(
        &mut self,
        id: MeasurementId,
        c: Constraint,
    ) -> Result<(), DocumentError> {
        if !c.is_dimension() {
            return Err(DocumentError::InvalidValue);
        }
        // The value is irrelevant: validate the references only.
        self.validate(&c.with_value(1.0))?;
        self.reserve_param(id.0);
        self.touch_key(Key::Measurement(id));
        self.measurements.insert(id, c);
        Ok(())
    }

    pub fn remove_measurement(&mut self, id: MeasurementId) {
        if self.measurements.contains_key(&id) {
            self.touch_key(Key::Measurement(id));
            self.measurements.remove(&id);
        }
    }
}
