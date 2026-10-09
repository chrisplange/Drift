//! Where the side panels are: docked as tabs at either side of the canvas, floating over it,
//! or hidden. This is the layout state only (no egui), so it can be tested directly; the
//! panels themselves are drawn in `app::dock_ui`.

use std::collections::HashMap;

/// A panel that can be docked, floated or hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Panel {
    Inspector,
    Parameters,
    Layers,
    Sheets,
    Blocks,
}

impl Panel {
    pub const ALL: [Panel; 5] = [
        Panel::Inspector,
        Panel::Parameters,
        Panel::Layers,
        Panel::Sheets,
        Panel::Blocks,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Panel::Inspector => "Inspector",
            Panel::Parameters => "Parameters",
            Panel::Layers => "Layers",
            Panel::Sheets => "Sheets",
            Panel::Blocks => "Blocks",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Panel::Inspector => "The selection's geometry, appearance, relations and dimensions",
            Panel::Parameters => "Units, variables, variants, snapshots and pinned measurements",
            Panel::Layers => "Layers and underlays",
            Panel::Sheets => "Sheets, title blocks and output",
            Panel::Blocks => "Blocks and their copies",
        }
    }

    /// Name in saved preferences.
    pub fn key(self) -> &'static str {
        match self {
            Panel::Inspector => "inspector",
            Panel::Parameters => "parameters",
            Panel::Layers => "layers",
            Panel::Sheets => "sheets",
            Panel::Blocks => "blocks",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Panel::ALL.into_iter().find(|p| p.key() == s)
    }
}

/// A side of the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn index(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Right => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }
}

/// Where a panel is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Dock(Side),
    Float,
    Hidden,
}

/// The panel layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// Tabs in each dock (left, right), in order.
    pub docks: [Vec<Panel>; 2],
    /// The shown tab of each dock.
    pub active: [Option<Panel>; 2],
    /// A collapsed dock shows only a rail of icons.
    pub collapsed: [bool; 2],
    pub floating: Vec<Panel>,
    /// Where each hidden panel was, so showing it again puts it back.
    pub last: HashMap<Panel, Place>,
}

impl Default for Layout {
    /// Everything in the right dock, the inspector in front.
    fn default() -> Self {
        Self {
            docks: [Vec::new(), Panel::ALL.to_vec()],
            active: [None, Some(Panel::Inspector)],
            collapsed: [false, false],
            floating: Vec::new(),
            last: HashMap::new(),
        }
    }
}

impl Layout {
    pub fn place(&self, p: Panel) -> Place {
        if self.docks[0].contains(&p) {
            Place::Dock(Side::Left)
        } else if self.docks[1].contains(&p) {
            Place::Dock(Side::Right)
        } else if self.floating.contains(&p) {
            Place::Float
        } else {
            Place::Hidden
        }
    }

    pub fn dock(&self, side: Side) -> &[Panel] {
        &self.docks[side.index()]
    }

    /// Whether the panel can be seen: in front in an open dock, or floating.
    pub fn is_shown(&self, p: Panel) -> bool {
        match self.place(p) {
            Place::Dock(side) => {
                let i = side.index();
                self.active[i] == Some(p) && !self.collapsed[i]
            }
            Place::Float => true,
            Place::Hidden => false,
        }
    }

    /// Take a panel out of wherever it is, fixing up its dock's front tab.
    fn take(&mut self, p: Panel) {
        for i in 0..2 {
            if let Some(at) = self.docks[i].iter().position(|q| *q == p) {
                self.docks[i].remove(at);
                if self.active[i] == Some(p) {
                    let next = at.min(self.docks[i].len().saturating_sub(1));
                    self.active[i] = self.docks[i].get(next).copied();
                }
            }
        }
        self.floating.retain(|q| *q != p);
    }

