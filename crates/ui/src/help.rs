//! Built-in help: short topics on using the program and on the methods it implements.

use crate::theme::Tokens;
use crate::widgets;
use egui::{RichText, vec2};

pub struct Topic {
    pub id: &'static str,
    pub title: &'static str,
    pub body: &'static str,
}

pub const TOPICS: &[Topic] = &[
    Topic {
        id: "start",
        title: "Getting started",
        body: "A project is a routing diagram of nodes. Drag a Subcat, Reach, Pond or Link from the palette on the left onto the diagram, \
then double-click it (or right-click > Edit) to describe it.\n\n\
Each node's primary outflow is the arrow leaving it. Drag the arrowhead onto another node to route the flow there; drop it on empty space to remove it. \
Ponds and links also have secondary (dashed red) and tertiary (dashed green) outflows: select the node to see their handles.\n\n\
Results are recalculated after every change. Double-click a described node to open its report; reports stay open and update live.\n\n\
Choose the rainfall event in the toolbar. Set the storm, depths, time span and methods under Settings > Calculation.",
    },
    Topic {
        id: "mouse",
        title: "Mouse and keyboard",
        body: "Wheel: zoom about the pointer.  Middle-drag or Space+drag: pan.  Shift+wheel: pan sideways.\n\
Click: select a node.  Ctrl/Shift+click: add to the selection.  Drag on empty space: select with a rectangle.\n\
Drag a node: move the selection.  Ctrl+drag: clone the selection.\n\
Drag an arrowhead: reroute.  Double-click: report.  Right-click: node menu.\n\
Delete: delete the selection.  Ctrl+C / Ctrl+X / Ctrl+V: copy, cut, paste nodes.  Ctrl+Z / Ctrl+Y: undo, redo.\n\
Ctrl+S: save.  Ctrl+O: open.  Ctrl+N: new project.  Esc: cancel a drawing tool.\n\n\
In charts, drag down-right to zoom into a region and drag up-left to restore the full view.",
    },
    Topic {
        id: "runoff",
        title: "Runoff: SCS (TR-20) unit hydrograph",
        body: "Runoff depth follows the SCS runoff equation  Q = (P - Ia)^2 / (P - Ia + S),  S = 1000/CN - 10,  Ia = (Ia/S) x S  (Ia/S = 0.2 by default).\n\n\
The runoff hydrograph is built by convolving each increment of rainfall excess with the NRCS curvilinear dimensionless unit hydrograph (NEH Part 630, Table 16-1, peak rate factor 484, or a gamma unit hydrograph for another peak factor). \
The unit duration is 0.133 Tc, so the time to peak is Tp = 0.0665 Tc + 0.6 Tc. The convolution runs on an internal step no longer than the unit duration and is sampled at the calculation step; runoff volume is conserved exactly. \
The response is delayed by 0.09 Tc, calibrated so peak times agree with published results.\n\n\
Reported peaks are interpolated with a parabola through the three highest values, as TR-20 does. \
The time span must cover the whole storm for the volume to equal the runoff equation's depth.",
    },
    Topic {
        id: "resize",
        title: "Automatic pipe and culvert sizing",
        body: "Mark a round pipe reach (reach editor) or a round culvert outlet (outlet editor) with \"Size automatically\". \
Every calculation then sets its diameter to the smallest size in the list on Settings > Calculation > Resize that meets the criterion for the current event, and writes it into the project (Undo restores the previous size).\n\n\
Pipe reach: the peak flow depth may not exceed 98% of the diameter, so the pipe is sized for pipe-full Manning flow. A reach assumes normal flow and ignores inlet control and tailwater; where those matter, model a pond with a culvert outlet instead.\n\
Culvert: the pond's peak water level may not exceed the culvert crown (the outlet crown when the barrel slopes backwards). Several marked culverts in one pond get the same size.\n\n\
With the Flood elevation criterion a reach's flood depth or a pond's flood elevation is the limit instead, where one is defined. \
Nodes are sized in flow order, so each size accounts for the sizes chosen upstream. Message 59 reports each change; if even the largest size fails, it is used and the message is a warning. \
Clear \"Allow sizes smaller\" to only ever increase the sizes entered.",
    },
    Topic {
        id: "cn",
        title: "Curve numbers",
        body: "Each sub-area has an area, a curve number and a description. Use Lookup CN to choose a cover and hydrologic soil group from TR-55 Table 2-2.\n\n\
The composite CN is the area-weighted average, rounded to a whole number unless that is turned off (Settings > Calculation > Advanced). \
Other options compute pervious and impervious runoff separately, or runoff for every CN separately (weighted Q).\n\n\
Areas with CN 98 or more are reported as impervious. Pre-weighted urban covers such as \"1/4 acre lots, 38% imp\" are split into their impervious and pervious parts. \
If the total impervious fraction is under 30% and some impervious covers are marked \"Unconnected\", the TR-55 Figure 2-4 reduction is applied: CNc = CNp + Pimp (98 - CNp)(1 - 0.5R).\n\n\
The antecedent moisture condition (AMC) for each event converts CNs to dry (I) or wet (III) conditions.",
    },
    Topic {
        id: "tc",
        title: "Time of concentration",
        body: "Tc is the sum of the travel times of the flow segments on the Tc tab, each rounded to 0.1 minute.\n\n\
Sheet flow (TR-55):  Tt = 0.007 (nL)^0.8 / (P2^0.5 s^0.4)  hours, L up to 300 ft; McCuen-Spiess limits L to 100 s^0.5 / n.\n\
Shallow concentrated flow:  V = Kv s^0.5  (Kv 20.33 paved, 16.13 unpaved, and the NEH upland values).\n\
Channel or pipe:  Manning's equation at bank-full depth; capacity is reported.\n\
Lag (CN) method:  Tc = L^0.8 (S+1)^0.7 / (1140 Y^0.5)  hours, Y in percent.\n\
Kirpich:  Tc = 0.0078 L^0.77 s^-0.385  minutes times an adjustment factor.\n\
Kerby:  Tc = 1.44 (L N)^0.467 s^-0.235  minutes.\n\
Lake or reservoir:  wave celerity V = (g D)^0.5.\n\
A minimum Tc can be set on the Advanced settings tab.\n\n\
Drawing the flow path: select a subcatchment and use the Tc Flow Path tool (beside Measure, \
or Draw Flow Path on Diagram on the Tc tab, or the node's right-click menu). Click along the \
longest flow path from the hydraulically most distant point to the outlet; every clicked point \
can start a new segment. Double-click or press Enter to finish (Backspace removes the last \
point, Esc cancels); while drawing every point is lettered. Then set each leg's flow type \
and, where it changes, its slope (blank keeps the slope of the leg above). A new segment starts \
wherever the flow type or the slope changes; only segment ends are lettered on the diagram \
(A, B, C…, and the segments are described \"A to B\"), and each leg is coloured by its flow \
type (sheet green, shallow orange, channel blue, pipe violet, lake teal). Sheet flow beyond \
the limit (100 ft by default; TR-55 allows 300 ft) continues as shallow concentrated flow. \
Lengths are measured with the background's calibrated scale; surface (and slope where no leg \
gives one) is copied from an existing segment of the same type when there is one. The path is \
kept with the subcatchment and shown while it is selected; deleting its last segment removes \
it.",
    },
    Topic {
        id: "rainfall",
        title: "Rainfall events and storms",
        body: "Each event names a storm distribution and a total depth, with an antecedent moisture condition and an optional number of back-to-back storms.\n\n\
The NRCS Type I, IA, II and III 24-hour distributions use the published 0.1-hour tables, smoothed with a monotone cubic so short Tc values see realistic intensities; \"Tabular\" versions are unsmoothed for direct TR-20 comparisons. \
6- and 12-hour Type II and III storms are the central part of the 24-hour storms.\n\n\
Duration mode Rescale stretches a storm to a new duration; Trim keeps its central part (recommended for the NRCS types).\n\
More Storms creates custom distributions from mass-curve, intensity or incremental data, or from IDF coefficients (Chicago storm).\n\n\
Rainfall Event Lookup (Settings menu, or Lookup on the Rainfall tab) creates events from a NOAA Atlas 14 precipitation-frequency table: \
download the point estimates (precipitation depth, CSV) for the site from NOAA's Precipitation Frequency Data Server, load the file, \
pick the duration row and the recurrence intervals, then Replace or Add events.",
    },
    Topic {
        id: "reach",
        title: "Reach routing",
        body: "A reach routes a hydrograph through an open channel or pipe under normal-flow conditions.\n\n\
Stor-Ind: level-pool routing with storage = flow area x length and outflow = Manning discharge.\n\
Stor-Ind+Trans: the same, with the outflow translated by the travel time at the peak velocity (length / Vmax).\n\
Muskingum-Cunge: celerity and attenuation parameters from the peak inflow.\n\n\
Inflow above the Manning capacity is reported (hint 55). A full pipe detains the excess at its inlet (warning 76); use a pond with a culvert outlet to model inlet losses and pressure flow.",
    },
    Topic {
        id: "pond",
        title: "Ponds, storage and outlets",
        body: "A pond routes its inflow with the storage-indication (modified Puls) method. Storage can combine prismatoids, cones and cylinders, pipes, arch chambers and custom stage data, with void ratios, multipliers and embedding (a chamber inside a stone bed).\n\n\
Outlet devices: culvert (barrel or inlet control, the lesser of the two), orifice/grate, sharp- and broad-crested weirs (Brater & King coefficients), vee/trapezoidal and custom weirs, exfiltration, rating tables, constant-flow skimmers, pumps, siphons, delayed gates, dam breach and channel outlets.\n\n\
Route a device to another device to build a riser: the downstream device passes no more than its own capacity. Route devices to Secondary or Tertiary to split flows, and exfiltration to Discarded.\n\n\
Tailwater can be free, fixed, a fixed difference below the headwater, or dynamic (Dyn-Stor-Ind routing takes it from the downstream node).\n\
Detention times are reported by the center-of-mass and plug-flow methods. The Sizing tab estimates the storage needed for any allowed discharge.",
    },
    Topic {
        id: "link",
        title: "Links",
        body: "A link sums its inflows and can add a manually entered hydrograph or one imported from a CSV file (first column time, then one column per hydrograph or event).\n\n\
The Advanced tab adjusts the primary outflow: threshold (only flow above it), limit, multiplier, time lag, start and end times and a volume slice. The remainder is the secondary outflow. \
The Elevation tab defines a fixed, tabular or tidal water level that upstream ponds see as tailwater with dynamic routing.",
    },
    Topic {
        id: "background",
        title: "Background images and drawing on the diagram",
        body: "Diagram > Background > Load Image or PDF places a site plan, aerial photo or PDF sheet behind the diagram, sized to the part of the diagram in view. \
It starts in Place Background mode: drag the image to move it, drag a corner to resize it, press Enter when done. \
Move / Resize Background (Diagram > Background, the dock, or Background Settings) returns to that mode later. Everything drawn on the diagram moves and \
scales with the background so it stays on it, and the calibration follows the size, so measured areas and lengths do not change. \
Background Settings > Lock position and size prevents moving it by accident.\n\n\
Calibrate the scale: choose Calibrate Scale (ruler button), click two points a known distance apart (a scale bar or dimensioned line) and enter the real distance. \
Drawn areas and Tc lengths are re-measured at the new scale.\n\n\
Trace a subcatchment: choose Trace Subcatchment, click the corners of the drainage area and click the first point again (or double-click, or press Enter). \
A subcatchment is created at the centre of the outline with the measured area; edit it to set ground covers and Tc. Corners snap to the corners of other \
outlines, so neighbouring subcatchments can share boundaries. An existing subcatchment can be given an outline, or a new one, with Draw / Redraw Outline \
(right-click menu or the Area tab).\n\n\
Sub-areas: select a subcatchment that has an outline and choose Draw Sub-Area (dock, right-click menu or the Area tab), then click the corners of an area \
of different cover (roofs, pavement, a pond…). Points are kept inside the outline and snap to nearby corners and edges; a sub-area may not overlap another. \
Its area is measured, the Area tab opens with it selected to pick its cover, and the row that was there becomes the remainder: the outline's area less the \
drawn sub-areas, so the rows always add up to the outline. Each drawn sub-area has its own colour on the diagram and on the Area tab; their areas (and \
the remainder's) cannot be typed. Subcatchments without drawn sub-areas work as before.\n\n\
Editing drawings: select the subcatchment. Its outline, sub-areas and Tc path show their points: drag a point to move it, drag the small dot in the middle \
of an edge to add a point there, right-click a point to delete it. Areas, the remainder and the Tc segment lengths (keeping their types, slopes and \
surfaces) update when you let go. An edit that would leave a sub-area outside the outline or overlapping another is undone. Moving the subcatchment's \
symbol moves its drawings with it.\n\n\
Measure Length reports the length of a clicked path.",
    },
    Topic {
        id: "messages",
        title: "Calculation messages",
        body: "Notes describe the calculation. Hints point out conditions worth checking. Warnings indicate results that may not be reliable. Errors stop the calculation.\n\n\
Common messages: [37] longer time span advised; [40] node not described; [43] pond has no inflow; [45] no runoff; [47] subcatchment peak exceeds a Tc segment's capacity; \
[49] Tc less than 2 dt; [55] reach inflow above Manning capacity; [73] peak may fall outside the time span; [76] pipe reach detained water; [82] early inflow needs an earlier time span; \
[92] outlet device above the storage; [93] storage range exceeded (add a higher stage); [520] routing loop; [595] rainfall not found.\n\n\
Click a message to select its node; double-click to open its report.",
    },
    Topic {
        id: "tr20",
        title: "Importing TR-20 input files",
        body: "Project > Import > TR-20 Input File creates a new project from a legacy NRCS TR-20 input deck. The original file is not changed.\n\n\
RUNOFF statements become subcatchments (area in square miles, CN and Tc in hours as given), REACH statements become reaches with the XSECTN rating table, \
RESVOR statements become ponds whose storage and outlet come from the STRUCT table, and ADDHYD statements become links that sum their inflows. \
SAVMOV moves hydrographs between locations; the routing diagram is drawn from the order of the statements.\n\n\
Each COMPUT statement becomes a rainfall event (depth, duration, antecedent runoff condition). RAINFL tables become custom storms and a DIMHYD table becomes the project's unit hydrograph. \
INCREM sets the time step. Composite CNs are not rounded, as in TR-20.\n\n\
Anything that cannot be converted (DIVERT, READHD, BASFLO, INSERT/ALTER/DELETE) or that should be checked is listed in Project Notes, which open after the import. \
Results differ somewhat from TR-20 because reaches are routed by this program's own methods rather than Att-Kin.",
    },
    Topic {
        id: "print",
        title: "Printing and exporting",
        body: "Print > Report (PDF) writes a report with the routing diagram, node listing, area and soil listing, node summaries, hydrograph graphs and optional tabular hydrographs. \
In the browser the PDF downloads; on the desktop you choose where to save it.\n\n\
Project > Export writes the node listing as text or every hydrograph as CSV. Report windows can copy their summary to the clipboard.",
    },
];

