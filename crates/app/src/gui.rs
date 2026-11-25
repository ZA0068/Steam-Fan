//! The Steam-Fan window: browse the shelf, put games on this PC or take them
//! off, add games to the shelf, keep Steam's library in step, settings.

use crate::util;
use eframe::egui::{self, Color32, RichText};
use egui_extras::{Column, TableBuilder};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use steam_fan_core::archive::{self, Freshness};
use steam_fan_core::config::{default_emulators, Emulator};
use steam_fan_core::steam::{self, Manifest};
use steam_fan_core::transfer::{self, Cancel, Progress};
use steam_fan_core::{library, Config, Db, Game, Kind, Platform};

pub fn run() -> i32 {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 740.0])
            .with_min_inner_size([760.0, 420.0])
            .with_app_id("steam-fan")
            .with_title("Steam-Fan"),
        ..Default::default()
    };
    match eframe::run_native("Steam-Fan", opts, Box::new(|cc| Ok(Box::new(App::new(cc))))) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("steam-fan: cannot open the window: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------- background jobs

#[derive(Default)]
struct JobState {
    progress: Option<Progress>,
    status: String,
    done: bool,
    error: Option<String>,
    result: String,
}

struct Job {
    title: String,
    state: Arc<Mutex<JobState>>,
    cancel: Cancel,
    /// read the library again when it finishes
    reload: bool,
}

impl Job {
    fn spawn(
        title: impl Into<String>,
        reload: bool,
        work: impl FnOnce(&Arc<Mutex<JobState>>, &Cancel) -> anyhow::Result<String> + Send + 'static,
    ) -> Job {
        let state = Arc::new(Mutex::new(JobState::default()));
        let cancel = Cancel::default();
        let (s, c) = (state.clone(), cancel.clone());
        std::thread::spawn(move || {
            let r = work(&s, &c);
            let mut st = s.lock().unwrap();
            st.done = true;
            match r {
                Ok(msg) => st.result = msg,
                Err(e) => st.error = Some(format!("{e:#}")),
            }
        });
        Job { title: title.into(), state, cancel, reload }
    }
}

fn progress_cb(s: &Arc<Mutex<JobState>>) -> impl FnMut(&Progress) + '_ {
    move |p: &Progress| s.lock().unwrap().progress = Some(p.clone())
}

// ---------------------------------------------------------------- app state

#[derive(PartialEq, Clone, Copy)]
enum View {
    All,
    Kind(Kind),
    OnPc,
}

struct Row {
    game: Game,
    on_pc: bool,
    fresh: Option<Freshness>,
}

struct UploadForm {
    open: bool,
    folder: String,
    kind: Kind,
    group: String,
}

struct OwnedList {
    loading: Option<Arc<Mutex<Option<Result<Vec<steam_fan_core::webapi::Owned>, String>>>>>,
    missing: Vec<(u32, String)>,
    shown: bool,
    error: String,
}

pub struct App {
    cfg: Config,
    draft: Config,
    rows: Vec<Row>,
    shortcut_ids: HashMap<String, u32>,
    view: View,
    search: String,
    selected: Option<String>,
    jobs: Vec<Job>,
    messages: Vec<(bool, String)>,
    settings_open: bool,
    upload: UploadForm,
    owned: OwnedList,
    shelf_online: bool,
    steam_running: bool,
    unknown_systems: Vec<String>,
    history: (String, Vec<steam_fan_core::db::Event>),
    last_check: std::time::Instant,
    /// the shelf check runs in the background: a NAS that is off can make the mount hang
    online_probe: Option<Arc<Mutex<Option<bool>>>>,
    first_frame: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> App {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let (cfg, err) = match Config::load() {
            Ok(c) => (c, None),
            Err(e) => (Config::default(), Some(format!("{e:#}"))),
        };
        let mut app = App {
            draft: cfg.clone(),
            cfg,
            rows: vec![],
            shortcut_ids: HashMap::new(),
            view: View::All,
            search: String::new(),
            selected: None,
            jobs: vec![],
            messages: vec![],
            settings_open: false,
            upload: UploadForm { open: false, folder: String::new(), kind: Kind::Fan, group: String::new() },
            owned: OwnedList { loading: None, missing: vec![], shown: false, error: String::new() },
            shelf_online: false,
            steam_running: steam::running(),
            unknown_systems: vec![],
            history: (String::new(), vec![]),
            last_check: std::time::Instant::now(),
            online_probe: None,
            first_frame: true,
        };
        if let Some(e) = err {
            app.say(false, format!("Settings file has a mistake, using defaults: {e}"));
        }
        if !Config::path().exists() {
            app.settings_open = true;
            app.say(true, "Welcome! Check where your shelf is in the settings, then press Save.".into());
        }
        app.reload();
        app
    }

