//! Writes the sample project's PDF report: `cargo run -p stormwaves-ui --example print_pdf -- out.pdf`

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "report.pdf".into());
    let p = stormwaves_ui::samples::sample_project();
    let r = stormwaves_core::calc::calculate(&p);
    let opts = stormwaves_core::printing::PrintOptions { date: "10/22/2026".into(), tables: true, ..Default::default() };
    std::fs::write(&out, stormwaves_core::printing::report_pdf(&p, &r, &opts)).expect("write");
    println!("wrote {out}");
}
