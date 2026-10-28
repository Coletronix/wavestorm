# AGENTS.md — guide for AI agents and contributors

WAVEStorm is a clean-room, open-source stormwater modelling application (TR-55 / TR-20 hydrology,
reach and pond routing) written in **Rust only**. It runs natively on Linux, Windows and macOS
(egui/eframe) and in the browser (the same Rust compiled to WebAssembly with trunk). The goal is
close behavioural and visual parity with the established commercial routing-diagram stormwater
package that engineers already know ("the reference application"), plus extras such as tracing
subcatchments over a scaled background image or PDF.

Read this file first. Keep it current: it is the hand-off between sessions.

## 1. Hard rules

1. **Never name the reference application** anywhere in the repository: code, comments, docs,
   file names, test names, commit messages. Say "the reference application". A committed hook in
   `.githooks/` (enabled with `git config core.hooksPath .githooks`) rejects such commits.
2. **Clean-room, no copied assets.** Never copy the reference application's icons, images,
   sample projects, file formats, help text or proprietary data tables. All UI art is drawn in
   code. Engineering data must come from public sources (NRCS TR-55, NEH Part 630, WinTR-55
   tables, FHWA HDS-5, published handbooks) and say so in a comment. Test oracles may use numbers
   published in the reference application's public sample reports, under neutral test names.
   Exception (user decision, 2026-10-23): for interoperability we read and write the
   reference application's project (`.hcp`), rainfall (`.hcr`), unit-hydrograph (`.huh`),
   IDF (`.hci`) and county rainfall-table (`*-Rain.txt`) text formats. Their data files are
   loaded at runtime from files the user supplies; never commit or bundle them. Tests use small
   fixtures written by us, or the gitignored samples in `reference_images/expert_reference/`
   and skip when those are absent. The `.hcp` header section carries the product name: build
   that string at runtime from two pieces (`concat!`-style splitting) so the hook never sees it.
3. **Never panic** on user input or file data. No `unwrap`/`expect`/`panic!`/indexing on
   untrusted values in non-test code (`#![deny(clippy::unwrap_used, …)]` in each crate). Invalid
   input produces a calculation message, never a crash.
4. **The core never depends on the UI** and must compile for `wasm32-unknown-unknown`.
5. **Tests are the gate.** Every algorithm ships with tests; reproduce published reference
   results where they exist (see §4). Run `cargo fmt` and `cargo test` before every commit.
6. Commit each major feature. Commit messages end with the co-author line the session asks for.
7. **Edit files with the editor tools** (Edit/Write), never with Python or sed scripts, so every
   change is a reviewable diff. Scripts are fine for analysis in the gitignored research folder.

## 2. Workspace

```text
crates/core   wavestorm-core: hydrology & hydraulics engine (no UI, wasm-clean)
crates/ui     wavestorm-ui:   egui application shell
apps/wavestorm       desktop binary
apps/wavestorm-web   browser build via trunk
reference_images/     gitignored: screenshots and research material (never commit)
```

Research material lives in `reference_images/research/` (gitignored): `NOTES.md` (golden test
cases, equations, UI layout facts, menus, calculation-message catalog) and `web/` (crawled public
documentation, sample-report PDFs, TR-55 and NEH-16 text, UI screenshots). Read `NOTES.md`
before UI or parity work. It may contain the reference application's name; that is why it is
gitignored.

### Core modules (`crates/core/src`)

| Module | Contents |
|---|---|
| `hydrograph` | `Hydrograph` (t0, dt, cfs), parabolic peak interpolation (TR-20 convention), volume, centroid |
| `rainfall`, `rainfall_data` | NRCS Type I/IA/II/III 24-hr mass curves (WinTR-55 v2, smoothed by monotone cubic), 6/12-hr trims, custom/Chicago storms, events, AMC, back-to-back storms |
| `rainlib` | Rainfall library files: `.hcr`/`.huh`/`.hci` (IDF) parsers and writers, `*-Rain.txt` county tables, storm-code mapping |
| `uh` | NRCS dimensionless UH (NEH 630 Table 16-1), gamma UHs for any peak factor |
| `runoff` | SCS runoff equation, TR-20 unit-hydrograph runoff, SBUH, Rational |
| `cn` | TR-55 Table 2-2 lookup, composite CN, unconnected-impervious adjustment |
| `tc` | Tc segments: sheet, McCuen-Spiess, shallow concentrated, channel, lag/CN, Kirpich, Kerby, lake, direct |
| `geometry` | Channel and pipe sections, Manning (k = 1.486), normal depth, custom sections |
| `storage` | Pond storage volumes, voids, multipliers, embedding, surface and wetted area |
| `outlets` | Culverts, orifices/grates, weirs, exfiltration, special, constant-flow, pump; compound device routing |
| `routing` | Level-pool solver (ponds and reaches), translation, Muskingum-Cunge, detention times |
| `link` | Link nodes: manual/CSV hydrographs, thresholds/limits/multipliers/lag/slices, tailwater elevation |
| `units` | English/metric, large/small display units, formatting |
| `project` | Project model: nodes, outflows, settings, diagram, background underlay; JSON I/O |
| `calc` | Calculation engine: topological order, per-node results, calculation messages |
| `hcp` | Open/save the reference application's `.hcp` interchange files (Latin-1 aware, unmatched keys kept in `foreign` fields and written back) |
| `base64` | Embedding background files in project JSON |
| `geo` | Background georeference (pixel ↔ lat/lon on a tangent plane), two-point solve, map image / address request URLs |
| `soils` | NRCS Soil Data Access query/parse (WKT), soil map on the diagram, polygon overlap areas, splitting sub-areas by HSG, HSG report |
| `shapefile` | One-polygon zipped shapefile (AOI export for Web Soil Survey) |

