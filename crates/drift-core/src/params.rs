//! Evaluating a document's variables and the dimensions linked to expressions.

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::expr::{self, Evaluated, ExprError};
use crate::units::{Kind, Quantity};
use crate::{ConstraintId, Document, VariableId, dimension_kind};

/// The values of every variable and linked dimension, or why each can't be computed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Evaluation {
    /// Variable values in base units (mm, radians, plain).
    pub variables: BTreeMap<VariableId, Result<Quantity, ExprError>>,
    /// Linked dimension values as dimensions store them (mm, degrees).
    pub dimensions: BTreeMap<ConstraintId, Result<f64, ExprError>>,
}

impl Evaluation {
    /// A variable's value in its kind's stored unit (mm, degrees, plain).
    pub fn value(&self, doc: &Document, id: VariableId) -> Option<Result<f64, ExprError>> {
        let kind = doc.variable(id)?.kind;
        Some(
            self.variables
                .get(&id)?
                .clone()
                .and_then(|q| expr::to_kind(q, kind, doc.units().length)),
        )
    }

    /// The first problem, for a status message.
    pub fn first_error(&self, doc: &Document) -> Option<String> {
        let var = self.variables.iter().find_map(|(id, r)| {
            r.as_ref().err().map(|e| {
                let name = doc.variable(*id).map_or("?", |v| v.name.as_str());
                format!("{name}: {e}")
            })
        });
        var.or_else(|| {
            self.dimensions
                .iter()
                .find_map(|(id, r)| r.as_ref().err().map(|e| format!("dimension {id}: {e}")))
        })
    }

    /// True if everything evaluated.
    pub fn is_ok(&self) -> bool {
        self.variables.values().all(Result::is_ok) && self.dimensions.values().all(Result::is_ok)
    }
}

/// Evaluate all variables (in dependency order, detecting cycles) and linked dimensions.
pub fn evaluate(doc: &Document) -> Evaluation {
    let cache: RefCell<BTreeMap<VariableId, Result<Quantity, ExprError>>> = Default::default();
    let stack: RefCell<Vec<VariableId>> = Default::default();
    for (id, _) in doc.variables() {
        // Results are cached; errors are read back from the cache.
        let _ = eval_variable(doc, id, &cache, &stack);
    }
    let variables = cache.into_inner();
    let lookup = by_name(doc, &variables);
    let mut dimensions = BTreeMap::new();
    for (id, text) in doc.expressions() {
        let Some(c) = doc.constraint(id) else {
            continue;
        };
        let kind = dimension_kind(c);
        let result = expr::parse(text)
            .and_then(|e| e.eval(&lookup))
            .and_then(|q| expr::to_kind(q, kind, doc.units().length))
            .and_then(|v| {
                if c.with_value(v).value_is_valid() {
                    Ok(v)
                } else {
                    Err(ExprError(format!(
                        "{} isn't a valid value here",
                        doc.units().format(kind, v)
                    )))
                }
            });
        dimensions.insert(id, result);
    }
    drop(lookup);
    Evaluation {
        variables,
        dimensions,
    }
}

/// Look variables up by name in computed values.
fn by_name<'a>(
    doc: &'a Document,
    values: &'a BTreeMap<VariableId, Result<Quantity, ExprError>>,
) -> impl Fn(&str) -> Option<Result<Quantity, ExprError>> + 'a {
    move |name| {
        let id = doc.variable_named(name)?;
        Some(
            values
                .get(&id)
                .cloned()
                .unwrap_or_else(|| Err(ExprError(format!("`{name}` has no value"))))
                .map_err(|e| ExprError(format!("`{name}`: {e}"))),
        )
    }
}

fn eval_variable(
    doc: &Document,
    id: VariableId,
    cache: &RefCell<BTreeMap<VariableId, Result<Quantity, ExprError>>>,
    stack: &RefCell<Vec<VariableId>>,
) -> Result<Quantity, ExprError> {
    if let Some(r) = cache.borrow().get(&id) {
        return r.clone();
    }
    let Some(var) = doc.variable(id) else {
        return Err(ExprError(format!("variable {id} does not exist")));
    };
    if stack.borrow().contains(&id) {
        let names: Vec<_> = stack
            .borrow()
            .iter()
            .skip_while(|v| **v != id)
            .chain(std::iter::once(&id))
            .filter_map(|v| doc.variable(*v).map(|x| x.name.clone()))
            .collect();
        return Err(ExprError(format!(
            "circular reference: {}",
            names.join(" → ")
        )));
    }
    stack.borrow_mut().push(id);
    let lookup = |name: &str| {
        let other = doc.variable_named(name)?;
        Some(eval_variable(doc, other, cache, stack).map_err(|e| {
            if e.0.starts_with("circular") {
                e
            } else {
                ExprError(format!("`{name}`: {e}"))
            }
        }))
    };
    let result = expr::parse(&var.expr)
        .and_then(|e| e.eval(&lookup))
        .and_then(|q| expr::to_kind(q, var.kind, doc.units().length))
        .map(|v| expr::from_kind(v, var.kind));
    stack.borrow_mut().pop();
    // A variable on a cycle gets the cycle error; don't cache partial results mid-cycle.
    cache.borrow_mut().insert(id, result.clone());
    result
}

