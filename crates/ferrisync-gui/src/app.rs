//! Main egui application — pick source + destination folders, then sync.

use std::path::PathBuf;

use eframe::egui::{self, Align, Layout, RichText, ScrollArea};
use ferrisync_core::config::{default_gui_session_path, Config};

use crate::theme::{self, AMBER, BG, GREEN, MUTED, PANEL, PANEL_EDGE, RED, TEXT};
use crate::worker::{FolderSession, Job, JobEvent, WorkerHandle};

pub struct FerrisyncApp {
    source: String,
    destination: String,
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

        let mut app = Self {
            source: String::new(),
            destination: String::new(),
            status_line: "Choose a source folder and a destination folder, then Sync.".into(),
            log_lines: vec![(
                LogKind::Info,
                "Pick source (original) and destination (copy target), then press Sync.".into(),
            )],
            busy: false,
            force_enable_retention: false,
            confirm_live_cleanup: false,
            worker: WorkerHandle::spawn(),
        };
        app.restore_last_session();
        app
    }

    fn restore_last_session(&mut self) {
        let path = default_gui_session_path();
        if !path.exists() {
            return;
        }
        match Config::load(&path) {
            Ok(cfg) => {
                if let Some(pair) = cfg.pairs.first() {
                    self.source = pair.source.display().to_string();
                    self.destination = pair.destination.display().to_string();
                    self.status_line = format!(
                        "Restored last folders. State DB: {}",
                        cfg.state_db_path().display()
                    );
                    self.push_log(LogKind::Ok, self.status_line.clone());
                }
            }
            Err(e) => {
                self.push_log(
                    LogKind::Warn,
                    format!("Could not restore last session: {e}"),
                );
            }
        }
    }

    fn session(&self) -> FolderSession {
        FolderSession {
            source: PathBuf::from(self.source.trim()),
            destination: PathBuf::from(self.destination.trim()),
        }
    }

    fn folders_ready(&self) -> bool {
        !self.source.trim().is_empty() && !self.destination.trim().is_empty()
    }

    fn push_log(&mut self, kind: LogKind, msg: String) {
        self.log_lines.push((kind, msg));
        if self.log_lines.len() > 2000 {
            let drain = self.log_lines.len() - 2000;
            self.log_lines.drain(0..drain);
        }
    }

    fn submit(&mut self, job: Job) {
        if self.busy {
            self.push_log(
                LogKind::Warn,
                "Busy — wait for the current job to finish.".into(),
            );
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

    fn pick_folder(title: &str) -> Option<String> {
        rfd::FileDialog::new()
            .set_title(title)
            .pick_folder()
            .map(|p| p.display().to_string())
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
                    ui.label(
                        RichText::new("  pick folders → sync")
                            .color(MUTED)
                            .italics(),
                    );
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
                        ui.label(RichText::new("FOLDERS").color(AMBER).small().strong());
                        ui.add_space(6.0);

                        ui.label(RichText::new("Source (original files)").color(MUTED));
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.source)
                                    .desired_width(640.0)
                                    .hint_text("folder to copy from"),
                            );
                            if ui.button("Browse…").clicked() {
                                if let Some(path) = Self::pick_folder("Choose source folder") {
                                    self.source = path;
                                }
                            }
                        });

                        ui.add_space(8.0);
                        ui.label(RichText::new("Destination (NAS / copy target)").color(MUTED));
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.destination)
                                    .desired_width(640.0)
                                    .hint_text("folder to copy to"),
                            );
                            if ui.button("Browse…").clicked() {
                                if let Some(path) = Self::pick_folder("Choose destination folder") {
                                    self.destination = path;
                                }
                            }
                        });

                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(
                                "Config, SQLite state, and audit log are created automatically under AppData/Ferrisync.",
                            )
                            .color(MUTED)
                            .small(),
                        );
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
                            let can_run = !self.busy && self.folders_ready();
                            if ui
                                .add_enabled(can_run, egui::Button::new("▶  Sync"))
                                .on_hover_text("Compare + copy + BLAKE3 verify")
                                .clicked()
                            {
                                self.submit(Job::Sync(self.session()));
                            }
                            if ui
                                .add_enabled(can_run, egui::Button::new("Status"))
                                .clicked()
                            {
                                self.submit(Job::Status(self.session()));
                            }
                            if ui
                                .add_enabled(can_run, egui::Button::new("Cleanup (dry-run)"))
                                .on_hover_text("Never touches source files")
                                .clicked()
                            {
                                self.submit(Job::Cleanup {
                                    session: self.session(),
                                    force_dry_run: true,
                                    force_enabled: true,
                                });
                            }
                            if ui
                                .add_enabled(
                                    can_run,
                                    egui::Button::new("Purge quarantine (dry-run)"),
                                )
                                .clicked()
                            {
                                self.submit(Job::Purge {
                                    session: self.session(),
                                    force_dry_run: true,
                                });
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
                            && self.folders_ready()
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
                            self.submit(Job::Cleanup {
                                session: self.session(),
                                force_dry_run: false,
                                force_enabled: true,
                            });
                            self.confirm_live_cleanup = false;
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
