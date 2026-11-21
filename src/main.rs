#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod server;

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{Receiver, channel};

use eframe::egui;
use qrcode::{Color, QrCode};

struct App {
    url: String,
    save_dir: PathBuf,
    qr: Vec<Vec<bool>>,
    rx: Receiver<server::Received>,
    received: Vec<server::Received>,
    files: server::Files,
    next_id: u64,
}

fn main() -> eframe::Result {
    let save_dir = dirs::download_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Vom Handy");
    std::fs::create_dir_all(&save_dir).expect("Zielordner konnte nicht angelegt werden");

    let listener = TcpListener::bind("0.0.0.0:0").expect("Port konnte nicht geöffnet werden");
    let port = listener.local_addr().unwrap().port();
    let ip = local_ip_address::local_ip().map(|i| i.to_string()).unwrap_or_else(|_| "127.0.0.1".into());
    let token = server::new_token();
    let url = format!("http://{ip}:{port}/{token}/");

    let code = QrCode::new(url.as_bytes()).unwrap();
    let w = code.width();
    let qr = code
        .to_colors()
        .chunks(w)
        .map(|row| row.iter().map(|c| *c == Color::Dark).collect())
        .collect();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([380.0, 600.0])
            .with_min_inner_size([320.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Handy Transfer",
        options,
        Box::new(move |cc| {
            let (tx, rx) = channel();
            let files: server::Files = Arc::new(Mutex::new(Vec::new()));
            server::spawn(listener, token, save_dir.clone(), tx, files.clone(), cc.egui_ctx.clone());
            Ok(Box::new(App { url, save_dir, qr, rx, received: Vec::new(), files, next_id: 1 }))
        }),
    )
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(r) = self.rx.try_recv() {
            self.received.insert(0, r);
        }
        self.add_dropped(ui.ctx());
        let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());

        ui.vertical_centered(|ui| {
            ui.add_space(8.0);
            ui.heading("Handy Transfer");
            ui.label("Mit dem Handy scannen (gleiches WLAN)");
            ui.add_space(8.0);
            draw_qr(ui, &self.qr, 260.0);
            ui.add_space(8.0);
            ui.add(egui::Label::new(egui::RichText::new(&self.url).small().weak()).wrap());
            ui.horizontal(|ui| {
                if ui.button("Link kopieren").clicked() {
                    ui.ctx().copy_text(self.url.clone());
                }
                if ui.button("Ordner öffnen").clicked() {
                    let _ = std::process::Command::new("explorer").arg(&self.save_dir).spawn();
                }
            });
        });

        ui.separator();
        ui.strong("Zum Handy senden");
        let mut remove = None;
        {
            let files = self.files.lock().unwrap();
            if files.is_empty() {
                let hint = if hovering { "Jetzt loslassen …" } else { "Dateien hierher ziehen." };
                ui.weak(hint);
            }
            for f in files.iter() {
                ui.horizontal(|ui| {
                    if ui.small_button("✕").clicked() {
                        remove = Some(f.id);
                    }
                    ui.label(format!("{}  ({})", f.name, human_size(f.size)));
                });
            }
        }
        if let Some(id) = remove {
            self.files.lock().unwrap().retain(|f| f.id != id);
        }

        ui.separator();
        ui.strong("Empfangen");
        if self.received.is_empty() {
            ui.weak("Noch nichts empfangen.");
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for r in &self.received {
                let name = r.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                ui.label(format!("✔ {name}  ({})", human_size(r.size)));
            }
        });
    }
}

impl App {
    /// Per Drag & Drop abgelegte Dateien (keine Ordner) für den Download am Handy freigeben.
    fn add_dropped(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for d in dropped {
            let path = d.path().to_path_buf();
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            if !meta.is_file() {
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("datei").to_string();
            self.files.lock().unwrap().push(server::SharedFile { id: self.next_id, path, name, size: meta.len() });
            self.next_id += 1;
        }
    }
}

fn draw_qr(ui: &mut egui::Ui, modules: &[Vec<bool>], size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::WHITE);
    let quiet = 2.0;
    let n = modules.len() as f32;
    let cell = size / (n + quiet * 2.0);
    for (y, row) in modules.iter().enumerate() {
        for (x, dark) in row.iter().enumerate() {
            if *dark {
                let min = rect.min + egui::vec2((x as f32 + quiet) * cell, (y as f32 + quiet) * cell);
                painter.rect_filled(egui::Rect::from_min_size(min, egui::vec2(cell, cell)), 0.0, egui::Color32::BLACK);
            }
        }
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS[i]) }
}
