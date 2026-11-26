//! Pressing Play: copy the game from the shelf if needed (with a progress
//! window), start it, back up its saves.

use eframe::egui;
use std::sync::{Arc, Mutex};
use steam_fan_core::{launch, library, transfer, Config, Db, Game, Kind};

#[derive(Default)]
struct State {
    progress: transfer::Progress,
    done: bool,
    error: Option<String>,
}

pub fn launch(id: &str, command: &[String]) -> i32 {
    let cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => return show_error("Steam-Fan settings", &format!("{e:#}")),
    };
    let mut db = match Db::open_default() {
        Ok(d) => d,
        Err(e) => return show_error("Steam-Fan", &format!("{e:#}")),
    };
    let g = match launch::find_game(&cfg, &mut db, id) {
        Ok(g) => g,
        Err(e) => return show_error("Can't start this game", &format!("{e:#}")),
    };

    if !library::is_installed(&cfg, &g) {
        if !cfg.shelf_online() {
            return show_error(
                &format!("Can't start {}", g.name),
                "It isn't on this PC yet and the shelf (NAS) isn't reachable.",
            );
        }
        if !download(&cfg, &g) {
            return 1;
        }
    }

    let res = match g.kind {
        Kind::Rom => launch::run_rom(&cfg, &db, &g),
        _ => launch::run_program(&cfg, &db, &g, command),
    };
    match res {
        Ok(rc) => rc,
        Err(e) => show_error(&format!("Can't start {}", g.name), &format!("{e:#}")),
    }
}

/// Copy the game with a progress window. True when it's on the PC.
fn download(cfg: &Config, g: &Game) -> bool {
    let state = Arc::new(Mutex::new(State::default()));
    let cancel = transfer::Cancel::default();
    {
        let (cfg, g, state, cancel) = (cfg.clone(), g.clone(), state.clone(), cancel.clone());
        std::thread::spawn(move || {
            let res = Db::open_default().and_then(|db| {
                library::install(&cfg, &db, &g, &cancel, &mut |p| state.lock().unwrap().progress = p.clone())
            });
            let mut s = state.lock().unwrap();
            s.done = true;
            if let Err(e) = res {
                s.error = Some(format!("{e:#}"));
            }
        });
    }
    let title = format!("Copying {} from the shelf", g.name);
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([460.0, 150.0])
            .with_resizable(false)
            .with_app_id("steam-fan")
            .with_icon(crate::util::icon())
            .with_title(&title),
        ..Default::default()
    };
    let shown = {
        let state = state.clone();
        let cancel = cancel.clone();
        let name = g.name.clone();
        eframe::run_native(
            "Steam-Fan",
            opts,
            Box::new(move |_| Ok(Box::new(Progress { state, cancel, name, title }))),
        )
    };
    if shown.is_err() {
        // no screen to draw on: wait quietly, with notifications
        steam_fan_core::notify(&format!("Copying {}", g.name), "From the shelf. The game starts when it's done.");
        loop {
            {
                let s = state.lock().unwrap();
                if s.done {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    } else {
        // window closed by hand before the end: stop the copy
        if !state.lock().unwrap().done {
            cancel.cancel();
            while !state.lock().unwrap().done {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
    let s = state.lock().unwrap();
    match &s.error {
        None => true,
        Some(e) => {
            if shown.is_err() {
                steam_fan_core::notify(&format!("Copy of {} failed", g.name), e);
            }
            false
        }
    }
}

struct Progress {
    state: Arc<Mutex<State>>,
    cancel: transfer::Cancel,
    name: String,
    title: String,
}

impl eframe::App for Progress {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let s = self.state.lock().unwrap();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(&self.title);
            ui.add_space(8.0);
            if let Some(e) = &s.error {
                ui.colored_label(ui.visuals().error_fg_color, format!("{} could not be copied:", self.name));
                ui.label(e);
                ui.add_space(6.0);
                if ui.button("Close").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            } else {
                let p = &s.progress;
                ui.add(egui::ProgressBar::new(p.fraction()).show_percentage().animate(true));
                ui.label(format!(
                    "{} of {}  ·  file {} of {}",
                    transfer::human(p.done_bytes),
                    transfer::human(p.total_bytes),
                    p.done_files,
                    p.total_files
                ));
                ui.small(&p.current);
                ui.label("The game starts when this is done.");
                if ui.button("Cancel").clicked() {
                    self.cancel.cancel();
                }
            }
        });
        if s.done && s.error.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(150));
    }
}

/// Tell the user something went wrong (Steam would show nothing). Returns exit code 1.
pub fn show_error(title: &str, msg: &str) -> i32 {
    eprintln!("steam-fan: {title}: {msg}");
    let (t, m) = (title.to_string(), msg.to_string());
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([440.0, 150.0]).with_app_id("steam-fan")
            .with_icon(crate::util::icon()).with_title(&t),
        ..Default::default()
    };
    let r = eframe::run_native("Steam-Fan", opts, Box::new(move |_| Ok(Box::new(ErrorBox(t, m)))));
    if r.is_err() {
        steam_fan_core::notify(title, msg);
    }
    1
}

struct ErrorBox(String, String);
impl eframe::App for ErrorBox {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(&self.0);
            ui.add_space(6.0);
            ui.label(&self.1);
            ui.add_space(8.0);
            if ui.button("Close").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}
