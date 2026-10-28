# One model for several open drawings

PLAN.md §11 step 2: decide between the per-window sessions built in #220 (#217) and the tabs
PLAN.md plans, before anyone builds tabs. Written 2026-10-28 on main e4ac025. No code was
changed for it.

## Recommendation (short)

**Tabs in one process, built after the pilot, with a second window still possible; fix macOS
file opening first.** The per-window sessions from #217 stay: they become the window's list of
tabs. One process per drawing costs about 650 MB per extra drawing and
barely works on macOS; tabs are what AutoCAD, Illustrator and Affinity users expect. Floating
a tab out is a second process for now. The refactor splits state into app, drawing and
*view*, so a later split view, a second window on the same drawing and editing on sheets
(drift-releases#16) need no second refactor. Fable reached the same shape independently;
where we differed, its version is taken (below).

## What exists today

- **One drawing per window, one window per process.** `apps/native/src/main.rs` runs one
  eframe viewport; `DriftApp` (`crates/drift-app/src/lib.rs:249`) holds about 150 fields with
  the drawing's state (editor, file, camera, picks, sheet preview, caches) mixed in with the
  app's (preferences, layout, fonts, tool, palette). `self.editor` is used about 1,090 times.
  There is no File → New window; a second window appears only when Drift is started again
  (a double-clicked `.drift` file, the Start menu) or when **Drawings left open → Open them**
  spawns one (`app/session.rs:452`).
- **Sessions (#217).** Each window autosaves its one drawing to `sessions/<id>.session`,
  atomically and only when something changed, and holds an OS lock on `<id>.lock` for life. A
  new window takes over the newest session left behind; one opened for a file takes over that
  file's session; other dirty ones are offered. The updater refuses while another window holds
  a lock, because the installer can't close it.
- **AI link.** Each window runs its own localhost link server and marks its link file as used
  while it has the focus (`app/link_ui.rs:54`); `drift-mcp` connects to the most recently used
  window (`agent/link.rs:418`) and stays on it until it reconnects.
- **Clipboard.** Copy puts a small Drift document on the system clipboard, blocks included and
  matched by name on paste (`clipboard.rs`), so copy and paste already work between windows.
- **Undo, sheets and views** belong to the document (`Editor` owns the history; sheets and
  views are in the file), so they are per drawing in every option. Nothing in a drawing refers
  to another drawing.
- **Web demo:** one canvas, one drawing, its session in `localStorage`.

## What each option costs

| | Windows (today) | Tabs in one window |
|---|---|---|
| Memory per extra drawing | a whole process: about 400 MB idle plus 250–300 MB of GPU memory (PLAN, soak test), plus the drawing | the drawing, its undo history and (if kept) its canvas caches; one wgpu device and one surface for all |
| GPU / surfaces | one device and surface per process | one; a floated-out tab would add a surface on the same device (egui viewports, not used in Drift yet) |
| Windows | works: each double-click starts a process and gets a taskbar button | Open adds a tab; a double-click while Drift runs needs the file passed to the running process (a single-instance hand-off) or it opens a second process as now |
| macOS | **barely works**: Finder brings the running app forward instead of starting a second one, winit 0.30 doesn't hand Drift the opened file, and `Info.plist` declares no document types. In practice a Mac user opens a second drawing by replacing the first. (From the platform docs; not tried on a Mac.) | the Mac convention (one app, several documents); the Finder-open event still needs native glue |
| Web demo | one drawing | stays one drawing (tabs turned off: `localStorage` is a few MB) |
| Autosave / recovery | built and tested (#217) | the window's session holds a list of drawings and which is in front; takeover and the offer work per window as now. Write each drawing only when it changed, so a big PDF isn't re-serialised for an edit elsewhere |
| Memory guard (8 GB) | ends one window | ends every tab of the window at once; the session brings them all back |
| AI link | follows the focused window | must say which drawing: the tab in front, with a refusal if the user switched tabs mid-job, or a drawing it is bound to |
| Copy / paste, blocks | system clipboard, works | the same; later, the Blocks panel could offer blocks from other open drawings |
| Updater | blocked while other windows are open | one process: the problem goes away for users who use tabs |
| Build cost | nothing more | about 2.5–3 weeks with tests (below) |

**Building tabs.** Move the per-drawing fields of `DriftApp` into a `Drawing` struct, and the
per-view ones into a `View` it holds, so the compiler finds every use, rather than swapping fields one by one as PLAN.md's sketch does (a
field left behind would carry a pick or a cached page from one drawing into another). Then: a
tab bar, New and Open making a tab (an untouched Untitled one reused), close-tab and quit
prompts per unsaved tab, Ctrl+Tab, the session as a list, the link's choice of drawing, and
tests (switching keeps picks, undo and camera apart; restore of several; link refusal). My first
guess was two weeks; Fable's 2.5–3 weeks with tests is the better figure. Switching to a
parked huge drawing either keeps its canvas caches (memory) or rebuilds them (about a second on the 73k-curve PDF).

## What the target users expect

AutoCAD and AutoCAD LT have file tabs in one window (Ctrl+Tab); a drawing can't float out, but
a second AutoCAD can be started. Illustrator and Affinity have document tabs that drag out
into floating windows, all in one process. SolidWorks shows each document in its own window
inside one application frame. Canvas X Draw has tabs too, as far as I know (not checked). None of them runs
one process per drawing. An AutoCAD LT drafter keeps a few drawings open (a detail sheet, the
plan it comes from, a block source) and copies between them; an Illustrator user expects Open
to add a tab. Two monitors are the one case where separate windows win, and there two needs
must be kept apart: two *files* side by side (a second process does it) and two *views of one
file*, such as the model on one screen and its sheet on the other. Revit users work that way,
and SolidWorks, Illustrator and Affinity offer Window → New Window on the same document. That
needs a second window in the same process, since two processes can't share a drawing or its
undo history; it is filed as drift-releases#16, after tabs.

## Side by side: mine and Fable's

Fable was given the code pointers and the question, not this draft.

| Point | Mine | Fable's | Taken |
|---|---|---|---|
| Overall | tabs in one process, a second window still allowed | the same: "(c), staged", AutoCAD's model | agreed |
| When | after paper cuts and the canvas-first batch, if the pilot asks | after the pilot; nothing new now | agreed |
| macOS | a gap to note | **the first item, for either model**: no document types in `Info.plist`, and the file only comes from `argv[1]` (`main.rs:57`), so a Finder double-click while Drift runs does nothing | Fable's: it's a bug fix and blocks both |
| Splitting `DriftApp` | one `Drawing` struct instead of PLAN's field swap | the same, with `Deref` (or a method) so the 1,087 `self.editor` uses stay; the hard part is sorting the other fields, not the editor | Fable's detail |
| The tool | (not covered) | PLAN is wrong that the tool is shared: it holds mid-gesture state, so a switch must cancel it, as `replace_document` (`document.rs:147`) does | Fable's |
| Leaks | a forgotten field leaks on switch | `replace_document` already resets only the tool, layer isolation and fit by hand; the same leak surface exists on Open today | Fable's; worth checking as a possible paper cut |
| Floating out | egui viewports later, if asked | at first: never as egui viewports (whole app re-runs per viewport, macOS focus/IME quirks); drag a tab out = a new process. Asked again about the model and its sheet on two monitors: those are real costs, not disqualifiers, and a same-process viewport is the only way | a tab dragged out = a process; the same drawing in a second window = a viewport, later (#16) |
| State levels | one `Drawing` | (second review) one bucket is a level too coarse: `Drawing { editor, file, caches, views }` with `View { camera, sheet shown, view drags, hover }`; selection and undo per drawing, the tool and gestures with the view that owns the pointer | Fable's, built into step 2 |
| Order after tabs | — | (second review) split view before tabs, since it gives most of the value | tabs first (the user's call, 2026-10-28); the `View` split keeps #16 cheap |
| AI link | the tab in front, refusing if the user switched tabs mid-job | the tab in front of the focused window; touch the link on tab switch | open: see decisions |
| Parked drawings | keep or rebuild canvas caches | measure a parked 73k-curve PDF; over ~300 MB, unload parked tabs to the session file | Fable's: measure first |
| Cost | about 2 weeks | about 1 week split + 1.5–2 weeks tabs, ~10 new smoke tests, the five #217 smoke tests rewritten | Fable's, as the more honest figure |
| Blocks | later, from other open drawings | the same, and the real payoff for millwork users with a house block set | agreed |

## Recommendation and staged path

1. **Now, during the pilot (1–2 days, a bug fix):** macOS opens files: declare `.drift` in
   `Info.plist` (document type and UTI) and handle the open-file event, opening into the
   running window if its drawing is clean and untouched, else starting a window. Nothing else.
2. **After the pilot (about a week):** move the per-drawing state of `DriftApp` into a
   `Drawing` struct and the per-view state (camera, fit target, pan, the sheet shown and its
   preview, view drags and crops, hover) into a `View` inside it, starting with one view.
   `self.editor` keeps working through `Deref` or a method, and canvas code takes a (drawing,
   view) pair. No change users can see; the smoke suite is the safety net. Measure a parked
   big drawing's memory.
3. **Tabs (1.5–2 weeks):** `Vec<Drawing>` and the one in front; tab bar and Ctrl+Tab; New and
   Open make tabs; close-tab and quit prompts per unsaved tab; switching cancels the tool; the
   window's session becomes a list (one lock per process, each drawing written only when it
   changed); the AI link names the drawing in front. Keep **File → New window** (a process, as
   `open_window()` already does) for two monitors, and let a tab dragged out of the bar open
   there. The web demo stays one drawing (`localStorage`).
4. **Later (drift-releases#16):** a split view in one window, a second window on the same
   drawing (an immediate egui viewport drawing only the canvas), and editing dimensions and
   notes through a view on a sheet. Each needs per-view canvas buffers and a measurement of
   the sheet rebuild on big drawings first.
5. **Later, if asked:** blocks from other open drawings in the Blocks panel; unloading parked
   tabs; a double-click on Windows going to the running Drift as a tab (a single-instance
   hand-off) instead of a new window.

## What would change the call

- **Pilot users rarely have two drawings open:** stop at step 1 and leave tabs unbuilt.
- **They mostly want two drawings side by side** (a reference detail beside the working
  sheet): separate windows are the right tool; add File → New window and spend the tab time
  on picking blocks from another file instead.
- **A parked big drawing costs more than about 300 MB:** unload parked tabs before shipping.
- **Several Mac users in the pilot:** step 1 moves to the front of the paper cuts.
- **Pilot users ask for the model and its sheet on two screens, or for editing on sheets**
  (likely from Revit users): #16 moves up, split view first.

## Decisions for the user

1. **The model — decided 2026-10-28:** tabs first, in one process; several views of one
   drawing and sheet editing later (drift-releases#16).
2. **When:** the macOS fix now as a paper cut, and steps 2–3 after the pilot (recommended),
   or schedule tabs sooner.
3. **A file double-clicked while Drift runs (Windows):** a new window as now (recommended
   for the first tab release), or a tab in the running window (needs a hand-off between
   processes).
4. **The AI link when the user switches tabs:** follow the tab in front (Fable; simplest), or
   stay on the drawing it started on and refuse until the assistant reconnects (mine; safer
   for a long job).
5. **Floating out:** a tab dragged out becomes its own process (recommended, since a different
   file needs nothing shared), or the same in-process window #16 uses for the same drawing.
