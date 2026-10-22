# AGENTS.md — guide for AI agents and contributors

Stormwaves is a clean-room, open-source stormwater modelling application (TR-55 / TR-20 hydrology,
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
3. **Never panic** on user input or file data. No `unwrap`/`expect`/`panic!`/indexing on
   untrusted values in non-test code (`#![deny(clippy::unwrap_used, …)]` in each crate). Invalid
   input produces a calculation message, never a crash.
4. **The core never depends on the UI** and must compile for `wasm32-unknown-unknown`.
5. **Tests are the gate.** Every algorithm ships with tests; reproduce published reference
   results where they exist (see §4). Run `cargo test` before every commit.
6. Commit each major feature. Commit messages end with the co-author line the session asks for.

## 2. Workspace

```text
crates/core   stormwaves-core: hydrology & hydraulics engine (no UI, wasm-clean)
crates/ui     stormwaves-ui:   egui application shell (planned)
apps/stormwaves       desktop binary (planned)
apps/stormwaves-web   browser build via trunk (planned)
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
| `base64` | Embedding background files in project JSON |

Internal units are US customary: ft, sq ft, cu ft, cfs, hours, inches of rain. Conversion to
display units (English/metric, large/small units) happens only at the UI/report edge.

## 3. Methods confirmed against published reference results

These choices were reverse-engineered from public sample reports and are covered by tests. Do
not change them without re-running the golden tests.

- **SCS runoff:** NRCS curvilinear UH, unit duration ΔD = 0.133 Tc so Tp = (0.0665 + 0.6)·Tc.
  Excess is convolved on an internal step df = dt / ceil(dt/ΔD), sampling the UH at
  τ = (k+1)·df after each increment began, kernel normalised to exact volume; results sampled
  at dt. Eight published subcatchments match within 1.5 % peak, 0.5 % volume.
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
sample reports summarised in `reference_images/research/NOTES.md`.

## 5. Status and next steps

Keep this section updated at the end of every session.

- Done: core hydrology (runoff, Tc, CN, rainfall, UH), hydraulics (sections, storage, all
  outlet devices), routing (level pool, Stor-Ind+Trans, Muskingum-Cunge, Dyn-Stor-Ind, detention
  times, pond sizing), links, units, project model (JSON `*.waves`), calculation engine with
  messages, reports, PDF printing, land-use loading, CSV import/export/merge, full egui UI
  (menus, palette, diagram, dialogs, charts, help), background image/PDF underlay with scale
  calibration and subcatchment tracing, desktop and web (trunk) apps.
- Remaining parity gaps: TR-55 graphical peak/storage worksheet, node profile report, rainfall
  event lookup tables, TR-20 import, custom units, rock-fill/asymmetrical weirs, pump priority
  groups, report "Inflows" columns, status-bar hints, theme persistence.
- Open investigation (runoff timing), data in `tests/published_subcatchments.rs` (18 cases
  from public engineering reports made with the current reference version; raw text and
  `cases.json` in gitignored `reference_images/research/ext/`):
  - Volumes match exactly in every case; peaks within ~1.5 % for Tc >= 10 min.
  - Peak *times* run early by about 0.06-0.09 Tc at dt 0.05 h (e.g. Tc 72.5 min: 12.87 vs
    12.98 h) and ~0.01 h at dt 0.01. Delaying the UH by ~0.08 Tc also fixes the summed link
    peaks (published 2-subcat link sums match within 0.3 %, vs +1.5 % now) and the ~4.5 % high
    summed inflow in `tests/reference_network.rs`. BUT the older-version golden hydrograph
    (`runoff::tests::golden_reference_runoff_hydrograph`, Tc 23.2, dt 0.1, from a 2003 sample)
    matches with zero delay, so the mechanism is unconfirmed. Do not change `uh_kernel`
    until one model fits both; candidates tried (Python scripts in gitignored `reference_images/research/ext/tools/`; they take the folder holding `t2tab.txt` as first argument): kernel offset,
    interval-averaged UH, Tp = ΔD + L (peaks -6 %), fixed fractional delay (best, 0.06 Tc with
    fine convolution or 0.09 Tc with the current substep scheme).
  - Short Tc (4.8 min) peaks are ~12 % low and ~0.04 h early: the reference uses its own
    piecewise-cubic ("polynomial") fit of the NRCS Type storms with a sharper, later peak than
    our monotone-cubic smoothing of the WinTR-55 0.1-h table. Sharpening only the 11.8-11.9 h
    slopes does not fix it; a free fit of the 11.6-12.2 h knots trades it against long-Tc
    timing. Build our own fit (never copy the reference's coefficient files).
- After each milestone, rate 1–10: UI look, UI parity, code cleanliness, feature parity.

## 6. Commands

```sh
cargo test -p stormwaves-core          # engine tests (fast)
cargo test                             # everything
cargo check -p stormwaves-core --target wasm32-unknown-unknown
```
