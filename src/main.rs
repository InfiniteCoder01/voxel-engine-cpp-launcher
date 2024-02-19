// hide console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::{
    egui::{self, Button, ComboBox, Layout, RichText},
    emath::Align,
};
use egui_notify::Toasts;
use std::sync::{Arc, Mutex};

pub mod version_manager;
use version_manager::*;

fn main() -> Result<(), eframe::Error> {
    env_logger::init();

    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "VoxelEngine Launcher",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);

            let mut fonts = egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            cc.egui_ctx.set_fonts(fonts);

            let toasts = Arc::new(Mutex::new(Toasts::default()));

            Box::new(Launcher {
                toasts: toasts.clone(),
                version_manager: VersionManager::new(toasts.clone()),
                selected_version: None,
            })
        }),
    )
}

struct Launcher {
    toasts: Arc<Mutex<Toasts>>,
    version_manager: VersionManager,
    selected_version: Option<Arc<Version>>,
}

impl eframe::App for Launcher {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            for (_text_style, font_id) in ui.style_mut().text_styles.iter_mut() {
                font_id.size = 24.0;
            }

            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    ui.label("Version:");
                    let versions = self.version_manager.versions.lock().unwrap();
                    ComboBox::new("Version", "")
                        .selected_text(
                            self.selected_version
                                .as_ref()
                                .map_or("<None>", |version| &version.name),
                        )
                        .show_ui(ui, |ui| {
                            for version in versions.iter() {
                                ui.selectable_value(
                                    &mut self.selected_version,
                                    Some(version.clone()),
                                    &version.name,
                                );
                            }
                        });

                    if ui
                        .button(egui_phosphor::regular::ARROWS_CLOCKWISE)
                        .clicked()
                    {
                        self.version_manager.update();
                    }
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::Image::new(egui::include_image!("assets/banner.png")).shrink_to_fit(),
                    );
                    if let Some(progress) = *self.version_manager.progress.lock().unwrap() {
                        ui.add(
                            egui::ProgressBar::new(progress)
                                .text(format!("{}%", (progress * 100.0) as u8)),
                        );
                    } else {
                        ui.style_mut().text_styles.insert(
                            egui::TextStyle::Button,
                            egui::FontId::new(40.0, eframe::epaint::FontFamily::Proportional),
                        );
                        if ui
                            .add_sized(
                                [140.0, 55.0],
                                Button::new(RichText::new("Play").strong()).rounding(10.0),
                            )
                            .clicked()
                        {
                            if let Some(version) = &self.selected_version {
                                version.play(&self.version_manager);
                            } else {
                                self.toasts.lock().unwrap().error("No version selected");
                            }
                        }
                    }
                });
            });
        });

        self.toasts.lock().unwrap().show(ctx);
    }
}