    fn say(&mut self, ok: bool, msg: String) {
        self.messages.push((ok, msg));
        if self.messages.len() > 6 {
            self.messages.remove(0);
        }
    }

    /// Fill the list from the database (no shelf access).
    fn reload(&mut self) {
        let games = Db::open_default().and_then(|db| db.games()).unwrap_or_default();
        let root = self.cfg.steam_root();
        let mut local: HashMap<u32, Manifest> = HashMap::new();
        for lib in steam::libraries(&root) {
            if let Ok(rd) = std::fs::read_dir(lib.join("steamapps")) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().into_owned();
                    if n.starts_with("appmanifest_") && n.ends_with(".acf") {
                        if let Some(m) = steam::read_manifest(&e.path()) {
                            local.insert(m.appid, m);
                        }
                    }
                }
            }
        }
        self.rows = games
            .into_iter()
            .map(|g| {
                if g.kind == Kind::Steam {
                    let pc = local.get(&g.appid);
                    let shelf = Manifest { buildid: g.buildid.clone(), ..Default::default() };
                    Row { on_pc: pc.is_some(), fresh: Some(archive::freshness(Some(&shelf), pc)), game: g }
                } else {
                    Row { on_pc: library::is_installed(&self.cfg, &g), fresh: None, game: g }
                }
            })
            .collect();
        self.shortcut_ids = steam::shortcut_ids(&self.cfg);
        self.unknown_systems = steam_fan_core::shelf::unknown_systems(&self.cfg);
        self.history.0.clear(); // read again on the next frame
    }

    fn busy(&self) -> bool {
        self.jobs.iter().any(|j| !j.state.lock().unwrap().done)
    }

    fn game(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.game.id == id)
    }

    // ------------------------------------------------------------ actions

    fn start_refresh(&mut self, full: bool) {
        let cfg = self.cfg.clone();
        self.jobs.push(Job::spawn("Reading the shelf", true, move |s, _| {
            let mut db = Db::open_default()?;
            let rep = library::refresh(&cfg, &mut db, full, |k| s.lock().unwrap().status = format!("reading {k}"))?;
            let mut games = db.games()?;
            library::find_programs(&cfg, &db, &mut games, &[], full, |g| {
                s.lock().unwrap().status = format!("looking for the program in {}", g.name)
            })?;
            let mut msg = format!("{} games on the shelf", rep.games);
            if !rep.errors.is_empty() {
                msg.push_str(&format!(" ({} folders could not be read)", rep.errors.len()));
            }
            Ok(msg)
        }));
    }

    fn start_sync(&mut self) {
        let cfg = self.cfg.clone();
        self.jobs.push(Job::spawn("Updating Steam's library", true, move |s, _| {
            let mut db = Db::open_default()?;
            let out = library::sync_to_steam(&cfg, &mut db, &[], false, false, |m| s.lock().unwrap().status = m.to_string())?;
            let mut msg = format!("Steam library: {}", out.steam.summary());
            if !out.not_playable.is_empty() {
                msg.push_str(&format!("; {} games have no program and were skipped", out.not_playable.len()));
            }
            Ok(msg)
        }));
    }

    fn start_install(&mut self, g: Game) {
        let cfg = self.cfg.clone();
        let title = format!("Copying {} to this PC", g.name);
        self.jobs.push(Job::spawn(title, true, move |s, c| {
            let db = Db::open_default()?;
            let mut v = vec![g];
            library::find_programs(&cfg, &db, &mut v, &[], false, |_| {})?;
            library::install(&cfg, &db, &v[0], c, &mut progress_cb(s))?;
            Ok(format!("{} is on this PC", v[0].name))
        }));
    }

    fn start_remove(&mut self, g: Game) {
        let cfg = self.cfg.clone();
        self.jobs.push(Job::spawn(format!("Removing {}", g.name), true, move |_, _| {
            let db = Db::open_default()?;
            library::remove(&cfg, &db, &g)?;
            Ok(format!("Removed {} from this PC; saves are on the shelf", g.name))
        }));
    }

    fn start_archive(&mut self, g: Game, upload: bool) {
        let cfg = self.cfg.clone();
        let title = if upload { format!("Copying {} to the shelf", g.name) } else { format!("Installing {} from the shelf", g.name) };
        self.jobs.push(Job::spawn(title, true, move |s, c| {
            if upload {
                archive::upload(&cfg, g.appid, c, &mut progress_cb(s))?;
                Ok(format!("{} copied to the shelf", g.name))
            } else {
                archive::restore(&cfg, g.appid, None, c, &mut progress_cb(s))?;
                Ok(format!("{} installed from the shelf; start Steam to play", g.name))
            }
        }));
    }

    fn start_upload(&mut self) {
        let cfg = self.cfg.clone();
        let folder = PathBuf::from(self.upload.folder.trim());
        let (kind, group) = (self.upload.kind, self.upload.group.trim().to_string());
        if !folder.is_dir() {
            self.say(false, "Pick the game's folder first.".into());
            return;
        }
        if group.is_empty() {
            self.say(false, "Type a group (for example Sonic The Hedgehog) or system (for example wii).".into());
            return;
        }
        self.upload.open = false;
        let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.jobs.push(Job::spawn(format!("Adding {name} to the shelf"), true, move |s, c| {
            let mut db = Db::open_default()?;
            let dst = library::upload(&cfg, &db, &folder, kind, &group, c, &mut progress_cb(s))?;
            library::refresh(&cfg, &mut db, false, |_| {})?;
            Ok(format!("Added to the shelf at {}. Update Steam's library to see it there.", dst.display()))
        }));
    }

    fn start_owned_check(&mut self) {
        let slot = Arc::new(Mutex::new(None));
        let (key, id, s) = (self.cfg.steam_api_key.clone(), self.cfg.steam_id.clone(), slot.clone());
        std::thread::spawn(move || {
            let r = steam_fan_core::webapi::owned_games(&key, &id).map_err(|e| format!("{e:#}"));
            *s.lock().unwrap() = Some(r);
        });
        self.owned.loading = Some(slot);
        self.owned.shown = true;
        self.owned.error.clear();
    }

    // ------------------------------------------------------------ drawing

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("Steam-Fan").strong());
            ui.separator();
            let (c, t) = if self.shelf_online { (Color32::from_rgb(90, 200, 120), "Shelf online") } else { (Color32::from_rgb(230, 120, 90), "Shelf offline") };
            ui.colored_label(c, t.to_string()).on_hover_text(self.cfg.shelf().display().to_string());
            let (c, t) = if self.steam_running { (Color32::from_rgb(120, 170, 255), "Steam running") } else { (Color32::GRAY, "Steam closed") };
            ui.colored_label(c, t.to_string());
            ui.separator();
            let busy = self.busy();
            if ui.add_enabled(!busy && self.shelf_online, egui::Button::new("🔍 Read shelf")).on_hover_text("Look for new or changed games on the shelf").clicked() {
                self.start_refresh(false);
            }
            let sync = ui.add_enabled(!busy && !self.steam_running && self.shelf_online, egui::Button::new("⟳ Update Steam library"));
            let sync = if self.steam_running { sync.on_disabled_hover_text("Quit Steam first (Steam > Exit). Steam overwrites its library when it closes.") } else { sync.on_hover_text("Add new shelf games to Steam, update changed ones, remove ones that left the shelf") };
            if sync.clicked() {
                self.start_sync();
            }
            if ui.add_enabled(self.shelf_online, egui::Button::new("+ Add game to shelf")).clicked() {
                self.upload.open = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⚙ Settings").clicked() {
                    self.draft = self.cfg.clone();
                    self.settings_open = true;
                }
            });
        });
    }

    fn side(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let count = |f: &dyn Fn(&Row) -> bool| self.rows.iter().filter(|r| f(r)).count();
        let mut entry = |ui: &mut egui::Ui, v: View, label: &str, n: usize| {
            if ui.selectable_label(self.view == v, format!("{label}  ({n})")).clicked() {
                self.view = v;
            }
        };
        let all = count(&|_| true);
        let on_pc = count(&|r| r.on_pc);
        let per: Vec<(Kind, usize)> = Kind::all().into_iter().map(|k| (k, count(&|r: &Row| r.game.kind == k))).collect();
        entry(ui, View::All, "Everything", all);
        ui.separator();
        for (k, n) in per {
            entry(ui, View::Kind(k), k.label(), n);
        }
        ui.separator();
        entry(ui, View::OnPc, "On this PC", on_pc);
        ui.add_space(12.0);
        if self.view == View::Kind(Kind::Steam) {
            if ui.button("Owned but not archived…").on_hover_text("Needs your Steam API key in the settings").clicked() {
                self.start_owned_check();
            }
        }
        let unknown = self.unknown_systems.clone();
        if self.shelf_online && !unknown.is_empty() && matches!(self.view, View::Kind(Kind::Rom) | View::All) {
            ui.add_space(8.0);
            ui.small(format!("No emulator set for: {}", unknown.join(", "))).on_hover_text("Add them under Settings > Emulators");
        }
    }

    fn visible(&self) -> Vec<usize> {
        let q = self.search.to_lowercase();
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| match self.view {
                View::All => true,
                View::Kind(k) => r.game.kind == k,
                View::OnPc => r.on_pc,
            })
            .filter(|(_, r)| q.is_empty() || r.game.name.to_lowercase().contains(&q) || r.game.group.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    fn status_text(r: &Row) -> (String, Color32) {
        let g = &r.game;
        if g.kind == Kind::Steam {
            let f = r.fresh.unwrap_or(Freshness::Unknown);
            let c = match f {
                Freshness::Same => Color32::from_rgb(90, 200, 120),
                Freshness::ShelfNewer | Freshness::PcNewer => Color32::from_rgb(240, 190, 80),
                _ => Color32::GRAY,
            };
            return (f.label().to_string(), c);
        }
        if g.program.is_empty() {
            let why = if g.kind == Kind::Rom { "no game file found" } else { "no program found" };
            return (why.into(), Color32::from_rgb(230, 120, 90));
        }
        if r.on_pc {
            ("on this PC".into(), Color32::from_rgb(90, 200, 120))
        } else {
            ("on shelf".into(), Color32::GRAY)
        }
    }

    fn table(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search by name or group").desired_width(320.0));
            if !self.search.is_empty() && ui.small_button("✕").clicked() {
                self.search.clear();
            }
        });
        ui.add_space(4.0);
        let idx = self.visible();
        if idx.is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                if self.rows.is_empty() {
                    ui.label("Nothing here yet. Press \"Read shelf\" to look at your shelf.");
                } else {
                    ui.label("No games match.");
                }
            });
            return;
        }
        let mut clicked = None;
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::remainder().at_least(220.0).clip(true))
            .column(Column::initial(180.0).clip(true))
            .column(Column::initial(120.0))
            .column(Column::initial(130.0))
            .column(Column::initial(80.0))
            .header(22.0, |mut h| {
                for t in ["Name", "Group", "Type", "Status", "Size"] {
                    h.col(|ui| {
                        ui.strong(t);
                    });
                }
            })
            .body(|body| {
                body.rows(24.0, idx.len(), |mut row| {
                    let r = &self.rows[idx[row.index()]];
                    let sel = self.selected.as_deref() == Some(r.game.id.as_str());
                    row.col(|ui| {
                        if ui.selectable_label(sel, &r.game.name).clicked() {
                            clicked = Some(r.game.id.clone());
                        }
                    });
                    row.col(|ui| {
                        ui.label(&r.game.group);
                    });
                    row.col(|ui| {
                        ui.label(match (r.game.kind, r.game.platform) {
                            (Kind::Steam, _) => "Steam",
                            (Kind::Rom, _) => "Emulator",
                            (_, Platform::Linux) => "Linux",
                            _ => "Windows (Proton)",
                        });
                    });
                    row.col(|ui| {
                        let (t, c) = Self::status_text(r);
                        ui.colored_label(c, t);
                    });
                    row.col(|ui| {
                        if r.game.size > 0 {
                            ui.label(transfer::human(r.game.size));
                        }
                    });
                });
            });
        if let Some(id) = clicked {
            self.selected = Some(id);
        }
    }

    fn details(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected.clone() else {
            ui.add_space(20.0);
            ui.label("Pick a game to see what you can do with it.");
            return;
        };
        let Some(r) = self.game(&id) else {
            self.selected = None;
            return;
        };
        let g = r.game.clone();
        let on_pc = r.on_pc;
        let fresh = r.fresh;
        ui.heading(&g.name);
        ui.label(RichText::new(format!("{}  ·  {}", g.kind.label(), g.group)).weak());
        ui.add_space(6.0);
        let busy = self.busy();
        let steam_id = self.shortcut_ids.get(&g.id).copied();

        if g.kind == Kind::Steam {
            ui.label(format!("App id {}   ·   shelf build {}", g.appid, g.buildid));
            if let Some(f) = fresh {
                ui.label(format!("Status: {}", f.label()));
            }
            ui.add_space(6.0);
            let can_restore = matches!(fresh, Some(Freshness::OnlyShelf | Freshness::ShelfNewer | Freshness::Unknown));
            let b = ui.add_enabled(!busy && !self.steam_running && can_restore && self.shelf_online, egui::Button::new("⬇ Install from shelf"));
            let b = if self.steam_running { b.on_disabled_hover_text("Quit Steam first") } else { b.on_hover_text("Copy the shelf version into Steam's library, for when Steam's servers are down") };
            if b.clicked() {
                self.start_archive(g.clone(), false);
            }
            if ui
                .add_enabled(!busy && matches!(fresh, Some(Freshness::PcNewer)) && self.shelf_online, egui::Button::new("⬆ Copy to shelf"))
                .on_hover_text("This PC has a newer version: put it on the shelf too")
                .clicked()
            {
                self.start_archive(g.clone(), true);
            }
            if ui.button("▶ Open in Steam").clicked() {
                util::open(&format!("steam://nav/games/details/{}", g.appid));
            }
        } else {
            let local = g.local_path(&self.cfg);
            if !g.program.is_empty() {
                ui.label(format!("Starts: {}", g.program));
            }
            if g.kind == Kind::Rom {
                match steam_fan_core::launch::rom_command(&self.cfg, &g) {
                    Ok(c) => ui.small(util::join_words(&c)),
                    Err(e) => ui.colored_label(Color32::from_rgb(230, 120, 90), format!("{e:#}")),
                };
            }
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if !on_pc {
                    if ui.add_enabled(!busy && self.shelf_online, egui::Button::new("⬇ Copy to this PC")).clicked() {
                        self.start_install(g.clone());
                    }
                } else if ui.add_enabled(!busy, egui::Button::new("🗑 Remove from PC")).on_hover_text("Saves are backed up to the shelf first. It stays in Steam and copies again when you press Play.").clicked() {
                    self.start_remove(g.clone());
                }
                let play = ui.add_enabled(steam_id.is_some(), egui::Button::new("▶ Play in Steam"));
                let play = if steam_id.is_none() { play.on_disabled_hover_text("Not in Steam's library yet: press Update Steam library") } else { play };
                if play.clicked() {
                    util::open(&steam::run_url(steam_id.unwrap()));
                }
            });
            ui.horizontal_wrapped(|ui| {
                if on_pc && ui.button("📁 Folder on PC").clicked() {
                    util::open(&local.to_string_lossy());
                }
                if self.shelf_online && ui.button("📁 Folder on shelf").clicked() {
                    let p = g.shelf_path(&self.cfg);
                    let p = if p.is_file() { p.parent().unwrap().to_path_buf() } else { p };
                    util::open(&p.to_string_lossy());
                }
                if self.shelf_online && ui.button("💾 Saves on shelf").clicked() {
                    util::open(&g.save_path(&self.cfg).to_string_lossy());
                }
            });
        }
        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("History").strong());
        if self.history.0 != g.id {
            let ev = Db::open_default().and_then(|db| db.events(Some(&g.id), 8)).unwrap_or_default();
            self.history = (g.id.clone(), ev);
        }
        if self.history.1.is_empty() {
            ui.small("Nothing yet.");
        }
        for e in &self.history.1 {
            ui.small(format!("{}  {}", e.ts, e.msg));
        }
    }

    fn jobs_panel(&mut self, ui: &mut egui::Ui) {
        let mut reload = false;
        let mut finished: Vec<(bool, String)> = vec![];
        self.jobs.retain(|j| {
            let s = j.state.lock().unwrap();
            if s.done {
                reload |= j.reload;
                match &s.error {
                    Some(e) => finished.push((false, format!("{}: {e}", j.title))),
                    None if !s.result.is_empty() => finished.push((true, s.result.clone())),
                    None => {}
                }
                return false;
            }
            true
        });
        for (ok, m) in finished {
            self.say(ok, m);
        }
        if reload {
            self.reload();
        }
        for j in &self.jobs {
            let s = j.state.lock().unwrap();
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(&j.title);
                if let Some(p) = &s.progress {
                    ui.add(egui::ProgressBar::new(p.fraction()).show_percentage().desired_width(260.0));
                    ui.small(format!("{} of {}", transfer::human(p.done_bytes), transfer::human(p.total_bytes)));
                } else if !s.status.is_empty() {
                    ui.small(&s.status);
                }
                if ui.small_button("Stop").clicked() {
                    j.cancel.cancel();
                }
            });
        }
        let mut clear = false;
        for (ok, m) in self.messages.iter().rev().take(3) {
            let c = if *ok { Color32::from_rgb(140, 200, 150) } else { Color32::from_rgb(235, 130, 100) };
            ui.horizontal(|ui| {
                ui.colored_label(c, m);
            });
        }
        if !self.messages.is_empty() && ui.small_button("Clear messages").clicked() {
            clear = true;
        }
        if clear {
            self.messages.clear();
        }
    }

    fn upload_window(&mut self, ctx: &egui::Context) {
        if !self.upload.open {
            return;
        }
        let mut open = true;
        let mut go = false;
        egui::Window::new("Add a game to the shelf").open(&mut open).collapsible(false).default_width(480.0).show(ctx, |ui| {
            ui.label("Copies a game folder from this PC onto the shelf. The original stays where it is.");
            ui.add_space(6.0);
            egui::Grid::new("up").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Game folder");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.upload.folder).desired_width(300.0));
                    if ui.button("Browse…").clicked() {
                        if let Some(p) = util::pick_folder("Pick the game's folder", &steam_fan_core::config::home().to_string_lossy()) {
                            self.upload.folder = p;
                        }
                    }
                });
                ui.end_row();
                ui.label("Kind");
                egui::ComboBox::from_id_salt("kind").selected_text(self.upload.kind.label()).show_ui(ui, |ui| {
                    for k in [Kind::Fan, Kind::NonSteam, Kind::Rom] {
                        ui.selectable_value(&mut self.upload.kind, k, k.label());
                    }
                });
                ui.end_row();
                ui.label(if self.upload.kind == Kind::Rom { "System" } else { "Group" });
                let groups: Vec<String> = {
                    let mut g: Vec<String> = self
                        .rows
                        .iter()
                        .filter(|r| r.game.kind == self.upload.kind)
                        .map(|r| if r.game.kind == Kind::Rom { r.game.system.clone() } else { r.game.rel.split('/').next().unwrap_or("").to_string() })
                        .collect();
                    g.sort();
                    g.dedup();
                    g
                };
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.upload.group).desired_width(200.0).hint_text("e.g. Sonic The Hedgehog"));
                    egui::ComboBox::from_id_salt("grp").selected_text("pick").show_ui(ui, |ui| {
                        for g in groups {
                            if ui.selectable_label(self.upload.group == g, &g).clicked() {
                                self.upload.group = g;
                            }
                        }
                    });
                });
                ui.end_row();
            });
            ui.add_space(8.0);
            if ui.button("Add to shelf").clicked() {
                go = true;
            }
        });
        if !open {
            self.upload.open = false;
        }
        if go {
            self.start_upload();
        }
    }

    fn owned_window(&mut self, ctx: &egui::Context) {
        if !self.owned.shown {
            return;
        }
        let got = self.owned.loading.as_ref().and_then(|slot| slot.lock().unwrap().take());
        {
            if let Some(r) = got {
                match r {
                    Ok(list) => {
                        let have: std::collections::HashSet<u32> = self.rows.iter().filter(|r| r.game.kind == Kind::Steam).map(|r| r.game.appid).collect();
                        self.owned.missing = list.into_iter().filter(|o| !have.contains(&o.appid)).map(|o| (o.appid, o.name)).collect();
                    }
                    Err(e) => self.owned.error = e,
                }
                self.owned.loading = None;
            }
        }
        let mut open = true;
        egui::Window::new("Owned but not on the shelf").open(&mut open).default_size([420.0, 420.0]).show(ctx, |ui| {
            if self.owned.loading.is_some() {
                ui.spinner();
                ui.label("Asking Steam…");
            } else if !self.owned.error.is_empty() {
                ui.colored_label(Color32::from_rgb(235, 130, 100), &self.owned.error);
            } else {
                ui.label(format!("{} games you own aren't archived yet. The NAS adds them on its next daily run.", self.owned.missing.len()));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (id, n) in &self.owned.missing {
                        ui.label(format!("{id:<9}  {n}"));
                    }
                });
            }
        });
        if !open {
            self.owned.shown = false;
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = true;
        let mut save = false;
        egui::Window::new("Settings").open(&mut open).default_width(640.0).default_height(560.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Shelf");
                ui.label("Where all your games are stored. Usually the NAS share mounted on this PC, but any folder works.");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.draft.shelf).desired_width(380.0));
                    if ui.button("Browse…").clicked() {
                        if let Some(p) = util::pick_folder("Pick the shelf folder", &self.draft.shelf) {
                            self.draft.shelf = p;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui.small_button("NAS (/mnt/nas-games)").clicked() {
                        self.draft.shelf = "/mnt/nas-games".into();
                    }
                    if ui.small_button("Folder on this PC (~/Games/Shelf)").clicked() {
                        self.draft.shelf = "~/Games/Shelf".into();
                    }
                });
                let online = steam_fan_core::config::shelf_online(&steam_fan_core::config::expand(&self.draft.shelf));
                ui.small(if online { "✔ reachable" } else { "✖ not reachable (empty, missing, or the NAS is off)" });
                ui.small("Inside it: fangames/, nonsteam/, roms/, steam/ (the archive), saves/.");

                ui.add_space(10.0);
                ui.heading("Box (this PC)");
                egui::Grid::new("box").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    for (label, v) in [
                        ("Fan games go to", &mut self.draft.fan_local),
                        ("Non-Steam games go to", &mut self.draft.nonsteam_local),
                        ("Emulator games go to", &mut self.draft.rom_local),
                        ("Steam folder", &mut self.draft.steam_root),
                    ] {
                        ui.label(label);
                        ui.add(egui::TextEdit::singleline(v).desired_width(360.0));
                        ui.end_row();
                    }
                    ui.label("Proton for Windows games");
                    ui.add(egui::TextEdit::singleline(&mut self.draft.proton).hint_text("empty: Steam's default").desired_width(220.0));
                    ui.end_row();
                });

                ui.add_space(10.0);
                ui.heading("Steam account (optional)");
                ui.label("Lets the app list the games you own that aren't archived yet.");
                egui::Grid::new("acct").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Web API key");
                    ui.add(egui::TextEdit::singleline(&mut self.draft.steam_api_key).password(true).desired_width(300.0));
                    ui.end_row();
                    ui.label("Steam id (64-bit)");
                    ui.add(egui::TextEdit::singleline(&mut self.draft.steam_id).desired_width(300.0));
                    ui.end_row();
                });
                ui.hyperlink_to("Get a key at steamcommunity.com/dev/apikey", "https://steamcommunity.com/dev/apikey");

                ui.add_space(10.0);
                ui.heading("Emulators");
                ui.label("One entry per system folder in roms/. In the command, {rom} is the game file and {dir} the game folder.");
                let mut remove = None;
                for (i, e) in self.draft.emulators.iter_mut().enumerate() {
                    ui.push_id(i, |ui| emulator_editor(ui, e, &mut remove, i));
                }
                if let Some(i) = remove {
                    self.draft.emulators.remove(i);
                }
                ui.horizontal(|ui| {
                    if ui.button("Add a system").clicked() {
                        self.draft.emulators.push(Emulator { system: "new".into(), ..Default::default() });
                    }
                    if ui.button("Reset to the examples").clicked() {
                        self.draft.emulators = default_emulators();
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Save").strong()).clicked() {
                    save = true;
                }
                ui.small(Config::path().display().to_string());
            });
        });
        if save {
            match self.draft.save() {
                Ok(()) => {
                    let changed_shelf = self.draft.shelf != self.cfg.shelf;
                    self.cfg = self.draft.clone();
                    self.say(true, "Settings saved.".into());
                    self.settings_open = false;
                    self.shelf_online = self.cfg.shelf_online();
                    if changed_shelf || self.rows.is_empty() {
                        if self.shelf_online {
                            self.start_refresh(true);
                        }
                    } else {
                        self.reload();
                    }
                }
                Err(e) => self.say(false, format!("Settings not saved: {e:#}")),
            }
        } else if !open {
            self.settings_open = false;
        }
    }
}