/// Evaluate typed input for a value of `kind`, with the document's variables and units.
pub fn evaluate_text(
    doc: &Document,
    eval: &Evaluation,
    text: &str,
    kind: Kind,
) -> Result<Evaluated, ExprError> {
    let lookup = by_name(doc, &eval.variables);
    expr::evaluate_input(text, kind, doc.units().length, &lookup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Constraint, DVec2, Variable};

    fn var(name: &str, expr: &str, kind: Kind) -> Variable {
        Variable {
            name: name.into(),
            expr: expr.into(),
            kind,
        }
    }

    #[test]
    fn variables_evaluate_in_dependency_order() {
        let mut doc = Document::new();
        let hole = doc
            .add_variable(var("hole", "width / 10", Kind::Length))
            .unwrap();
        let width = doc
            .add_variable(var("width", "120 mm", Kind::Length))
            .unwrap();
        let n = doc.add_variable(var("n", "4", Kind::Number)).unwrap();
        let tilt = doc.add_variable(var("tilt", "30", Kind::Angle)).unwrap();
        let eval = evaluate(&doc);
        assert!(eval.is_ok());
        assert_eq!(eval.value(&doc, hole), Some(Ok(12.0)));
        assert_eq!(eval.value(&doc, width), Some(Ok(120.0)));
        assert_eq!(eval.value(&doc, n), Some(Ok(4.0)));
        assert_eq!(eval.value(&doc, tilt), Some(Ok(30.0)));
    }

    #[test]
    fn cycles_and_errors_are_reported() {
        let mut doc = Document::new();
        let a = doc.add_variable(var("a", "b + 1mm", Kind::Length)).unwrap();
        doc.add_variable(var("b", "a * 2", Kind::Length)).unwrap();
        let c = doc.add_variable(var("c", "a + 1mm", Kind::Length)).unwrap();
        let eval = evaluate(&doc);
        let err = eval.value(&doc, a).unwrap().unwrap_err().0;
        assert!(err.contains("circular reference"), "{err}");
        let err = eval.value(&doc, c).unwrap().unwrap_err().0;
        assert!(err.contains("circular"), "{err}");
        assert!(eval.first_error(&doc).is_some());
    }

    #[test]
    fn linked_dimensions_follow_variables() {
        let mut doc = Document::new();
        let p = doc.add_point(DVec2::ZERO);
        let q = doc.add_point(DVec2::new(10.0, 0.0));
        let d = doc
            .add_constraint(Constraint::Distance {
                a: p,
                b: q,
                value: 10.0,
            })
            .unwrap();
        doc.add_variable(var("w", "25 mm", Kind::Length)).unwrap();
        doc.set_expression(d, Some("w * 2".into()));
        let eval = evaluate(&doc);
        assert_eq!(eval.dimensions.get(&d), Some(&Ok(50.0)));
        // Invalid values (negative lengths) are errors.
        doc.set_expression(d, Some("-w".into()));
        assert!(evaluate(&doc).dimensions[&d].is_err());
        // Typed input sees variables.
        let r = evaluate_text(&doc, &evaluate(&doc), "w + 1", Kind::Length);
        assert!(r.is_err(), "mixing a length and a number");
        let r = evaluate_text(&doc, &evaluate(&doc), "w + 1mm", Kind::Length).unwrap();
        assert_eq!((r.value, r.references), (26.0, vec!["w".to_owned()]));
    }

    #[test]
    fn renaming_updates_users_and_removing_checks_them() {
        let mut doc = Document::new();
        let w = doc.add_variable(var("w", "25 mm", Kind::Length)).unwrap();
        let h = doc.add_variable(var("h", "w / 2", Kind::Length)).unwrap();
        doc.rename_variable(w, "width").unwrap();
        assert_eq!(doc.variable(h).unwrap().expr, "width / 2");
        assert!(doc.rename_variable(h, "width").is_err(), "duplicate name");
        assert!(doc.rename_variable(h, "in").is_err(), "reserved");
        assert!(matches!(
            doc.remove_variable(w),
            Err(crate::DocumentError::InUse(_))
        ));
        doc.remove_variable(h).unwrap();
        doc.remove_variable(w).unwrap();
    }

    #[test]
    fn parameter_edits_undo() {
        let mut doc = Document::new();
        let mut history = crate::History::default();
        doc.begin();
        let w = doc.add_variable(var("w", "25 mm", Kind::Length)).unwrap();
        history.push(doc.commit("Add"));
        doc.begin();
        doc.set_variable_expr(w, "30 mm").unwrap();
        doc.set_units(crate::Units::architectural());
        history.push(doc.commit("Edit"));
        assert!(history.undo(&mut doc));
        assert_eq!(doc.variable(w).unwrap().expr, "25 mm");
        assert_eq!(doc.units(), crate::Units::default());
        assert!(history.undo(&mut doc));
        assert!(doc.variable(w).is_none());
    }
}
