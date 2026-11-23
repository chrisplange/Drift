//! The user's keys (#499): which shortcut runs each command, over the registry's defaults.
//!
//! A keymap is a preset (Drift default, AutoCAD-style) or one of the user's own presets: a
//! built-in one with the user's changes on top. The keymap in use lives here, per thread (the
//! UI is one thread; each test has its own), and every place that shows or dispatches a key
//! reads it through [`crate::commands::Spec::shortcuts`]. It is saved with the app's settings
//! in eframe's storage, never in drawings.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use eframe::egui::Key;

use crate::commands::{self, Shortcut, Spec};

/// Where the keymap is kept in eframe's storage.
const KEY_KEYMAP: &str = "drift.keymap";

/// A preset Drift comes with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BuiltIn {
    #[default]
    Drift,
    AutoCad,
}

impl BuiltIn {
    pub(crate) const ALL: [BuiltIn; 2] = [BuiltIn::Drift, BuiltIn::AutoCad];

    pub(crate) fn label(self) -> &'static str {
        match self {
            BuiltIn::Drift => "Drift default",
            BuiltIn::AutoCad => "AutoCAD-style",
        }
    }

    fn key(self) -> &'static str {
        match self {
            BuiltIn::Drift => "drift",
            BuiltIn::AutoCad => "autocad",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.key() == text)
    }

    /// The commands whose keys differ from the registry's, and their keys.
    fn changes(self) -> &'static [(&'static str, &'static [Shortcut])] {
        match self {
            BuiltIn::Drift => &[],
            BuiltIn::AutoCad => AUTOCAD,
        }
    }
}

const fn k(command: bool, shift: bool, alt: bool, key: Key) -> Shortcut {
    Shortcut {
        command,
        shift,
        alt,
        typed: false,
        key,
    }
}
const fn plain(key: Key) -> Shortcut {
    k(false, false, false, key)
}
const fn shift(key: Key) -> Shortcut {
    k(false, true, false, key)
}
const fn cmd(key: Key) -> Shortcut {
    k(true, false, false, key)
}

/// AutoCAD-style: its function keys and Ctrl keys, and its one-letter command aliases where
/// Drift has the command (L line, C circle, A arc, M move, E erase, O offset, F fillet,
/// T text, H hatch, I insert, D dimension…; X is the tools', so explode is Shift+X). What AutoCAD keeps two letters for
/// (TR trim, EX extend, MI mirror, RO rotate, SC scale) is in the command palette as typed.
const AUTOCAD: &[(&str, &[Shortcut])] = &[
    // Drawing aids on the function keys.
    ("view.grid", &[plain(Key::F7)]),
    ("view.snap", &[plain(Key::F3)]),
    ("view.perp_tangent", &[plain(Key::F10)]),
    ("view.auto_relations", &[plain(Key::F8)]),
    ("view.auto_merge", &[plain(Key::F9)]),
    ("view.hide_panels", &[cmd(Key::Num0)]),
    ("view.panel.inspector", &[cmd(Key::Num1)]),
    ("view.palette", &[cmd(Key::K), cmd(Key::Num9)]),
    // One letter for the commands AutoCAD gives one letter.
    ("modify.move", &[plain(Key::M)]),
    ("modify.mirror", &[shift(Key::M)]),
    (
        "edit.delete",
        &[plain(Key::Delete), plain(Key::Backspace), plain(Key::E)],
    ),
    ("modify.extend", &[shift(Key::E)]),
    ("modify.trim", &[shift(Key::T)]),
    ("draw.text", &[plain(Key::T)]),
    ("edit.explode", &[shift(Key::X)]),
    ("modify.rotate", &[shift(Key::R)]),
    ("modify.scale", &[shift(Key::S)]),
    ("draw.point", &[shift(Key::P)]),
    ("draw.pen", &[shift(Key::L)]),
    ("draw.polygon", &[shift(Key::K)]),
];

