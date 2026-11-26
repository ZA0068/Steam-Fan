/// The Steam-Fan logo, for window icons.
pub fn icon() -> std::sync::Arc<eframe::egui::IconData> {
    std::sync::Arc::new(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default())
}
