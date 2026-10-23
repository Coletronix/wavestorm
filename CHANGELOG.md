# Changelog

All notable changes to StormWAVES are recorded here. Versions follow
[Semantic Versioning](https://semver.org/): the version lives once in the workspace
`Cargo.toml` and is shown in the About dialog and on printed pages. Each release is a git tag
`vX.Y.Z` on `main`.

## [Unreleased]

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

[Unreleased]: https://github.com/Coletronix/stormwaves/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Coletronix/stormwaves/releases/tag/v0.1.0