/// One of the user's presets: a built-in one with changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct UserPreset {
    pub name: String,
    pub base: BuiltIn,
    /// Command id → its keys (empty: no key).
    pub changes: BTreeMap<String, Vec<Shortcut>>,
}

/// Which preset is in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Active {
    BuiltIn(BuiltIn),
    User(usize),
}

impl Default for Active {
    fn default() -> Self {
        Active::BuiltIn(BuiltIn::Drift)
    }
}

/// The presets and which one is in use.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Keymap {
    pub active: Active,
    pub user: Vec<UserPreset>,
}

/// A key someone else already has, for the clash question.
#[derive(Clone, Debug)]
pub(crate) enum Clash {
    /// Another command's key.
    Command(&'static Spec),
    /// A key tools take while in hand (X / Y lock, G, B, Tab, Enter, Esc, the arrows…).
    Tools(&'static str),
}

impl PartialEq for Clash {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Clash::Command(a), Clash::Command(b)) => a.id == b.id,
            (Clash::Tools(a), Clash::Tools(b)) => a == b,
            _ => false,
        }
    }
}

/// Keys the tools use themselves: never given to a command.
pub(crate) const RESERVED: &[(Key, &str)] = &[
    (Key::X, "locking to X while drawing"),
    (Key::Y, "locking to Y while drawing"),
    (Key::G, "grabbing what is picked"),
    (Key::B, "picking a base point"),
    (Key::Tab, "moving between typed values"),
    (Key::Enter, "finishing and repeating"),
    (Key::Space, "repeating the last command"),
    (Key::Escape, "cancelling"),
    (Key::ArrowLeft, "nudging"),
    (Key::ArrowRight, "nudging"),
    (Key::ArrowUp, "nudging"),
    (Key::ArrowDown, "nudging"),
];

impl Keymap {
    /// The preset in use's name.
    pub(crate) fn active_name(&self) -> String {
        match self.active {
            Active::BuiltIn(b) => b.label().to_owned(),
            Active::User(i) => self
                .user
                .get(i)
                .map_or_else(String::new, |p| p.name.clone()),
        }
    }

    /// The keys `spec` has in the preset in use.
    pub(crate) fn keys_of(&self, spec: &Spec) -> Vec<Shortcut> {
        let (base, changes) = match self.active {
            Active::BuiltIn(b) => (b, None),
            Active::User(i) => match self.user.get(i) {
                Some(p) => (p.base, Some(&p.changes)),
                None => (BuiltIn::Drift, None),
            },
        };
        if let Some(keys) = changes.and_then(|c| c.get(spec.id)) {
            return keys.clone();
        }
        Self::base_keys(base, spec)
    }

    /// `spec`'s keys in a built-in preset.
    fn base_keys(base: BuiltIn, spec: &Spec) -> Vec<Shortcut> {
        if let Some((_, keys)) = base.changes().iter().find(|(id, _)| *id == spec.id) {
            return keys.to_vec();
        }
        // A key a preset gives another command is taken from the one that had it.
        let taken = |k: &Shortcut| {
            base.changes()
                .iter()
                .any(|(id, keys)| *id != spec.id && keys.contains(k))
        };
        spec.default_keys
            .iter()
            .filter(|k| !taken(k))
            .copied()
            .collect()
    }

    /// The preset to change: the user's preset in use, or a new one made from the built-in
    /// preset in use (a built-in preset never changes).
    fn editable(&mut self) -> &mut UserPreset {
        if let Active::User(i) = self.active
            && i < self.user.len()
        {
            return &mut self.user[i];
        }
        let base = match self.active {
            Active::BuiltIn(b) => b,
            Active::User(_) => BuiltIn::Drift,
        };
        let name = self.fresh_name();
        self.user.push(UserPreset {
            name,
            base,
            changes: BTreeMap::new(),
        });
        self.active = Active::User(self.user.len() - 1);
        self.user.last_mut().expect("just pushed")
    }