    /// Move a panel: into a dock (at `index`, or last) in front, floating, or hidden.
    pub fn move_to(&mut self, p: Panel, place: Place, index: Option<usize>) {
        let was = self.place(p);
        if place == Place::Hidden && was != Place::Hidden {
            self.last.insert(p, was);
        }
        // Reordering within a dock: the index counts the panel itself.
        let index = match (was, place, index) {
            (Place::Dock(a), Place::Dock(b), Some(i)) if a == b => {
                let from = self.docks[a.index()].iter().position(|q| *q == p);
                Some(match from {
                    Some(from) if from < i => i - 1,
                    _ => i,
                })
            }
            (_, _, i) => i,
        };
        self.take(p);
        match place {
            Place::Dock(side) => {
                let i = side.index();
                let at = index
                    .unwrap_or(self.docks[i].len())
                    .min(self.docks[i].len());
                self.docks[i].insert(at, p);
                self.active[i] = Some(p);
                self.collapsed[i] = false;
            }
            Place::Float => self.floating.push(p),
            Place::Hidden => {}
        }
    }

    /// Bring a panel into view: in front of its dock (opening it), or back where it was if
    /// hidden.
    pub fn show(&mut self, p: Panel) {
        match self.place(p) {
            Place::Dock(side) => {
                let i = side.index();
                self.active[i] = Some(p);
                self.collapsed[i] = false;
            }
            Place::Float => {}
            Place::Hidden => {
                let back = self
                    .last
                    .get(&p)
                    .copied()
                    .filter(|b| *b != Place::Hidden)
                    .unwrap_or(Place::Dock(Side::Right));
                self.move_to(p, back, None);
            }
        }
    }

    /// Show a panel, or hide it if it is already in view.
    pub fn toggle(&mut self, p: Panel) {
        if self.is_shown(p) {
            self.move_to(p, Place::Hidden, None);
        } else {
            self.show(p);
        }
    }

    /// Preferences as `key=value` pairs for the view settings string.
    pub fn prefs(&self) -> Vec<String> {
        let list = |v: &[Panel]| v.iter().map(|p| p.key()).collect::<Vec<_>>().join(",");
        let active = |a: Option<Panel>| a.map_or("", Panel::key);
        let mut out = vec![
            format!("dock_left={}", list(&self.docks[0])),
            format!("dock_right={}", list(&self.docks[1])),
            format!("dock_float={}", list(&self.floating)),
            format!("dock_front_left={}", active(self.active[0])),
            format!("dock_front_right={}", active(self.active[1])),
            format!(
                "dock_collapsed={}{}",
                u8::from(self.collapsed[0]),
                u8::from(self.collapsed[1])
            ),
        ];
        let mut last: Vec<String> = self
            .last
            .iter()
            .filter(|(p, _)| self.place(**p) == Place::Hidden)
            .map(|(p, place)| {
                let at = match place {
                    Place::Dock(side) => side.name(),
                    Place::Float => "float",
                    Place::Hidden => "hidden",
                };
                format!("{}:{at}", p.key())
            })
            .collect();
        last.sort();
        out.push(format!("dock_last={}", last.join(",")));
        out
    }