fn emulator_editor(ui: &mut egui::Ui, e: &mut Emulator, remove: &mut Option<usize>, i: usize) {
    egui::CollapsingHeader::new(RichText::new(&e.system).strong()).default_open(false).show(ui, |ui| {
        egui::Grid::new("emu").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("System folder");
            ui.text_edit_singleline(&mut e.system);
            ui.end_row();
            ui.label("Games subfolder");
            ui.add(egui::TextEdit::singleline(&mut e.games_dir).hint_text("empty: the system folder itself"));
            ui.end_row();
            ui.label("File types");
            let mut ext = e.extensions.join(", ");
            if ui.add(egui::TextEdit::singleline(&mut ext).desired_width(320.0)).changed() {
                e.extensions = ext.split(',').map(|s| s.trim().trim_start_matches('.').to_string()).filter(|s| !s.is_empty()).collect();
            }
            ui.end_row();
            ui.label("Command");
            let mut cmd = util::join_words(&e.command);
            if ui.add(egui::TextEdit::singleline(&mut cmd).desired_width(320.0)).changed() {
                e.command = util::split_words(&cmd);
            }
            ui.end_row();
            ui.label("Save folders");
            let mut saves = e.save_dirs.join("\n");
            if ui.add(egui::TextEdit::multiline(&mut saves).desired_rows(2).desired_width(320.0)).changed() {
                e.save_dirs = saves.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            }
            ui.end_row();
            ui.label("Not games if the name has");
            let mut skip = e.skip.join(", ");
            if ui.add(egui::TextEdit::singleline(&mut skip).desired_width(320.0)).changed() {
                e.skip = skip.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            }
            ui.end_row();
        });
        if ui.small_button("Remove this system").clicked() {
            *remove = Some(i);
        }
    });
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.first_frame {
            self.shelf_online = self.cfg.shelf_online();
            if self.shelf_online && Config::path().exists() {
                self.start_refresh(false);
            }
            self.first_frame = false;
        }
        if let Some(probe) = &self.online_probe {
            let got = probe.lock().unwrap().take();
            if let Some(online) = got {
                if online && !self.shelf_online {
                    self.say(true, "The shelf is online.".into());
                }
                self.shelf_online = online;
                self.online_probe = None;
            }
        }
        if self.online_probe.is_none() && self.last_check.elapsed().as_secs() >= 3 {
            self.last_check = std::time::Instant::now();
            self.steam_running = steam::running();
            let slot = Arc::new(Mutex::new(None));
            let (s, shelf) = (slot.clone(), self.cfg.shelf());
            std::thread::spawn(move || {
                let r = steam_fan_core::config::shelf_online(&shelf);
                *s.lock().unwrap() = Some(r);
            });
            self.online_probe = Some(slot);
        }
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            self.top_bar(ui);
            ui.add_space(4.0);
        });
        egui::TopBottomPanel::bottom("jobs").show(ctx, |ui| {
            ui.add_space(3.0);
            self.jobs_panel(ui);
            ui.add_space(3.0);
        });
        egui::SidePanel::left("side").resizable(false).default_width(190.0).show(ctx, |ui| self.side(ui));
        egui::SidePanel::right("details").resizable(true).default_width(330.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.details(ui));
        });
        egui::CentralPanel::default().show(ctx, |ui| self.table(ui));
        self.upload_window(ctx);
        self.settings_window(ctx);
        self.owned_window(ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(if self.busy() { 150 } else { 1000 }));
    }
}
