//! `solvers-gui` binary entry point; everything lives in the `gui` library.

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1600.0, 950.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Solvers — Multiway Preflop",
        options,
        Box::new(|cc| Ok(Box::new(gui::app::App::new(cc)))),
    )
}