    fn fresh_name(&self) -> String {
        (1..)
            .map(|n| {
                if n == 1 {
                    "My keys".to_owned()
                } else {
                    format!("My keys {n}")
                }
            })
            .find(|name| self.user.iter().all(|p| &p.name != name))
            .expect("a free name")
    }

    /// Give `spec` exactly `keys`.
    pub(crate) fn set(&mut self, spec: &Spec, keys: Vec<Shortcut>) {
        let p = self.editable();
        let base = Self::base_keys(p.base, spec);
        if keys == base {
            p.changes.remove(spec.id);
        } else {
            p.changes.insert(spec.id.to_owned(), keys);
        }
    }

    /// Give `spec` the key `key` (its first, the one menus show), taking it from whoever had
    /// it. Its other keys stay.
    pub(crate) fn assign(&mut self, spec: &Spec, key: Shortcut) {
        if let Some(Clash::Command(other)) = self.clash(spec, key) {
            let keys: Vec<_> = self
                .keys_of(other)
                .into_iter()
                .filter(|k| *k != key)
                .collect();
            self.set(other, keys);
        }
        let mut keys = self.keys_of(spec);
        keys.retain(|k| *k != key);
        keys.insert(0, key);
        self.set(spec, keys);
    }

    /// `spec` with no key.
    pub(crate) fn clear(&mut self, spec: &Spec) {
        self.set(spec, Vec::new());
    }

    /// `spec` back to the keys of the preset the user's preset was made from.
    pub(crate) fn reset(&mut self, spec: &Spec) {
        if let Active::User(i) = self.active
            && let Some(p) = self.user.get_mut(i)
        {
            p.changes.remove(spec.id);
        }
    }

    /// Every command back to the preset the user's preset was made from.
    pub(crate) fn reset_all(&mut self) {
        if let Active::User(i) = self.active
            && let Some(p) = self.user.get_mut(i)
        {
            p.changes.clear();
        }
    }

    /// Whether `spec`'s keys differ from the preset this one was made from.
    pub(crate) fn changed(&self, spec: &Spec) -> bool {
        match self.active {
            Active::User(i) => self
                .user
                .get(i)
                .is_some_and(|p| p.changes.contains_key(spec.id)),
            Active::BuiltIn(_) => false,
        }
    }

    /// Who already has `key`, if giving it to `spec` would take it from them.
    pub(crate) fn clash(&self, spec: &Spec, key: Shortcut) -> Option<Clash> {
        if !key.command
            && !key.shift
            && !key.alt
            && let Some((_, what)) = RESERVED.iter().find(|(k, _)| *k == key.key)
        {
            return Some(Clash::Tools(what));
        }
        commands::registry()
            .iter()
            .filter(|s| s.id != spec.id)
            .find(|s| self.keys_of(s).contains(&key))
            .map(Clash::Command)
    }

    /// The user's preset in use removed; the preset it was made from is used.
    pub(crate) fn delete_active(&mut self) {
        if let Active::User(i) = self.active
            && i < self.user.len()
        {
            let p = self.user.remove(i);
            self.active = Active::BuiltIn(p.base);
        }
    }

    /// As saved: a line naming the preset in use, then each user preset and its changes.
    pub(crate) fn to_text(&self) -> String {
        let mut out = String::from("keymap 1\n");
        out.push_str(&match self.active {
            Active::BuiltIn(b) => format!("active\t{}\n", b.key()),
            Active::User(i) => format!("active\tuser {i}\n"),
        });
        for p in &self.user {
            out.push_str(&format!("preset\t{}\t{}\n", p.base.key(), clean(&p.name)));
            for (id, keys) in &p.changes {
                let keys: Vec<String> = keys.iter().map(|k| shortcut_token(*k)).collect();
                out.push_str(&format!("keys\t{id}\t{}\n", keys.join(" ")));
            }
        }
        out
    }

