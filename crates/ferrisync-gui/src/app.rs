//! Main egui application state and layout.

use std::path::PathBuf;

use eframe::egui::{self, Align, Layout, RichText, ScrollArea, Sense};
use ferrisync_core::config::Config;

use crate::theme::{self, AMBER, BG, GREEN, MUTED, PANEL, PANEL_EDGE, RED, TEXT};
use crate::worker::{Job, JobEvent, WorkerHandle};

pub struct FerrisyncApp {
    config_path: String,
    pair_ids: Vec<String>,
    selected_pair: usize,
    pair_source: String,
    pair_dest: String,
    retention_summary: String,
    status_line: String,
    log_lines: Vec<(LogKind, String)>,
    busy: bool,
    force_enable_retention: bool,
    confirm_live_cleanup: bool,
    worker: WorkerHandle,
}

#[derive(Clone, Copy)]
enum LogKind {
    Info,
    Ok,
    Warn,
    Err,
}

impl FerrisyncApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);

        let default_config = find_default_config();
        let mut app = Self {
            config_path: default_config,
            pair_ids: Vec::new(),
            selected_pair: 0,
            pair_source: String::new(),
            pair_dest: String::new(),
            retention_summary: String::new(),
            status_line: "Load a config to begin.".into(),
            log_lines: vec![(
                LogKind::Info,
                "Ferrisync GUI ready — load config, pick a pair, run Sync.".into(),
            )],
            busy: false,
            force_enable_retention: false,
            confirm_live_cleanup: false,
            worker: WorkerHandle::spawn(),
        };
        let _ = app.reload_config();
        app
    }

    fn reload_config(&mut self) -> Result<(), String> {
        let path = PathBuf::from(self.config_path.trim());
        if self.config_path.trim().is_empty() {
            return Err("config path is empty".into());
        }
        let config = Config::load(&path).map_err(|e| e.to_string())?;
        self.pair_ids = config.pairs.iter().map(|p| p.id.clone()).collect();
        if self.selected_pair >= self.pair_ids.len() {
            self.selected_pair = 0;
        }
        self.refresh_pair_view(&config);
        self.status_line = format!(
            "Loaded {} — {} pair(s), state_db={}",
            path.display(),
            self.pair_ids.len(),
            config.state_db_path().display()
        );
        self.push_log(LogKind::Ok, self.status_line.clone());
        Ok(())
    }

    fn refresh_pair_view(&mut self, config: &Config) {
        if let Some(id) = self.pair_ids.get(self.selected_pair).cloned() {
            if let Ok(pair) = config.pair(&id) {
                self.pair_source = pair.source.display().to_string();
                self.pair_dest = pair.destination.display().to_string();
                let r = config.effective_retention(pair);
                self.retention_summary = format!(
                    "enabled={}  dry_run={}  days={}  mode={:?}  quarantine={}",
                    r.enabled,
                    r.dry_run,
                    r.retention_days,
                    r.mode,
                    r.quarantine_dir
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "(none)".into())
                );
            }
        } else {
            self.pair_source.clear();
            self.pair_dest.clear();
            self.retention_summary.clear();
        }
    }

    fn push_log(&mut self, kind: LogKind, msg: String) {
        self.log_lines.push((kind, msg));
        if self.log_lines.len() > 2000 {
            let drain = self.log_lines.len() - 2000;
            self.log_lines.drain(0..drain);
        }
    }

    fn selected_pair_id(&self) -> Option<String> {
        self.pair_ids.get(self.selected_pair).cloned()
    }

    fn submit(&mut self, job: Job) {
        if self.busy {
            self.push_log(LogKind::Warn, "Busy — wait for the current job to finish.".into());
            return;
        }
        match self.worker.submit(job) {
            Ok(()) => {
                self.busy = true;
                self.status_line = "Running…".into();
            }
            Err(e) => self.push_log(LogKind::Err, e),
        }
    }

    fn poll_worker(&mut self) {
        while let Ok(ev) = self.worker.event_rx.try_recv() {
            match ev {
                JobEvent::Log(msg) => {
                    let kind = if msg.contains("error") || msg.starts_with("error") {
                        LogKind::Err
                    } else if msg.contains("done") || msg.contains("SYNC done") {
                        LogKind::Ok
                    } else if msg.contains("skipped") || msg.contains("would_") {
                        LogKind::Warn
                    } else {
                        LogKind::Info
                    };
                    self.push_log(kind, msg);
                }
                JobEvent::Done { ok } => {
                    self.busy = false;
                    self.status_line = if ok {
                        "Idle — last job succeeded.".into()
                    } else {
                        "Idle — last job failed (see log).".into()
                    };
                    self.push_log(
                        if ok { LogKind::Ok } else { LogKind::Err },
                        self.status_line.clone(),
                    );
                }
            }
        }
    }
}