pub struct HelpState {
    pub topic: usize,
}

impl HelpState {
    pub fn topic(id: &str) -> Self {
        HelpState { topic: TOPICS.iter().position(|t| t.id == id).unwrap_or(0) }
    }
}

pub fn show(ctx: &egui::Context, st: &mut HelpState) -> bool {
    let mut open = true;
    crate::frame::Win::new("WAVEStorm Help").open(&mut open).resizable(true).default_size(vec2(720.0, 460.0)).show(
        ctx,
        |ui| {
            let t = Tokens::get(ui.ctx());
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(210.0);
                    for (i, topic) in TOPICS.iter().enumerate() {
                        if ui.selectable_label(st.topic == i, topic.title).clicked() {
                            st.topic = i;
                        }
                    }
                });
                let (rect, _) = ui.allocate_exact_size(
                    vec2(ui.available_width(), ui.available_height().max(300.0)),
                    egui::Sense::hover(),
                );
                widgets::sunken(ui.painter(), rect, t.field, &t);
                let mut child = ui.new_child(
                    egui::UiBuilder::new().max_rect(rect.shrink(10.0)).layout(egui::Layout::top_down(egui::Align::Min)),
                );
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
                    if let Some(topic) = TOPICS.get(st.topic) {
                        ui.label(RichText::new(topic.title).size(17.0).strong());
                        ui.add_space(6.0);
                        for para in topic.body.split("\n\n") {
                            ui.add(egui::Label::new(para).wrap());
                            ui.add_space(6.0);
                        }
                    }
                });
            });
        },
    );
    open
}