Internal units are US customary: ft, sq ft, cu ft, cfs, hours, inches of rain. Conversion to
display units (English/metric, large/small units) happens only at the UI/report edge.

## 3. Methods confirmed against published reference results

These choices were reverse-engineered from public sample reports and are covered by tests. Do
not change them without re-running the golden tests.

- **SCS runoff:** NRCS curvilinear UH, unit duration ΔD = 0.133 Tc so Tp = (0.0665 + 0.6)·Tc.
  Excess is convolved on an internal step df = dt / ceil(dt/ΔD), sampling the UH at
  τ = (k+1)·df − 0.09·Tc after each increment began (`UH_DELAY_PER_TC`, a calibrated delay),
  kernel normalised to exact volume; results sampled at dt. Against 18 current-version
  published subcatchments: volumes exact, peaks within ~1.5 % and times within 0.02 h for
  Tc ≥ 10 min; published two-subcatchment link sums within 1 %.
- **Tc** = sum of segment times each rounded to 0.1 min (as printed).
- **Custom stage-area storage:** cumulative storage at the entered stages, linear between them.
- **Culvert:** min(barrel flow with D = min(¾H, rise) and Ke + friction losses, inlet control as
  an orifice with Cd = Cc/(1+Ke) on the head to the centroid).
- **Orifice:** head to centroid when submerged. **Broad-crested weir:** Brater & King table,
  bilinear in head and breadth.
- **Compound outlets:** a device fed by others passes min(sum of feeders, own potential).
- **Stor-Ind+Trans reach:** level-pool routing with S = A(y)·L and O = Manning Q(y), then the
  outflow is translated by L / Vmax (Vmax at the routed peak).
- **Peaks** are interpolated by a parabola through the three highest samples.

## 4. Golden tests

`runoff::tests::golden_*`, `tc::tests::reference_*`, `storage::tests::reference_*`,
`outlets::tests::reference_*`, `routing::tests::reference_*`. The numbers come from the public
sample reports summarised in `reference_images/research/NOTES.md` (a 2003 version).

Current-version oracles from public engineering submittals (texts and parsed `cases.json` in
gitignored `reference_images/research/ext*/`):
- `tests/published_subcatchments.rs`: 74 subcatchments (Type II/III/IA, Tc 4.8-90.6 min) and
  four two-subcatchment link sums.
- `tests/published_ponds.rs`: four ponds x two storms (stage-area storage with gravel voids,
  starting elevations, culverts fed by orifices and perimeter-limited grates, broad-crested
  weirs, conductivity exfiltration). Inflow Tc values were back-fitted to the published inflow
  peaks (one Tc fits both storms within 0.2 % each).
- `tr20::tests` uses a public TR-20 training deck; `noaa::tests` a NOAA Atlas 14 CSV.

## 5. Status and next steps

Keep this section updated at the end of every session.

- Done: core hydrology (runoff, Tc, CN, rainfall, UH), hydraulics (sections, storage, all
  outlet devices), routing (level pool, Stor-Ind+Trans, Muskingum-Cunge, Dyn-Stor-Ind, detention
  times, pond sizing), links, units, project model (JSON `*.waves`), calculation engine with
  messages, reports, PDF printing, land-use loading, CSV import/export/merge, full egui UI
  (menus, palette, diagram, dialogs, charts, help), background image/PDF underlay with scale
  calibration and subcatchment tracing, desktop and web (trunk) apps.