impl eframe::App for FerrisyncApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker();
        if self.busy {
            ctx.request_repaint();
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(16.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("FERRISYNC").color(AMBER).strong());
                    ui.label(RichText::new("  field sync console").color(MUTED).italics());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let badge = if self.busy {
                            RichText::new("● RUNNING").color(AMBER)
                        } else {
                            RichText::new("● IDLE").color(GREEN)
                        };
                        ui.label(badge);
                    });
                });
                ui.add_space(4.0);
                ui.label(RichText::new(&self.status_line).color(MUTED));
                ui.add_space(10.0);

                egui::Frame::new()
                    .fill(PANEL)
                    .stroke(egui::Stroke::new(1.0, PANEL_EDGE))
                    .inner_margin(14.0)
                    .corner_radius(4.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new("CONFIG").color(AMBER).small().strong());
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.config_path)
                                    .desired_width(560.0)
                                    .hint_text("path to ferrisync.toml"),
                            );
                            if ui.button("Browse…").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("TOML", &["toml"])
                                    .pick_file()
                                {
                                    self.config_path = path.display().to_string();
                                }
                            }
                            if ui.button("Reload").clicked() {
                                if let Err(e) = self.reload_config() {
                                    self.push_log(LogKind::Err, format!("reload failed: {e}"));
                                    self.status_line = format!("Reload failed: {e}");
                                }
                            }
                        });

                        ui.add_space(8.0);
                        ui.label(RichText::new("FOLDER PAIR").color(AMBER).small().strong());
                        if self.pair_ids.is_empty() {
                            ui.label(RichText::new("No pairs loaded.").color(MUTED));
                        } else {
                            let pair_ids = self.pair_ids.clone();
                            let mut changed = false;
                            egui::ComboBox::from_id_salt("pair_select")
                                .selected_text(
                                    pair_ids
                                        .get(self.selected_pair)
                                        .cloned()
                                        .unwrap_or_default(),
                                )
                                .show_ui(ui, |ui| {
                                    for (i, id) in pair_ids.iter().enumerate() {
                                        if ui
                                            .selectable_value(&mut self.selected_pair, i, id)
                                            .changed()
                                        {
                                            changed = true;
                                        }
                                    }
                                });
                            if changed {
                                if let Ok(cfg) =
                                    Config::load(PathBuf::from(self.config_path.trim()))
                                {
                                    self.refresh_pair_view(&cfg);
                                }
                            }
                            ui.add_space(4.0);
                            path_row(ui, "source", &self.pair_source);
                            path_row(ui, "dest  ", &self.pair_dest);
                            ui.label(RichText::new(&self.retention_summary).color(MUTED).small());
                        }
                    });

                ui.add_space(12.0);

                egui::Frame::new()
                    .fill(PANEL)
                    .stroke(egui::Stroke::new(1.0, PANEL_EDGE))
                    .inner_margin(14.0)
                    .corner_radius(4.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new("ACTIONS").color(AMBER).small().strong());
                        ui.horizontal_wrapped(|ui| {
                            let can_run = !self.busy && self.selected_pair_id().is_some();
                            if ui
                                .add_enabled(can_run, egui::Button::new("▶  Sync"))
                                .on_hover_text("Compare + copy + BLAKE3 verify")
                                .clicked()
                            {
                                if let Some(pair_id) = self.selected_pair_id() {
                                    self.submit(Job::Sync {
                                        config_path: PathBuf::from(self.config_path.trim()),
                                        pair_id,
                                    });
                                }
                            }
                            if ui
                                .add_enabled(can_run, egui::Button::new("Status"))
                                .clicked()
                            {
                                if let Some(pair_id) = self.selected_pair_id() {
                                    self.submit(Job::Status {
                                        config_path: PathBuf::from(self.config_path.trim()),
                                        pair_id,
                                    });
                                }
                            }
                            if ui
                                .add_enabled(can_run, egui::Button::new("Cleanup (dry-run)"))
                                .on_hover_text("Always dry-run — never touches source files")
                                .clicked()
                            {
                                if let Some(pair_id) = self.selected_pair_id() {
                                    self.submit(Job::Cleanup {
                                        config_path: PathBuf::from(self.config_path.trim()),
                                        pair_id,
                                        force_dry_run: true,
                                        force_enabled: true,
                                    });
                                }
                            }
                            if ui
                                .add_enabled(can_run, egui::Button::new("Purge quarantine (dry-run)"))
                                .clicked()
                            {
                                if let Some(pair_id) = self.selected_pair_id() {
                                    self.submit(Job::Purge {
                                        config_path: PathBuf::from(self.config_path.trim()),
                                        pair_id,
                                        force_dry_run: true,
                                    });
                                }
                            }
                        });

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("DESTRUCTIVE — live cleanup")
                                .color(RED)
                                .small()
                                .strong(),
                        );
                        ui.checkbox(
                            &mut self.force_enable_retention,
                            "Force enable retention for this run",
                        );
                        ui.checkbox(
                            &mut self.confirm_live_cleanup,
                            "I understand this may quarantine/delete source files",
                        );
                        let live_ok = !self.busy
                            && self.selected_pair_id().is_some()
                            && self.force_enable_retention
                            && self.confirm_live_cleanup;
                        if ui
                            .add_enabled(
                                live_ok,
                                egui::Button::new(
                                    RichText::new("Cleanup LIVE (no dry-run)").color(RED),
                                ),
                            )
                            .clicked()
                        {
                            if let Some(pair_id) = self.selected_pair_id() {
                                self.submit(Job::Cleanup {
                                    config_path: PathBuf::from(self.config_path.trim()),
                                    pair_id,
                                    force_dry_run: false,
                                    force_enabled: true,
                                });
                                self.confirm_live_cleanup = false;
                            }
                        }
                    });

                ui.add_space(12.0);

                egui::Frame::new()
                    .fill(PANEL)
                    .stroke(egui::Stroke::new(1.0, PANEL_EDGE))
                    .inner_margin(10.0)
                    .corner_radius(4.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("LOG").color(AMBER).small().strong());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("Clear").clicked() {
                                    self.log_lines.clear();
                                }
                            });
                        });
                        ui.add_space(4.0);
                        ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .stick_to_bottom(true)
                            .max_height(ui.available_height())
                            .show(ui, |ui| {
                                ui.style_mut().override_font_id =
                                    Some(egui::FontId::monospace(13.0));
                                for (kind, line) in &self.log_lines {
                                    let color = match kind {
                                        LogKind::Info => TEXT,
                                        LogKind::Ok => GREEN,
                                        LogKind::Warn => AMBER,
                                        LogKind::Err => RED,
                                    };
                                    ui.label(RichText::new(line).color(color));
                                }
                            });
                    });
            });
    }
}

fn path_row(ui: &mut egui::Ui, label: &str, path: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(MUTED).monospace());
        ui.add(
            egui::Label::new(RichText::new(path).color(TEXT).monospace())
                .sense(Sense::click())
                .truncate(),
        );
    });
}

fn find_default_config() -> String {
    let candidates = [
        "ferrisync.toml",
        "config.example.toml",
        "crates/../ferrisync.toml",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return p
                .canonicalize()
                .unwrap_or(p)
                .display()
                .to_string();
        }
    }
    "ferrisync.toml".into()
}
