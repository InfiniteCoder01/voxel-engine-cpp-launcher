// hide console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::{
    egui::{self, vec2, Button, ComboBox, ImageSource, Layout, RichText},
    emath::Align,
    epaint::Color32,
};
use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    sync::{Arc, Mutex},
};

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

            cc.egui_ctx.style_mut(|style| {
                for (_text_style, font_id) in style.text_styles.iter_mut() {
                    font_id.size = 24.0;
                }
                style
                    .text_styles
                    .get_mut(&egui::TextStyle::Small)
                    .unwrap()
                    .size = 18.0;
            });

            let config = std::fs::read_to_string("launcher.toml")
                .ok()
                .and_then(|config| toml::from_str::<LauncherConfig>(&config).ok())
                .unwrap_or_default();

            cc.egui_ctx.set_visuals(config.visuals());
            let interface = Arc::new(Interface::new(config));

            Box::new(Launcher {
                interface: interface.clone(),
                version_manager: VersionManager::new(interface),
                selected_version: None,

                settings: false,
                about: false,
                force_refresh: false,
            })
        }),
    )
}

struct Launcher {
    interface: Arc<Interface>,
    version_manager: VersionManager,
    selected_version: Option<Arc<Version>>,

    settings: bool,
    about: bool,
    force_refresh: bool,
}

impl eframe::App for Launcher {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.selected_version.is_none() {
                if let Some(last) = &self.interface.config().last_version {
                    self.selected_version = self.version_manager.try_find(last);
                }
            }
            ui.set_enabled(!self.settings && !self.about);

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

                    ui.checkbox(&mut self.force_refresh, "Force refresh");

                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        if ui.button(egui_phosphor::fill::GEAR).clicked() {
                            self.settings = true;
                        }

                        if ui.button(egui_phosphor::fill::INFO).clicked() {
                            self.about = true;
                        }
                    });
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(egui::Image::new(self.interface.config().get_banner()).shrink_to_fit());
                    if let Some((progress, label)) = &*self.interface.progress() {
                        ui.style_mut().override_text_style = Some(egui::TextStyle::Small);
                        ui.add(egui::ProgressBar::new(*progress).text(label));
                        ui.style_mut().override_text_style = None;
                        ctx.request_repaint_after(std::time::Duration::from_millis(200));
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
                            self.interface.log().clear();
                            if let Some(version) = &self.selected_version {
                                {
                                    let mut config = self.interface.config();
                                    config.last_version = Some(version.name.clone());
                                    config.save();
                                }
                                version.play(self.interface.clone(), self.force_refresh);
                                self.force_refresh = false;
                                ctx.request_repaint_after(std::time::Duration::from_millis(500));
                            } else {
                                self.interface.error("No version selected");
                            }
                        }
                    }

                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        for line in self.interface.log().iter() {
                            ui.label(line.clone());
                        }
                    });
                });
            });
        });

        if self.settings {
            self.interface.config().show(ctx, &mut self.settings);
        }

        if self.about {
            egui::Window::new("About")
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .default_size(vec2(400.0, 300.0))
                .show(ctx, |ui| {
                    ui.label("VoxelEngine Launcher");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    ui.label("By InfiniteCoder");
                    ui.label("VoxexlEngine by MihailRis");
                    ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                        if ui.button("Ok").clicked() {
                            self.about = false;
                        }
                    })
                });
        }

        self.interface.toasts().show(ctx);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LauncherConfig {
    pub dark_mode: bool,
    pub build_unsupported: bool,
    pub use_prebuilt_when_possible: bool,
    pub download_lua: bool,

    pub last_version: Option<String>,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            dark_mode: true,
            build_unsupported: true,
            use_prebuilt_when_possible: true,
            download_lua: false,

            last_version: None,
        }
    }
}

use eframe::egui::Visuals;
impl LauncherConfig {
    pub fn visuals(&self) -> Visuals {
        if self.dark_mode {
            Visuals::dark()
        } else {
            Visuals::light()
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, show: &mut bool) {
        egui::Window::new("Settings")
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_size(vec2(600.0, 300.0))
            .show(ctx, |ui| {
                ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    ui.label("Theme: ");
                    if ui
                        .button(if self.dark_mode {
                            egui_phosphor::fill::SUN
                        } else {
                            egui_phosphor::fill::MOON
                        })
                        .clicked()
                    {
                        self.dark_mode = !self.dark_mode;
                        ctx.set_visuals(self.visuals());
                    }
                });

                ui.checkbox(
                    &mut self.build_unsupported,
                    "Build unsupported versions from source",
                );

                ui.checkbox(
                    &mut self.use_prebuilt_when_possible,
                    "Use prebuilt versions when possible",
                );
                ui.checkbox(&mut self.download_lua, "Download Lua (NOTE: Installs lua into your home directory due to make issues. Might crash)");

                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    if ui.button("Save & Close").clicked() {
                        self.save();
                        *show = false;
                    }
                })
            });
    }

    fn get_banner(&self) -> ImageSource {
        if self.dark_mode {
            egui::include_image!("assets/banners/dark/preview1.png")
        } else {
            egui::include_image!("assets/banners/light/preview2.png")
        }
    }

    fn save(&self) {
        std::fs::write("launcher.toml", toml::to_string_pretty(self).unwrap()).unwrap();
    }
}

pub struct Interface {
    toasts: Mutex<egui_notify::Toasts>,
    progress: Mutex<Option<(f32, String)>>,
    config: Mutex<LauncherConfig>,

    log: Mutex<Vec<RichText>>,
}

use std::sync::MutexGuard;
impl Interface {
    pub fn new(config: LauncherConfig) -> Self {
        Self {
            toasts: Mutex::new(egui_notify::Toasts::default()),
            progress: Mutex::new(None),
            config: Mutex::new(config),

            log: Mutex::new(Vec::new()),
        }
    }

    pub fn toasts(&self) -> MutexGuard<egui_notify::Toasts> {
        self.toasts.lock().unwrap()
    }

    pub fn progress(&self) -> MutexGuard<Option<(f32, String)>> {
        self.progress.lock().unwrap()
    }

    pub fn set_progress(&self, progress: f32, label: impl Into<String>) {
        self.progress().replace((progress, label.into()));
    }

    pub fn replace_progress(&self, progress: f32) {
        self.set_progress(progress, format!("{:.1}%", progress * 100.0))
    }

    pub fn config(&self) -> MutexGuard<LauncherConfig> {
        self.config.lock().unwrap()
    }

    pub fn log(&self) -> MutexGuard<Vec<RichText>> {
        self.log.lock().unwrap()
    }

    pub fn info(&self, message: impl Into<String>) {
        let message = message.into();
        let message = message.trim();
        self.toasts().info(message);
        self.log()
            .push(RichText::new(message).color(Color32::LIGHT_BLUE));
    }

    pub fn error(&self, message: impl Into<String>) {
        let message = message.into();
        let message = message.trim();
        self.toasts().error(message);
        self.log().push(RichText::new(message).color(Color32::RED));
    }

    pub fn warning(&self, message: impl Into<String>) {
        let message = message.into();
        let message = message.trim();
        self.toasts().warning(message);
        self.log()
            .push(RichText::new(message).color(Color32::YELLOW));
    }
}
