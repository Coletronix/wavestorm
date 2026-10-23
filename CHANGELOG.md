# Changelog

All notable changes to WAVEStorm (called StormWAVES in 0.1.0) are recorded here. Versions follow
[Semantic Versioning](https://semver.org/): the version lives once in the workspace
`Cargo.toml` and is shown in the About dialog and on printed pages. Each release is a git tag
`vX.Y.Z` on `main`.

## [Unreleased]

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

[Unreleased]: https://github.com/Coletronix/stormwaves/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/Coletronix/stormwaves/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/Coletronix/stormwaves/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Coletronix/stormwaves/releases/tag/v0.1.0