    /// Read back what [`Keymap::to_text`] wrote. Commands this Drift doesn't have, and keys it
    /// can't read, are left out.
    pub(crate) fn from_text(text: &str) -> Option<Keymap> {
        let mut lines = text.lines();
        if lines.next()? != "keymap 1" {
            return None;
        }
        let mut map = Keymap::default();
        let mut active = None;
        for line in lines {
            let mut parts = line.split('\t');
            match parts.next() {
                Some("active") => active = parts.next().map(str::to_owned),
                Some("preset") => {
                    let base = parts.next().and_then(BuiltIn::parse).unwrap_or_default();
                    let name = parts.next().unwrap_or("My keys").to_owned();
                    map.user.push(UserPreset {
                        name,
                        base,
                        changes: BTreeMap::new(),
                    });
                }
                Some("keys") => {
                    let (Some(id), Some(p)) = (parts.next(), map.user.last_mut()) else {
                        continue;
                    };
                    let Some(spec) = commands::spec_by_id(id) else {
                        continue;
                    };
                    let keys = parts
                        .next()
                        .unwrap_or_default()
                        .split(' ')
                        .filter_map(parse_shortcut)
                        .collect();
                    p.changes.insert(spec.id.to_owned(), keys);
                }
                _ => {}
            }
        }
        map.active = match active.as_deref() {
            Some(a) if a.starts_with("user ") => a[5..]
                .parse()
                .ok()
                .filter(|i| *i < map.user.len())
                .map_or_else(Active::default, Active::User),
            Some(a) => BuiltIn::parse(a).map_or_else(Active::default, Active::BuiltIn),
            None => Active::default(),
        };
        Some(map)
    }
}

/// A name with no tabs or line breaks, to save.
fn clean(name: &str) -> String {
    name.replace(['\t', '\n', '\r'], " ")
}

/// A shortcut as saved: its modifiers (c Ctrl/⌘, s Shift, a Alt, t typed), a colon, the key.
fn shortcut_token(k: Shortcut) -> String {
    let mut out = String::new();
    for (on, c) in [
        (k.command, 'c'),
        (k.shift, 's'),
        (k.alt, 'a'),
        (k.typed, 't'),
    ] {
        if on {
            out.push(c);
        }
    }
    out.push(':');
    out.push_str(k.key.name());
    out
}

fn parse_shortcut(token: &str) -> Option<Shortcut> {
    let (mods, name) = token.split_once(':')?;
    Some(Shortcut {
        command: mods.contains('c'),
        shift: mods.contains('s'),
        alt: mods.contains('a'),
        typed: mods.contains('t'),
        key: Key::from_name(name)?,
    })
}

