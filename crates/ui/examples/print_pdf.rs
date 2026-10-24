//! Writes a PDF report: `cargo run -p wavestorm-ui --example print_pdf -- out.pdf [project.waves]`
//! (the sample project when no project is given; its background is printed when it has one).
//! An `.emf` output path writes the routing diagram as EMF instead.

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "report.pdf".into());
    let p = match args.next() {
        Some(path) => {
            let bytes = std::fs::read(&path).expect("read project");
            wavestorm_ui::project_from_bytes(&path, bytes).expect("parse project").0
        }
        None => wavestorm_ui::samples::sample_project(),
    };
    let r = wavestorm_core::calc::calculate(&p);
    let background = wavestorm_ui::underlay::print_background(&p);
    let bytes = if out.ends_with(".emf") {
        wavestorm_core::printing::diagram_emf(&p, &r, "10/22/2026", background)
    } else {
        let opts = wavestorm_core::printing::PrintOptions { date: "10/22/2026".into(), tables: true, background, ..Default::default() };
        wavestorm_core::printing::report_pdf(&p, &r, &opts)
    };
    std::fs::write(&out, bytes).expect("write");
    println!("wrote {out}");
}