- Rainfall library (`core::rainlib`, `ui::library`, `dialogs/lookup.rs`): reads the reference
  application's `.hcr` storms (coef polynomial segments, `coef1` IDF/Chicago, depth/inten/incr,
  timedepth/timeincr, `curve=` multi-storm files, units, inline expressions), `.huh` unit
  hydrographs (peak ordinate = t/Tp 1), `*-Rain.txt` county tables (quoted CSV, several storm codes
  per row, `storm_for_code` maps "NOAA_C", "MSE3", "N10_D"... to library storm names) and
  IDF files. Resolved 2026-10-28 (user request): the reference application's IDF files are
  `.hci`, NOT `.idf`; its public IDF.txt spec and ~19 public sample `.hci` files (gitignored
  copies in `reference_images/research/idf/`, read by
  `rainlib::tests::published_hci_samples_when_present`) define the layout `rainlib::parse_idf`
  follows: NAME/COMMENT/DEPTHUNITS(INCHES|MM)/TIMEUNITS (intensity time base, default
  HOURS)/DURATUNITS (default MINUTES), one DURATION line (expressions like `24*60`), events
  EVENT/Cf/INTEN (`2x` = first INTEN line × 2)/DEPTH/coef1/coef2/coef3; coefficient events
  are sampled (64 log points) over the DURATION range and held beyond it, as the spec says.
  Real files contain `event==25-Year`, tabs and trailing spaces. A NOAA PFDS CSV saved as
  `.hci` is also an IDF file (current layout and the pre-2012 transposed one,
  `noaa::parse_pfds`). Our earlier provisional `.idf` layout is still read
  (`parse_legacy_idf`); `write_idf` writes `.hci`. NOAA PFDS for a point:
  `noaa::pfds_url` (`cgi-bin/new/fe_text_mean.csv`; no CORS, desktop only), "NOAA for Site" in
  Event Lookup (`Fetch::Noaa`, centre of the georeferenced background). Writers:
  `write_hcr/huh/idf`. Library folder pref (Settings > General, desktop),
  "Import Rainfall Files..." (desktop and web; web imports persist in localStorage up to 2.5 MB).
  Library storms/UHs used by events, the default UH or a subcatchment UH are copied into the
  project (`library::adopt`) so projects stay self-contained. The full-library test runs when
  the folder exists (`WAVESTORM_RAIN_LIB` or `reference_images/expert_reference/Rainfall`);
  never copy or commit those files.
