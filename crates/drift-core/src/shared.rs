//! Copy-on-write parts of a document, so copying one (undo snapshots, options, ghosts, agent
//! edits) shares them until one is changed.

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// A value shared between copies: cloning it is a reference count, and the first change
/// through a copy that isn't the only one copies the value (`Arc::make_mut`).
#[derive(Default)]
pub struct Shared<T>(Arc<T>);

impl<T> Shared<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(value))
    }

    /// True if both are the same value in memory (so equal without comparing).
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T> From<T> for Shared<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: Clone> DerefMut for Shared<T> {
    fn deref_mut(&mut self) -> &mut T {
        Arc::make_mut(&mut self.0)
    }
}

impl<T: PartialEq> PartialEq for Shared<T> {
    fn eq(&self, other: &Self) -> bool {
        Self::ptr_eq(self, other) || *self.0 == *other.0
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<'a, T> IntoIterator for &'a Shared<T>
where
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        (**self).into_iter()
    }
}

impl<'a, T: Clone> IntoIterator for &'a mut Shared<T>
where
    &'a mut T: IntoIterator,
{
    type Item = <&'a mut T as IntoIterator>::Item;
    type IntoIter = <&'a mut T as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        (**self).into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_is_shared_until_it_is_changed() {
        let a = Shared::new(vec![1, 2, 3]);
        let mut b = a.clone();
        assert!(Shared::ptr_eq(&a, &b));
        b.push(4);
        assert!(!Shared::ptr_eq(&a, &b));
        assert_eq!(*a, [1, 2, 3]);
        assert_eq!(*b, [1, 2, 3, 4]);
        // The only owner changes in place.
        let before = b.as_ptr();
        b[0] = 9;
        assert_eq!(b.as_ptr(), before);
    }
}
