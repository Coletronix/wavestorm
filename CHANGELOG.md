# Changelog

All notable changes to WAVEStorm (called StormWAVES in 0.1.0) are recorded here. Versions follow
[Semantic Versioning](https://semver.org/): the version lives once in the workspace
`Cargo.toml` and is shown in the About dialog and on printed pages. Each release is a git tag
`vX.Y.Z` on `main`.

## [Unreleased]

### Changed
- Tc flow path editor:
  - Each leg can take a slope. Leave it blank where the slope stays the same as the leg above.
  - A new segment starts only where the flow type or the slope changes.
  - Only segment ends are lettered on the diagram; points inside a segment show a small dot.
  - The path stays on the diagram while the editor is open, with its letters updating live.
  - Segment slopes come from the legs.
  - Each leg is coloured by its flow type: sheet green, shallow orange, channel blue, pipe
    violet, lake teal. The editor rows have matching colour swatches.
  - The path's point markers and arrowhead now scale with the diagram zoom. Their letters
    keep a constant size.
- Node names under the diagram symbols keep a constant, readable size at every zoom. They no
  longer shrink when zoomed out.

## [0.4.0] - 2026-10-23

### Added
- Tc flow paths are lettered A, B, C… on the diagram, and the segments measured from a path
  are described "A to B", "B to D", and so on. When sheet flow ends partway along a leg, a new
  point is added there, so the letters always match the segments.
- **Recent projects** under Project > Open… and on the start page, in the browser too (kept
  as copies in browser storage). Projects whose files have moved or been deleted are reported
  and removed from the list.
- **Reopen the last session:** the projects open when the app last closed are reopened
  (saved files from disk; unsaved changes and browser projects from local storage). If none
  were open, or none can be found, the app starts with no project open and shows a start page
  (New, Open, Open Sample Project, recent projects).

### Changed
- Closing the last project leaves nothing open instead of creating an empty one.
- Opening a file that is already open switches to its tab.

### Fixed
- Deleting the Tc segments measured from a drawn flow path now removes the path from the
  subcatchment and the diagram (once none of its segments is left). "Remove Drawn Path" on the
  Tc tab removes it while keeping the segments.

## [0.3.0] - 2026-10-23

### Added
- **Tc flow path drawing:** select a subcatchment and click its longest flow path on the
  diagram. Use the new Tc Flow Path button beside Measure (greyed out until one subcatchment is
  selected), Draw Flow Path on Diagram on the Tc tab, or the right-click menu. Then choose the
  flow type of each leg. The Tc segment lengths come from the path (calibrated background
  scale), consecutive legs of one type merge, and sheet flow beyond 100 ft (adjustable)
  continues as shallow concentrated flow. Slope and surface are still entered per segment.
  The path is saved with the subcatchment and shown while it is selected.
- **Unsaved-work recovery:** a moment after every change, projects with unsaved changes are
  kept in local storage — browser storage on the web, the settings folder on the desktop. If
  the page or program closes before they are saved or exported, they reopen next time.
- **Desktop autosave** (Settings > General): save changed projects to their own files every N
  minutes. Off by default.

### Fixed
- Routing arrows are easier to grab: the whole arrowhead is the target, not just its tip, and
  the press point is used rather than where the pointer had moved by the time the drag began.
  The pointer shows a grab hand over arrowheads.

## [0.2.2] - 2026-10-23

### Fixed
- The app no longer redraws continuously: the window title was re-sent every frame, which
  requested another frame each time. It now goes idle when nothing changes, and the desktop and
  web builds keep one frame queued instead of two, so the UI keeps up with the mouse.
- Iridescent Glass: circular icons (Help, zoom lenses, settings knobs) and pill shapes no
  longer show a torn edge on their right side; an icon's shadow no longer falls inside its
  glass body.

## [0.2.1] - 2026-10-23

### Changed
- Iridescent Glass: nodes are cut like glass gems — deeper extrusion, a chamfer lit from the
  top-left and shaded on the lower right, a recessed top face — and drawn as one mesh, so the
  link banner no longer shows faint vertical seams. Routing lines are shaded glass tubes
  (dark edges, highlight, cast shadow) with faceted pyramid arrowheads.

### Fixed
- Iridescent Glass: the Help icon's question-mark dot no longer runs into its hook, and the
  Land-Use leaf is evenly filled.

## [0.2.0] - 2026-10-23

### Changed
- Renamed the application **WAVEStorm** everywhere: window title, About box, printed footers,
  file headers, crates (`wavestorm-core`, `wavestorm-ui`), binaries (`wavestorm`,
  `wavestorm-web`), the `WAVESTORM_RAIN_LIB` variable and the saved-settings keys. Settings saved
  by 0.1.0 under the old name are not carried over.
- New logo, drawn with a transparent background and a lightened variant for dark themes.
- Modern Dark theme: lighter borders, disabled text, routing colours, chart series and error
  text for at least 3:1 (graphics) / 4.5:1 (text) contrast; toolbar glyph inks are lifted on
  dark faces; the Messages window, chart legend, peak tags, profile and Chamber Wizard
  sketches no longer use fixed white/black.

### Added
- **Iridescent Glass** theme (View > Theme, Settings > General): a dark theme where everything
  has a little depth. Panels, buttons, tiles and windows are raised glass slabs with visible
  thickness, rainbow lower edges, bevels, glints and iridescent rims. Fields, tables, charts
  and the diagram are sunken wells. Checks, radios and status lights are glass beads, dividers
  are engraved, nodes are extruded glass blocks with embossed labels, and routing arrows,
  chart lines and the new icon set are lit glass tubes. All of this sits over an aurora
  backdrop. It also has a modernised layout: one header bar with the logo, menus, project
  tabs, event selector and undo/redo; glass node tiles and a settings rail; a floating tool
  dock on the diagram with a zoom readout; overlay scrollbars; and a status line with a
  messages chip. The other themes keep their layout.

## [0.1.0] - 2026-10-23

First tagged release.

### Hydrology
- SCS/NRCS runoff with the curvilinear unit hydrograph (TR-20), SBUH and the Rational method,
  calibrated against published reference results (see `AGENTS.md` §3–4).
- NRCS Type I, IA, II and III storms (24-, 12- and 6-hour), custom, Chicago and back-to-back
  storms, AMC adjustment; gamma unit hydrographs for any peak factor.
- Composite curve numbers from the TR-55 tables, including unconnected impervious areas.
- Time of concentration from sheet, shallow-concentrated, channel, lag/CN, Kirpich, Kerby and
  lake segments; TR-55 graphical peak discharge worksheet and detention storage estimate.

### Hydraulics and routing
- Reaches (rectangular, trapezoidal, parabolic, pipe, surveyed, rating table) routed by
  Stor-Ind, Stor-Ind+Trans, Muskingum-Cunge or Dyn-Stor-Ind.
- Ponds with stage-area/stage-storage data, prismatoids, cones, pipes and arch chambers, stone
  voids, embedded volumes and a Chamber Wizard.
- Outlets: culverts, orifices and grates, broad- and sharp-crested and custom weirs,
  exfiltration, pumps, siphons, dam breach, delayed gates (with vertical range and rotary
  weir), channel outlets and rating tables; compound devices; fixed, differential and dynamic
  tailwater.
- Automatic sizing of round pipe reaches and culverts to standard diameters (Resize tab).
- Calculation messages from the reference catalogue, including storage gaps, submerged and
  backed-up outlets, oscillation and Qout>Qin warnings. Ponds that overtop their defined
  storage report undefined results instead of extrapolating.

### Reports and output
- Per-node Summary, Hydrograph (2-D/3-D), Discharge, Storage, Events, Sizing and Profile views;
  comparison and project reports; PDF printing, EMF vector export, CSV and text exports;
  pollutant loading by land use.

### Data exchange
- Native `.waves` projects (JSON); open/save of the reference application's `.hcp` project
  files (English units), TR-20 deck import, rainfall library (`.hcr`, `.huh`, `.idf`
  (provisional), county rainfall tables, NOAA Atlas 14 CSV).

### Application
- Desktop (Linux, Windows, macOS) and browser (WebAssembly) builds from the same Rust code.
- Routing diagram with drag-and-drop, project tabs, undo history, pop-up field tips, three
  themes, and a calibrated background image/PDF underlay for tracing subcatchments.
- StormWAVES name and vector logo.

### Known limitations
- Rock-fill outlet device not implemented (needs a verified public equation).
- Metric `.hcp` files are not read yet; the `.idf` layout is unverified.
- Stor-Ind+Trans reach attenuation can differ from the reference application (~2 % vs ~4 % on
  one published example).

[Unreleased]: https://github.com/Coletronix/wavestorm/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/Coletronix/wavestorm/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/Coletronix/wavestorm/compare/v0.2.2...v0.3.0
[0.2.2]: https://github.com/Coletronix/wavestorm/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/Coletronix/wavestorm/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/Coletronix/wavestorm/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Coletronix/wavestorm/releases/tag/v0.1.0