- Calculation messages emitted (`calc.rs`): 13 16 19 22 31 37 40 42 43 44 45 46 47 49 55 58 67 70 73 76 77 78 80 82 85 86 89 91 92 93 95 96 97 98 (plus errors 500 520 550 560 595).
- Expert review pass (2026-10-23), all nine items addressed:
  1. 3-D charts redrawn as solid ribbons (no fill/hatch), first series in front, summary text
     placed clear of the curves (`chart.rs`).
  2. A pond that rises above the top of its defined storage is an Error (93) and its results
     are `NodeResult.undefined` (reports print UNDEFINED); everything downstream is undefined
     too (Error 95). The diagram rings such nodes in red.
  3. County rainfall lookup tables (`*-Rain.txt`) in Rainfall Event Lookup (see library below).
  4. Reach Summary draws the cross-section with the peak water level (`ui::section_view`).
  5. Events tabs recompute on demand whenever results change, so no Update button is needed.
  6. After every calculation the status bar summarises the messages and the Messages window
     opens when new hints/warnings/errors appear (`announce_calculation`, General pref).
  7. EMF export of node graphs/summaries and the diagram (`core::emf`, from the `pdf::Page`
     display list).
  7b. The printed/EMF diagram matches the screen: background raster (`PrintOptions.background`,
     decoded by `ui::underlay::print_background`, ≤1600 px, opacity pre-applied), translucent
     outline/sub-area fills (`Page::fill_alpha`: PDF ExtGState `/ca`; EMF clip path +
     EMR_ALPHABLEND with a premultiplied 32-bit pixel, which LibreOffice honours and Inkscape
     ignores), every subcatchment's Tc path, and CN labels for drawn sub-areas and the
     remainder (`drawing::place_label`: grid search for the most clearance, avoiding node
     symbols and Tc paths when the label still fits; same placement on screen). Printed labels
     are always shown, shrunk (down to 2 pt) to fit their spot; the screen omits ones that do
     not fit at its fixed size. Chart peak
     boxes are placed clear of each other and of the summary lines.
  8. `.hcp` open/save (`core::hcp`): unmodelled keys are kept losslessly in `foreign` fields.
     On the user's example, reach 2R matches the published inflow within 0.4 % and the flood
     depth line exactly, but our Stor-Ind+Trans attenuates 2 % where the reference shows 4 %
     (outflow +1.8 %), and our "Avg. Velocity" (2.3 fps) differs from the reference (1.42 fps,
     5.3 min avg travel). Open validation item: reach routing/average velocity definition.
     Findings (2026-10-23): sub-stepping the level-pool routing (1-30 sub-steps) converges to
     20.37 cfs, so step size is not the cause; Manning at the reference's own depth at peak
     storage (1.41') is 20.4 cfs, so the reference's 19.99 comes from its translation/peak
     interpolation or its earlier, sharper inflow (12.23 h vs our 12.26 h, the known UH timing
     offset). Resolved: "Avg. Velocity" = velocity at the mean outflow over the steps
     carrying flow (> 1e-6 cfs); reproduces 2R's 1.42 fps / 5.3 min exactly and the ceqa1
     reaches 3R/6R/7R within ~2 % (`calc::route_reach`, asserted in `hcp::tests`). Velocities
     print with 2 decimals as in the reference. More oracles: gitignored
     `research/ext2/ceqa1.txt` (reach chain 3R→6R→7R is fed through other nodes from WS-4A;
     rebuilding it needs the pond/link between them).
  9. `.hcr`/`.huh`/`.idf` and county tables via the rainfall library.
  Also: Project Reports (`core::listings`), Chamber Wizard (`core::chamber`, generic
  user-editable arch chambers only), TR-55 graphical worksheet tab on subcatchments
  (`core::tr55`, TR-55 examples 4-1/6-1 as tests), reach defined flood depth.
- Remaining parity gaps: rock-fill outlet (needs a verified public equation —
  the reference cites Haan, Barfield & Hayes 1994 pp. 151-155). Pop-up tips
  (`widgets::with_tip`, General setting) exist on node General, reach, pond Advanced and Tc
  segment fields, plus generic label-keyed tips on every numeric field (`dialogs::field_tip`);
  the areas/CN grid columns carry theirs via `widgets::Column::tip`.
- Done later in the session: custom decimal places (Settings > Units), status-bar hints for
  toolbar/palette, "Asymmetrical / Custom Weir" (our crest-profile weir).
- Also done this session: pump max elevation + priority groups; node Profile tab
  (`core::profile`, drawn by `reports::profile_view`) on subcat/reach/pond reports and as a
  "Profile" view of the comparison report (nodes chained in selection order).
- Recently done: tabular-hydrograph "Inflows" columns (right-click the table), Settings >
  General (theme, backup interval, multiple Project Reports; saved via `platform::store`),
  headless app (`WaveStormApp::headless()`) for tests and snapshots — never open native
  dialogs there.
- TR-20 import (`core::tr20`, Project > Import): whitespace-token parsing of the legacy card
  format, with two layout rules learned from published decks: COMPUT often fuses duration and
  rain table ("1.05" = 1.0 + table 5), and REACH/RESVOR hydrograph slots are one-column fields
  two columns apart ("1   2" vs "2 3 4"). Reaches get `Section::Rating` (depth–flow–area).
  Rain tables 1–4 → Type I, II, IA, III — only table 2 is confirmed; others are flagged in the
  notes. Test decks: a public TR-20 training manual (UMD GISHydro) and Howard County, MD flood
  studies (PDFs in gitignored `reference_images/research/tr20/`). Volumes match TR-20 output;
  reach-routed peaks differ (storage routing vs Att-Kin).
- Runoff timing (resolved empirically, 2026-10-22): the current reference version's peaks lag
  the textbook convolution by ≈0.09 Tc. `UH_DELAY_PER_TC = 0.09` fits all current-version data
  (`tests/published_subcatchments.rs`) and brings the 2003 network sums within 0.3 %
  (`tests/reference_network.rs`). The 2003 single-hydrograph golden test (dt 0.1) now runs ~2 %
  high / 0.04 h late, so its tolerances were widened; treat 2003 data as a legacy version.
  The mechanism is unknown (it is not a kernel offset, interval averaging or Tp = ΔD + L).
  Fitting scripts: gitignored `reference_images/research/ext/tools/` (first argument = that
  folder; `variants.py`, `jointfit.py`).
- Short-Tc peaks (resolved, 2026-10-22): with the monotone cubic alone, Tc ≤ 10 min peaks ran
  6-12 % low and ~0.04 h early. The reference's own "polynomial" storms are sharper inside the
  peak 0.1 h only (a coarse 1986 TR-55 curve overshoots mid-Tc by 2-6 %, so it is not that).
  Fix: `Storm::tangents` replaces the cubic's tangents at the peak knots without moving any
  table value — Type II (11.8 h 0.75, 11.9 h 1.6, 12.0 h 0.214 /h), Type III (12.0 h 1.3 /h).
  Fitted (`ext/tools/peakfit.py`) to the short-Tc cases now in
  `tests/published_subcatchments.rs` (46 cases from 5 public reports, Tc 4.8-90.6 min; all
  within ~1-2 %). Type IA (7.7 h 0.145, 7.8 h 0.218, 7.9 h 0.211 /h) was fitted to 28
  published cases (peaks +0.4..+2.2 %, times ±0.04 h; its flat peak makes timing noisy).
  Type I has no published data yet and keeps plain smoothing.
- UI parity pass (started 2026-10-22, user priority: "ensure all parts of UI match reference";
  validation work is paused). Reference screenshots: `reference_images/*.gif` and
  `reference_images/research/web/shots/` (s1-s8 are current-version screens). Compare with
  `cargo run -p wavestorm-ui --example snapshot -- --out x.png --do "<script>"`.
  Done: OK/Cancel/Apply/Help row in every dialog (`ok_cancel_apply(ui, apply, help_topic)`),
  white chart background, report button column (Table, 2D/Shrink, Print, Export, Edit, Help;
  `widgets::toggle_button`), min/max/close boxes on report windows (`Win::min_max`), Events
  table in the reference style (`reports::report_table`), comparison report with
  Summary/Hydrograph/Profile tabs and the outflow selector at top right.
  Checklist (all done 2026-10-23 except where noted):
  1. (done) Project Reports: reference tab set, All/Selected Nodes, Show Totals, button
     column; tables from `core::listings` (script `project-report:N`).
  2. (done) Storm / UH / IDF viewers (`reports/curves.rs`; IDF graph on a log duration axis,
     `ChartSpec::log_x`) and the Event Depth(s) option (script `storms-events`). IDF files are
     `.hci` (verified against public samples, 2026-10-28; see the rainfall library entry).
  3. (done) Calculation Settings Rainfall tab. (done) Resize tab: automatic sizing
     (`core::resize`) of round pipe reaches and round culverts marked `auto_size`, smallest
     listed diameter keeping the current event's peak depth ≤ 0.98 D (reach) or pond peak ≤
     culvert crown (outlet crown if reverse slope), or ≤ the flood level with that criterion.
     Bisection over sizes in flow order; `Doc::recalc` writes sizes into the project (message
     59). Behaviour follows the reference's public "pipe sizing" page; its finer options are
     unknown.
  4. (done) Pond Outlets tab: grid, tip text, Edit Outlet... (device-type chooser on an empty
     row), right-click Insert/Delete/Move Up/Down (`widgets::list_table_menu`).
  5. (done) Outlet editors: pump Disch. Length/Diam/Hazen-Williams (`core::outlets::pump_flow`
     adds pipe friction), Load From File for pump/special tables (`Purpose::RatingCsv`),
     Opening-in / Orifice / Grate group boxes, Area/Perimeter under the shape preview.
     Snapshot script: `edit:1P;tab:2;device:new Pump` or `device:2`.
  6. (done) Link edit Manual tab: time x event grid, Interpolate (`link::interpolate_blanks`).
  7. (done) Land-Use Settings tabs; the imperviousness scope drives the loading calculation
     (`landuse::loading_where`). Open: the Report Land-Use / Report Loading flags are stored
     but not yet used by printing.
  8. (partly) Selected-node bold outline, link circle markers, red ring on problem nodes.
     Right toolbar reordered to the reference settings column (Units, Land-Use, Calculation,
     Diagram, General, IDF, Print) 2026-10-23. Open: reference buttons for cost, site/house and a
     disabled back-arrow have no app feature; their exact meaning is a guess.
  9. (done) Chamber Wizard (Storage tab button; script `edit:1P;tab:1;chamber`).
  Next: reach routing attenuation/avg velocity vs the `.hcp` example (item 8 above), metric
  `.hcp` files (need a sample), rock-fill outlet (needs a verified public equation). Done
  2026-10-23: Delayed Gate editor layout with Opening Step, Vertical Range (moving full-size
  inlet whose lip drops from invert + range) and Rotary Weir (inlet tilts horizontal →
  vertical while lowering; from the reference's public batch-detention page —
  horizontal-state weir uses the generic 3.27 grate coefficient, not one derived from Cd),
  land-use and
  loading lines in subcatchment summaries driven by the Report flags.
- Name and logo (renamed 2026-10-23, user request): the product is **WAVEStorm** in all
  user-facing text; crates, binaries, directories, the `WAVESTORM_RAIN_LIB` variable and the
  prefs/localStorage keys use `wavestorm`, the app type is `WaveStormApp`. (It was briefly
  "StormWAVES" in v0.1.0; nothing keeps the old name.) The logo master is the user's raster
  `crates/ui/assets/logo.jpg` (no SVG yet). `ui::logo::cut_out` removes its white background at
  load time (colour-to-alpha only near background white, so pale fills stay opaque); dark
  themes mirror the lightness of dark ink (`lighten_dark`). `cargo run -p wavestorm-ui
  --example logo_png` writes `logo.png`/`logo-dark.png` (640 px) for the README `<picture>` and
  the web loading screen; re-run it after replacing the JPG.
- Themes (2026-10-23, user request). Classic / Modern Light / Modern Dark keep the reference
  layout; leave them visually unchanged apart from contrast fixes. Modern Dark contrast is
  guarded by `theme::tests::dark_themes_keep_readable_contrast` (text ≥ 4.5:1, graphics ≥ 3:1);
  hard-coded glyph/sketch inks go through `icons::ink` (lifted on dark themes only), and
  light-only surfaces (Messages, legends, peak tags, Chamber Wizard) use tokens.
  **Iridescent Glass** (`ThemeKind::Glass`, `Tokens::glass`) is the "fun", modernised theme
  after the user's 3-D glass icon reference; research and sources in gitignored
  `reference_images/research/glass/NOTES.md`. User direction: everything a bit 3-D, not just
  raised. `ui::glass` paints it: `surface` (raised slab: drop shadow, visible thickness with
  a rainbow lower edge, gradient face, sheen, inner `bevel`, `glint`, iridescent rim; kinds
  Rail/Float/Key/Hover/Selected — Selected is pushed in), `well`/`recess` (sunken: fields,
  tables, charts, the canvas), `bead`/`socket` (checks, radios, status lights), `lip`
  (window thickness), `groove` (engraved dividers), `backdrop` (aurora glows; egui has no blur,
  so glass reads against colour behind it). Icons: `icons::draw_glass_glyph` (Pen = lit glass
  tubes with shadows; bodies with an offset underside); nodes are cut glass gems
  (`draw_glass_node`: contact shadow, extruded sides, a chamfer lit/shaded per edge against
  `LIGHT`, recessed top face — all one mesh, since separate polygons show anti-aliasing seams);
  routing lines are shaded tubes (`diagram::route_line`, stopped short of the head by
  `line_end`) with pyramid arrowheads; chart lines are tubes too. Layout: `WaveStormApp::glass_chrome`
  (header with logo/menus/tabs/event/undo/help, node tile rail, settings rail, canvas island,
  floating dock `glass_dock` with a zoom readout, `glass_status` with a messages chip,
  overlay scrollbars). Toolbar actions are the shared `Cmd` enum (`run`, `cmd_button`), so
  both layouts call the same code. Widgets branch on `t.glass` (`push_button`, `tool_button`,
  `check`, `radio`, `group`, `tabs`/`pill_tab`, `tab_body`, `header_cell`, `sunken`,
  `combo` — the combo slab is painted under egui's button from last frame's rect). Snapshot
  with `--do "theme:glass;..."`; `tests/monkey.rs` runs the random-input test in glass too.
- Tc flow path (2026-10-23, user request): `Tool::TcPath { node, points }` draws a
  subcatchment's flow path (dock/tool-column button `Cmd::TcPath`, greyed out unless exactly one
  subcatchment is selected; Tc tab "Draw Flow Path on Diagram"; right-click menu). Finishing
  (double-click/Enter) opens the edit dialog's `TcPathState` sub-dialog: one flow type per leg,
  sheet limit (default `tc::SHEET_LIMIT_FT` = 100 ft), replace or append; OK writes the
  segments into the draft and `Node::tc_path` (shown while the node is selected). Segment logic
  is `core::tc::segments_from_path` (tested): only lengths come from the path; slope/cover are
  copied from an existing segment of the same type or default. Legs are `tc::PathLeg` (length,
  flow, optional slope; blank keeps the slope above); `tc::path_runs` groups legs into segments
  where flow type and slope stay the same (user rule, 2026-10-23: a new letter only where the
  type or slope changes). While drawing every point is lettered; afterwards only segment ends
  (`path_breaks`/`path_labels`; stored in `Node::tc_path_breaks` so deletions don't re-letter).
  The pending path in an open editor is drawn live; legs are coloured by flow type
  (`diagram::flow_color`). Segments are described "A to B" and remember their span in
  `TcSegment::path` (`TcSegment::new` for plain ones). The sheet limit
  is applied by `tc::limit_sheet_flow`, which inserts a point where it falls inside a leg
  (unless within `SPLIT_SLACK_FT`), so letters stay consistent; `TcPathState::reset`
  recomputes from the drawn points when the limit changes. Deleting segments drops the path
  once none references it (`dialogs::node::drop_orphan_path`); "Remove Drawn Path" keeps the
  segments.
- Drawing on the map (2026-10-24, user direction: "draw things on the map and bring the data
  into subcatchments"). `core::drawing` holds the geometry (polygon area/centroid, ear-clipping
  `triangulate`, `point_in_polygon`, `polygon_within`, `polygons_overlap`, `clamp_inside`,
  `subarea_problem`) and the bookkeeping: `sync_subareas` (drawn `SubArea::outline` rows are
  measured, the `SubArea::remainder` row = outline − drawn; a remainder row is created when
  needed), `outline_changed` (drawn: re-sync; else scale rows in proportion),
  `sync_tc_lengths`, `insert_path_point`/`remove_path_point` (Tc segments keep their legs),
  `transform` (moves all drawings + outlined subcatchment symbols), `sync_all` after
  calibration. UI: `Tool::Trace { node: Option }` (new subcatchment or (re)draw an existing
  one's outline), `Tool::SubArea` (points clamped inside, snapped to corners within 8 px /
  edges within 6 px; refused with a status message if outside/overlapping),
  `Tool::PlaceUnderlay` (drag to move, corner to resize uniformly; drawings follow;
  `feet_per_unit` divided by the scale change so real sizes hold; started automatically after
  loading a background, which is sized to 90 % of the view). Point editing lives in
  `select_tool` for the single selected subcatchment (`shape_handle_at`, `Drag::Vertex`,
  midpoint drag inserts, right-click deletes, `finish_shape_edit` validates/undoes, re-measures
  and emits `CanvasAction::ShapeEdited`, which refreshes open edit drafts). Area grid:
  `widgets::grid_marked` swatches, "Source" column, rebuild keeps each row's drawing by index
  (mirroring grid insert/delete). Colours: `diagram::subarea_color(row)`, remainder = subcat
  colour. Test: `tests/underlay.rs::draw_subareas_edit_and_place_background`.
- Pond estimator (2026-10-27, user request): `Tool::Pond` / `Cmd::Pond` (dock, settings column,
  Diagram menu; script `pond` or `pond:x0,y0,x1,y1` to open the dialog on a rectangle) traces
  the pond's top; `dialogs::misc::pond_estimate` asks top elevation, depth and side slope z
  (bottom = top − depth, `PondEstimate::bottom`),
  then creates a pond at the centroid with `Node::outline` = footprint (drawn/printed in the pond
  colour), `Pond::estimate`, and one `CustomKind::AreaPrismatic` volume described
  `drawing::POND_ESTIMATE`. `drawing::pond_storage`: inward mitred offset area
  A − P·r + K·r² (K = Σ tan(turn/2); exact for convex outlines until an edge vanishes), r =
  z·(depth − h), stages ~0.5 ft apart; `pond_max_depth` = where the sides meet. `sync_node`
  re-estimates that volume when the footprint is edited or the background recalibrated.
  `widgets::push_button` now carries an accessibility label (tests can `get_by_label("OK")`).
- Site location and soils (2026-10-27, user request; branch `feature/geolocated-site-soils`).
  User decisions: no tax-parcel maps (no free nationwide source); dual HSGs default to D with
  a "Site is drained" box (first letter); a subcatchment over several soils has its Area rows
  split by soil (option a), not a dominant-soil row. `Underlay::geo` (`geo::GeoRef`: anchor
  pixel, lon/lat, bearing) + `feet_per_unit` place the diagram; stored per pixel so moving or
  resizing the background keeps it. Background > Aerial Photo (`dialogs::site::locate`,
  `Fetch::Map`): Nominatim address lookup, then USGS NAIP `ImageServer/exportImage` (the cached
  basemap `USGSImageryOnly` is ~15 ft/px at site scale, so it is only used for the "roads and
  names" option), Web Mercator bbox sized so the image is north-up at `feet_per_pixel`; the
  reply must be exactly the requested size. Over an earlier north-up georeferenced background
  the photo lines up with it. Background > Georeference (`Tool::Georef`, `geo::two_point`) also
  sets the scale. Diagram > Soils (`dialogs::site::soils_window`): AOI drawn (`Tool::Aoi`) or
  `soils::default_aoi` (hull of outlines + 10 %/50 ft); `Fetch::Soils` posts
  `soils::sda_request` (`GetClippedMapunits` + `hydgrpdcd` + `STAsText`, AOI `MakeValid`, ≤
  10,000 ac) to SDA; `parse_sda` keeps units/polygons in lon/lat in `Project::soils`.
  `soils::split_subcat` merges earlier splits (`same_row`: description sans ", HSG x",
  outline, remainder, C, land use), then splits each row by the soils under its own ground
  (drawn outline / outline − drawn for remainder / whole outline when nothing is drawn),
  drops groups < 1 %, shares unmapped ground pro rata, keeps one row when the cover's CN is
  equal on all groups, leaves non-TR-55 descriptions alone. Split rows share an outline or
  the remainder flag: `drawing::sync_subareas` shares a shape's area in current proportions,
  `shape_row`/`cn_regions` draw a shared drawing once with a weighted CN label. Overlap areas:
  `soils::Prepared` (ear-clip triangles × Sutherland–Hodgman). Fixed with it: `triangulate`
  accepted ears touched by a reflex corner (symmetric L shapes overlapped). Network:
  `ui::net` (ehttp; ureq+rustls on desktop, fetch on web; replies via inbox like
  `platform`; headless apps never go online, tests push `net::Reply`s). SDA rejects two
  Content-Type headers: replace ehttp's default. Print option "Hydrologic soil group report"
  (`PrintOptions::hsg_report`, on by default when the project has soils): soil map page +
  `soils::report_lines`. `webpki-roots` brought CDLA-Permissive-2.0 into `deny.toml`/
  `about.toml`. Live check: `cargo test -p wavestorm-ui --test site -- --ignored`.
  Sub-area drawing snaps to soil boundaries while the soils are shown (`Doc::soil_snap`, on by
  default, S key or Soils window): `diagram::snap_to_rings` after `snap_to_drawings`; a click
  whose previous point is on the same ring inserts the ring corners between them
  (`drawing::follow_ring`; among rings the geometrically shortest run that passes
  `drawing::path_clear`: inside the outline, clear of other sub-areas, not crossing the line
  so far), also for the closing edge (dropped if the shape would then be invalid, so Enter
  always closes a valid shape); the rubber band previews it; Backspace removes a whole click
  (`Interaction::clicks`). Fixed with it (user report 2026-10-28): split rows share one
  drawing, so `edit_shape` applies point edits to every row with that drawing and
  `drawings_problem`/`subarea_outlines` count each drawing once (twins used to "overlap",
  undoing every edit); `subarea_problem` rejects self-crossing shapes
  (`drawing::polygon_is_simple`); a dragged point dropped within 3 px of a neighbour merges
  with it (`merge_dropped_point`); the drawing preview fill is ear-clipped (the centre fan
  spilled outside concave shapes).
- Tabs and the window title show the project's file name once it has one (`Doc::title`), so
  Save As renames the tab; the printed title is still `Project::title`.
- `tests/window_growth.rs` checks heights too (the curve viewers used to grow 1 px a frame
  from guessed tab/button heights; they now use the height left / last frame's button rows).
- Session (`ui::recovery`, store key `session`; 0.3.0's `recovery` key is still read): `DELAY`
  (1 s) after any change the open projects are recorded — saved files by path, unsaved or
  file-less ones (always on the web) with their JSON, plus the front tab. At start (non-headless)
  they are reopened; missing files are skipped; with nothing to reopen no project is open and
  `start_page` shows (New / Open / Open Sample / recent list; Project and View menus only).
  Code must not assume a document exists: `doc()` creates one, so empty-state paths
  (`draw`, `shortcuts`, `sync_underlay`) check `docs.is_empty()` first; `close_active` may leave
  none. Headless apps still start with the sample project. Recent projects
  (`platform::recent`, store key `recent`, old plain-path list still read): paths on the
  desktop (8), project copies in browser storage on the web (6, oldest dropped when storage is
  full); Project > Recent Projects sits under Open...; entries whose file is gone are reported
  and forgotten. Desktop autosave pref (`autosave`, `autosave_minutes`, off by default) saves
  dirty projects that have a path; the `.bak` timer remains. Timers wake the idle app with
  `request_repaint_after` only while saved projects have unsaved changes.
- Arrow grabbing: `diagram::handle_at` hit-tests the whole arrowhead from the press origin
  (egui reports drag starts after the pointer has moved), nearest wins; hovering a handle shows
  a grab cursor. `tests/monkey.rs::arrowheads_are_easy_to_grab` covers both layouts.
- Responsiveness (2026-10-23, user report "a few frames behind the mouse"): the app must go
  idle when nothing changes. Never call `send_viewport_cmd` or `request_repaint` every frame
  (the window title is sent only when it changes); `tests/idle.rs` asserts every theme and
  common windows settle within 30 frames (window-open animations take ~13). The desktop and
  web apps use `SurfaceConfig::LOW_LATENCY` (one queued frame). Frame cost per theme:
  `cargo run --release -p wavestorm-ui --example frame_bench` (glass ≈ 2 ms with a report
  open); `frame_bench "<script>"` prints the idle repaint delays.
- Glass outlines must not repeat points (a zero-length segment tears the stroke join):
  `glass::rounded` drops coincident corner points, `Pen::circle` drops the closing point.
- After each milestone, rate 1–10: UI look, UI parity, code cleanliness, feature parity.

## 6. README

`README.md` is maintained with the code (user request, 2026-10-23): update its feature list,
file-format table and installation steps (release asset names must match `release.yml`)
whenever those change. It is for users and human contributors: never put agent instructions
in it or link this file from it (user request, 2026-10-25). Regenerate the
screenshots in `docs/screenshots/` after visible UI changes, from the built-in sample project
only (never the user's private files or rainfall library):

```sh
for s in "main select:1P" "hydrograph report:1P;tab:1" "reach-summary report:1R;tab:0" \
         "project-reports project-report:6" "storms storms" "chamber-wizard edit:1P;tab:1;chamber" \
         "outlets edit:1P;tab:2" "glass theme:glass;select:1P" "glass-report theme:glass;compare"; do
  set -- $s; cargo run -q -p wavestorm-ui --example snapshot -- --out docs/screenshots/$1.png --size 1200x720 --do "$2"
done
```

## 7. Commands

```sh
cargo test -p wavestorm-core           # engine tests (fast)
cargo test                             # everything
cargo check -p wavestorm-core --target wasm32-unknown-unknown
```

## 8. Versioning and releases

Versioning started at **v0.1.0** (2026-10-23), the first version the user signed off on.
Semantic versioning: the single source of truth is `version` in the workspace `Cargo.toml`
(shown in the About dialog and on printed pages via `CARGO_PKG_VERSION`).

- There is no changelog file (removed 2026-10-25, user request): the git history is the
  changelog, so write commit subjects that read well as release-note lines (they are listed
  verbatim). Commits titled "Release v…" are left out of the notes.
- Release cadence (user, 2026-10-27, replaces the earlier "release without being asked"
  rule): **only bump the version and tag a release when the user says to.** Otherwise commit
  work normally and leave the version alone; when asked, pick patch or minor by the rules
  below and mention the new tag in the hand-off.
- To release: bump the workspace version (patch = fixes, minor = features or file-format
  additions, major = breaking project-file changes; while 0.x, breaking changes may go in a
  minor), run `cargo test`, clippy and the wasm check, commit "Release vX.Y.Z", then
  `git tag -a vX.Y.Z -m "WAVEStorm X.Y.Z" -m "<highlights paragraph>"`. Pushing commits and
  tags is the user's call.
- CI (`.github/workflows/ci.yml`): `cargo fmt --check` (the workspace was rustfmt-formatted
  2026-10-25; run `cargo fmt` before committing), clippy (`-D warnings`), `cargo test` on Linux/Windows/macOS,
  wasm checks of core and web, `cargo deny check licenses sources` (`deny.toml`: permissive
  licences only; a new dependency with another licence fails CI). Pushing a `v*` tag runs
  `release.yml`: it re-runs CI, checks the tag equals the Cargo.toml version, builds the Linux
  binary/tar.gz/.deb (`[package.metadata.deb]` in `apps/wavestorm/Cargo.toml`)/AppImage on
  Ubuntu 22.04 (glibc floor), a Windows zip and a universal macOS `.dmg`, then publishes a
  GitHub release whose notes come from `.github/release-notes.sh` (tag message body, commits
  since the previous tag, downloads) with `SHA256SUMS.txt`. Run it by hand
  (workflow_dispatch) to build packages without publishing. Packages are unsigned. Every
  package ships `THIRD-PARTY-LICENSES.html` (cargo-about: `about.toml` + `about.hbs`; the
  egui font notices are pinned by checksum in `about.toml` and the workflow fails if they
  drop out). Workspace crates are `publish = false` so they are not listed as third-party.
- App icon: `crates/ui/assets/icon.png` (512 px, written by the `logo_png` example, used for
  the window, the `.desktop` entry, AppImage and macOS bundle). Linux desktop entry:
  `apps/wavestorm/wavestorm.desktop`.
- Project files must stay readable by later versions: new fields get `#[serde(default)]`.
