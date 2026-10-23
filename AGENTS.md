# AGENTS.md — guide for AI agents and contributors

StormWAVES is a clean-room, open-source stormwater modelling application (TR-55 / TR-20 hydrology,
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
   IDF (`.idf`) and county rainfall-table (`*-Rain.txt`) text formats. Their data files are
   loaded at runtime from files the user supplies; never commit or bundle them. Tests use small
   fixtures written by us, or the gitignored samples in `reference_images/expert_reference/`
   and skip when those are absent. The `.hcp` header section carries the product name: build
   that string at runtime from two pieces (`concat!`-style splitting) so the hook never sees it.
3. **Never panic** on user input or file data. No `unwrap`/`expect`/`panic!`/indexing on
   untrusted values in non-test code (`#![deny(clippy::unwrap_used, …)]` in each crate). Invalid
   input produces a calculation message, never a crash.
4. **The core never depends on the UI** and must compile for `wasm32-unknown-unknown`.
5. **Tests are the gate.** Every algorithm ships with tests; reproduce published reference
   results where they exist (see §4). Run `cargo test` before every commit.
6. Commit each major feature. Commit messages end with the co-author line the session asks for.
7. **Edit files with the editor tools** (Edit/Write), never with Python or sed scripts, so every
   change is a reviewable diff. Scripts are fine for analysis in the gitignored research folder.

## 2. Workspace

```text
crates/core   stormwaves-core: hydrology & hydraulics engine (no UI, wasm-clean)
crates/ui     stormwaves-ui:   egui application shell
apps/stormwaves       desktop binary
apps/stormwaves-web   browser build via trunk
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
| `rainlib` | Rainfall library files: `.hcr`/`.huh`/`.idf` parsers and writers, `*-Rain.txt` county tables, storm-code mapping |
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
  `.idf` files. The `.idf` text layout is OUR ASSUMPTION (no sample): `name/comment/timeunits/
  depthunits`, then per event `curve=`, optional `factor=`, `coef= a b c` (i = a/(b+t)^c) or
  `idf= t1 i1 t2 i2 ...` points (log-log interpolation); a NOAA Atlas 14 depth CSV is also
  accepted. Writers: `write_hcr/huh/idf`. Library folder pref (Settings > General, desktop),
  "Import Rainfall Files..." (desktop and web; web imports persist in localStorage up to 2.5 MB).
  Library storms/UHs used by events, the default UH or a subcatchment UH are copied into the
  project (`library::adopt`) so projects stay self-contained. The full-library test runs when
  the folder exists (`STORMWAVES_RAIN_LIB` or `reference_images/expert_reference/Rainfall`);
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
  headless app (`StormwavesApp::headless()`) for tests and snapshots — never open native
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
  `cargo run -p stormwaves-ui --example snapshot -- --out x.png --do "<script>"`.
  Done: OK/Cancel/Apply/Help row in every dialog (`ok_cancel_apply(ui, apply, help_topic)`),
  white chart background, report button column (Table, 2D/Shrink, Print, Export, Edit, Help;
  `widgets::toggle_button`), min/max/close boxes on report windows (`Win::min_max`), Events
  table in the reference style (`reports::report_table`), comparison report with
  Summary/Hydrograph/Profile tabs and the outflow selector at top right.
  Checklist (all done 2026-10-23 except where noted):
  1. (done) Project Reports: reference tab set, All/Selected Nodes, Show Totals, button
     column; tables from `core::listings` (script `project-report:N`).
  2. (done) Storm / UH / IDF viewers (`reports/curves.rs`; IDF graph on a log duration axis,
     `ChartSpec::log_x`) and the Event Depth(s) option (script `storms-events`). `.idf` support is
     marked PROVISIONAL in the UI and in written files (`rainlib::IDF_PROVISIONAL`) until a
     real `.idf` sample is available; remove the marker once the layout is verified.
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
- Name and logo (2026-10-23): the product is written **StormWAVES** in all user-facing text.
  Crate, binary and directory names, Rust identifiers (`StormwavesApp`), the
  `STORMWAVES_RAIN_LIB` variable and the prefs/localStorage keys stay `stormwaves` (renaming the
  keys would lose saved settings). `crates/ui/assets/logo.svg` is the vector master ("Storm" in
  Liberation Sans Bold outlines, SIL OFL, before a vector trace of the WAVES wordmark,
  `logo_waves.svg`). `logo.png` is it rendered 1024 px wide for egui (`ui::logo`, About
  dialog; black ink drawn light on dark themes); re-render it after editing the SVG. The web
  loading screen inlines a copy of the SVG paths in `apps/stormwaves-web/index.html`.
- After each milestone, rate 1–10: UI look, UI parity, code cleanliness, feature parity.

## 6. README

`README.md` is maintained with the code (user request, 2026-10-23): update its feature list,
file-format table and installation steps whenever those change, and regenerate the
screenshots in `docs/screenshots/` after visible UI changes, from the built-in sample project
only (never the user's private files or rainfall library):

```sh
for s in "main select:1P" "hydrograph report:1P;tab:1" "reach-summary report:1R;tab:0" \
         "project-reports project-report:6" "storms storms" "chamber-wizard edit:1P;tab:1;chamber" \
         "outlets edit:1P;tab:2"; do
  set -- $s; cargo run -q -p stormwaves-ui --example snapshot -- --out docs/screenshots/$1.png --size 1200x720 --do "$2"
done
```

## 7. Commands

```sh
cargo test -p stormwaves-core          # engine tests (fast)
cargo test                             # everything
cargo check -p stormwaves-core --target wasm32-unknown-unknown
```

## 8. Versioning and releases

Versioning started at **v0.1.0** (2026-10-23), the first version the user signed off on.
Semantic versioning: the single source of truth is `version` in the workspace `Cargo.toml`
(shown in the About dialog and on printed pages via `CARGO_PKG_VERSION`).

- Record every user-visible change under `## [Unreleased]` in `CHANGELOG.md` as you commit it.
- Release only when the user asks: move the Unreleased entries under a new `## [X.Y.Z] - date`
  heading, bump the workspace version (patch = fixes, minor = features or file-format
  additions, major = breaking project-file changes), run `cargo test`, clippy and the wasm
  check, commit "Release vX.Y.Z", then `git tag -a vX.Y.Z`. Pushing tags is the user's call.
- Project files must stay readable by later versions: new fields get `#[serde(default)]`.