    /// Read the layout back from `(key, value)` pairs (see [`Layout::prefs`]). Without any
    /// dock keys the layout is left as it is.
    pub fn apply(&mut self, pairs: &[(&str, &str)]) {
        if !pairs.iter().any(|(k, _)| k.starts_with("dock_")) {
            return;
        }
        let mut layout = Layout {
            docks: [Vec::new(), Vec::new()],
            active: [None, None],
            collapsed: [false, false],
            floating: Vec::new(),
            last: HashMap::new(),
        };
        let panels = |v: &str| -> Vec<Panel> { v.split(',').filter_map(Panel::parse).collect() };
        for (key, value) in pairs {
            match *key {
                "dock_left" => layout.docks[0] = panels(value),
                "dock_right" => layout.docks[1] = panels(value),
                "dock_float" => layout.floating = panels(value),
                "dock_front_left" => layout.active[0] = Panel::parse(value),
                "dock_front_right" => layout.active[1] = Panel::parse(value),
                "dock_collapsed" => {
                    let b = value.as_bytes();
                    layout.collapsed = [b.first() == Some(&b'1'), b.get(1) == Some(&b'1')];
                }
                "dock_last" => {
                    for entry in value.split(',') {
                        let Some((p, at)) = entry.split_once(':') else {
                            continue;
                        };
                        let place = match at {
                            "left" => Place::Dock(Side::Left),
                            "right" => Place::Dock(Side::Right),
                            "float" => Place::Float,
                            _ => continue,
                        };
                        if let Some(p) = Panel::parse(p) {
                            layout.last.insert(p, place);
                        }
                    }
                }
                _ => {}
            }
        }
        // Each panel in one place only.
        let mut seen = Vec::new();
        let [left, right] = &mut layout.docks;
        for list in [left, right, &mut layout.floating] {
            list.retain(|p| {
                let new = !seen.contains(p);
                seen.push(*p);
                new
            });
        }
        for i in 0..2 {
            if !layout.active[i].is_some_and(|a| layout.docks[i].contains(&a)) {
                layout.active[i] = layout.docks[i].first().copied();
            }
        }
        *self = layout;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panels_move_between_docks_float_and_hide() {
        let mut l = Layout::default();
        assert!(l.is_shown(Panel::Inspector));
        assert!(!l.is_shown(Panel::Layers));
        // Docking on the left brings it to the front there; the right keeps its front tab.
        l.move_to(Panel::Layers, Place::Dock(Side::Left), None);
        assert_eq!(l.dock(Side::Left), [Panel::Layers]);
        assert!(l.is_shown(Panel::Layers) && l.is_shown(Panel::Inspector));
        // Moving the front tab out shows its neighbour.
        l.move_to(Panel::Inspector, Place::Float, None);
        assert_eq!(l.active[1], Some(Panel::Parameters));
        assert!(l.is_shown(Panel::Inspector));
        // Hidden panels come back where they were.
        l.toggle(Panel::Inspector);
        assert_eq!(l.place(Panel::Inspector), Place::Hidden);
        l.toggle(Panel::Inspector);
        assert_eq!(l.place(Panel::Inspector), Place::Float);
        // A docked tab behind another is shown, not hidden, by toggling.
        l.toggle(Panel::Sheets);
        assert_eq!(l.active[1], Some(Panel::Sheets));
        // A collapsed dock opens to show the panel.
        l.collapsed[1] = true;
        assert!(!l.is_shown(Panel::Sheets));
        l.show(Panel::Sheets);
        assert!(!l.collapsed[1]);
        // The last tab out leaves an empty dock with no front tab.
        l.move_to(Panel::Layers, Place::Dock(Side::Right), Some(0));
        assert!(l.dock(Side::Left).is_empty() && l.active[0].is_none());
        assert_eq!(l.dock(Side::Right)[0], Panel::Layers);
    }

    #[test]
    fn tabs_reorder_within_a_dock() {
        let mut l = Layout::default();
        // Inspector, Parameters, Layers, Sheets, Blocks: drop Inspector before Sheets.
        l.move_to(Panel::Inspector, Place::Dock(Side::Right), Some(3));
        assert_eq!(
            l.dock(Side::Right),
            [
                Panel::Parameters,
                Panel::Layers,
                Panel::Inspector,
                Panel::Sheets,
                Panel::Blocks
            ]
        );
        l.move_to(Panel::Blocks, Place::Dock(Side::Right), Some(0));
        assert_eq!(l.dock(Side::Right)[0], Panel::Blocks);
    }

    #[test]
    fn layout_survives_a_round_trip() {
        let mut l = Layout::default();
        l.move_to(Panel::Layers, Place::Dock(Side::Left), None);
        l.move_to(Panel::Blocks, Place::Float, None);
        l.move_to(Panel::Sheets, Place::Hidden, None);
        l.collapsed[0] = true;
        l.active[1] = Some(Panel::Parameters);
        let prefs = l.prefs();
        let pairs: Vec<(&str, &str)> = prefs.iter().filter_map(|s| s.split_once('=')).collect();
        let mut back = Layout::default();
        back.apply(&pairs);
        assert_eq!(back, l);
        // Old preferences without dock keys leave the default alone.
        let mut d = Layout::default();
        d.apply(&[("grid", "1")]);
        assert_eq!(d, Layout::default());
    }
}