thread_local! {
    static KEYMAP: RefCell<Keymap> = RefCell::new(Keymap::default());
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// The keymap in use.
pub(crate) fn current() -> Keymap {
    KEYMAP.with(|k| k.borrow().clone())
}

/// Read the keymap in use.
pub(crate) fn with<R>(f: impl FnOnce(&Keymap) -> R) -> R {
    KEYMAP.with(|k| f(&k.borrow()))
}

/// Change the keymap in use.
pub(crate) fn update<R>(f: impl FnOnce(&mut Keymap) -> R) -> R {
    let r = KEYMAP.with(|k| f(&mut k.borrow_mut()));
    GENERATION.with(|g| g.set(g.get() + 1));
    r
}

/// Use `map`.
pub(crate) fn install(map: Keymap) {
    update(|k| *k = map);
}

/// Bumped by every change, for what is made from the keys (the dispatch table).
pub(crate) fn generation() -> u64 {
    GENERATION.with(Cell::get)
}

/// The saved keymap, or Drift's default.
pub(crate) fn load(storage: &dyn eframe::Storage) {
    let map = storage
        .get_string(KEY_KEYMAP)
        .and_then(|t| Keymap::from_text(&t))
        .unwrap_or_default();
    install(map);
}

pub(crate) fn save(storage: &mut dyn eframe::Storage) {
    storage.set_string(KEY_KEYMAP, with(Keymap::to_text));
}

/// The shortcut a key press makes, for assigning: Ctrl (⌘) and the other modifiers held, or
/// `None` for a modifier alone and keys Drift can't give a command (Esc cancels).
pub(crate) fn shortcut_of(key: Key, modifiers: eframe::egui::Modifiers) -> Option<Shortcut> {
    if matches!(key, Key::Escape) {
        return None;
    }
    Some(Shortcut {
        command: modifiers.command,
        shift: modifiers.shift,
        alt: modifiers.alt,
        typed: false,
        key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec_by_id;

    fn spec(id: &str) -> &'static Spec {
        spec_by_id(id).unwrap_or_else(|| panic!("no command {id}"))
    }

    #[test]
    fn the_default_preset_is_the_registry() {
        let map = Keymap::default();
        for s in commands::registry() {
            assert_eq!(map.keys_of(s), s.default_keys, "{}", s.id);
        }
    }

    #[test]
    fn presets_name_real_commands_and_share_no_key() {
        for b in BuiltIn::ALL {
            for (id, _) in b.changes() {
                assert!(spec_by_id(id).is_some(), "{} names {id}", b.label());
            }
            let map = Keymap {
                active: Active::BuiltIn(b),
                user: Vec::new(),
            };
            let mut seen: Vec<(Shortcut, &str)> = Vec::new();
            for s in commands::registry() {
                for key in map.keys_of(s) {
                    if let Some((_, other)) = seen.iter().find(|(o, _)| *o == key) {
                        panic!(
                            "{}: {other} and {} are both {}",
                            b.label(),
                            s.id,
                            key.text(false)
                        );
                    }
                    if !key.command && !key.shift && !key.alt {
                        assert!(
                            !RESERVED.iter().any(|(r, _)| *r == key.key),
                            "{}: {} takes {}",
                            b.label(),
                            s.id,
                            key.text(false)
                        );
                    }
                    seen.push((key, s.id));
                }
            }
        }
    }

    #[test]
    fn assigning_takes_the_key_and_starts_a_users_preset() {
        let mut map = Keymap::default();
        let line = spec("draw.line");
        let circle = spec("draw.circle");
        assert_eq!(map.clash(line, plain(Key::C)), Some(Clash::Command(circle)));
        map.assign(line, plain(Key::C));
        assert_eq!(map.active, Active::User(0));
        assert_eq!(map.active_name(), "My keys");
        assert_eq!(map.keys_of(line), vec![plain(Key::C), plain(Key::L)]);
        assert!(map.keys_of(circle).is_empty());
        assert!(map.changed(line) && map.changed(circle));
        map.reset(circle);
        assert_eq!(map.keys_of(circle), vec![plain(Key::C)]);
        map.clear(line);
        assert!(map.keys_of(line).is_empty());
        map.reset_all();
        assert_eq!(map.keys_of(line), vec![plain(Key::L)]);
        assert_eq!(
            map.clash(line, plain(Key::X)),
            Some(Clash::Tools("locking to X while drawing"))
        );
    }

    #[test]
    fn the_keymap_round_trips_through_text() {
        let mut map = Keymap::default();
        map.active = Active::BuiltIn(BuiltIn::AutoCad);
        map.assign(spec("draw.line"), cmd(Key::Num5));
        map.clear(spec("help.shortcuts"));
        map.assign(spec("view.grid"), k(false, true, true, Key::Questionmark));
        let text = map.to_text();
        let back = Keymap::from_text(&text).expect("reads back");
        assert_eq!(back, map, "{text}");
        assert_eq!(back.user[0].base, BuiltIn::AutoCad);
        assert_eq!(Keymap::from_text("nonsense"), None);
    }
}
