#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::unreadable_literal)]

#[allow(clippy::module_name_repetitions)]
pub mod tweaks;
pub mod types;

use std::collections::{BTreeMap, HashMap};

use figment::providers::{Format, Toml};
use figment::Figment;
use hooks::opengl3::ImguiOpenGl3Hooks;
use hudhook::imgui::TreeNodeFlags;
use hudhook::tracing::{error, trace};
use hudhook::{eject, hooks, imgui, windows, Hudhook, ImguiRenderLoop, MessageFilter};
use imgui::{Condition, Io, Key, StyleColor, StyleVar, Ui};
use itertools::Itertools;
use memory_rs::internal::memory_region::MemoryRegion;
use memory_rs::internal::process_info::ProcessInfo;
use tweaks::dev_mode::DevModeTweak;
use tweaks::editor_camera_speed::EditorCameraSpeedTweak;
use tweaks::editor_placement::EditorPlacementTweak;
use tweaks::editor_show_hidden::ShowHiddenComponents;
use tweaks::fast_loading_animations::FastLoadingAnimationsTweak;
use tweaks::fullscreen::FullscreenTweak;
use tweaks::map_lag::MapLagTweak;
use tweaks::microcontroller_size::MicrocontrollerSizeTweak;
use tweaks::multithreaded_loading::MultithreadedLoadingTweak;
use tweaks::transform_edit::TransformEditTweak;
use tweaks::{Tweak, TweakConfig, TweakWrapper};
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

#[allow(clippy::missing_safety_doc)]
#[allow(non_snake_case)]
#[no_mangle]
pub unsafe extern "stdcall" fn DllMain(hmodule: HINSTANCE, reason: u32, _: *mut std::ffi::c_void) {
    if reason == DLL_PROCESS_ATTACH {
        trace!("DllMain()");
        std::thread::spawn(move || {
            // alloc_console().unwrap();

            println!("Spawned thread");

            std::panic::set_hook(Box::new(|panic_info| {
                if let Some(str) = panic_info.payload().downcast_ref::<&str>() {
                    println!("panic occurred: {str:?}");
                } else {
                    println!("panic occurred");
                }
            }));

            if let Err(e) = Hudhook::builder()
                .with::<ImguiOpenGl3Hooks>(MainHud::new())
                .with_hmodule(hmodule)
                .build()
                .apply()
            {
                error!("Couldn't apply hooks: {e:?}");
                eject();
            }
        });
    }
}

struct MainHud {
    version_string: String,
    simple_version_string: String,
    show: bool,
    tweaks: Vec<(TweakWrapper, &'static str)>,
    errors: HashMap<String, Vec<anyhow::Error>>,
}

impl MainHud {
    fn new() -> Self {
        let simple_version_string = format!(
            "{}{}",
            env!("CARGO_PKG_VERSION"),
            option_env!("SHA").map_or_else(String::new, |sha| format!(" ({sha})"))
        );
        let mut this = Self {
            version_string: format!("swQolSuite v{simple_version_string}"),
            simple_version_string,
            show: true,
            tweaks: vec![],
            errors: HashMap::new(),
        };

        match ProcessInfo::new(Some("stormworks64.exe")) {
            Ok(process) => {
                this.add_tweak::<EditorCameraSpeedTweak>(&process.region);
                this.add_tweak::<EditorPlacementTweak>(&process.region);
                this.add_tweak::<MicrocontrollerSizeTweak>(&process.region);
                this.add_tweak::<ShowHiddenComponents>(&process.region);
                this.add_tweak::<MapLagTweak>(&process.region);
                this.add_tweak::<FastLoadingAnimationsTweak>(&process.region);
                this.add_tweak::<MultithreadedLoadingTweak>(&process.region);
                this.add_tweak::<FullscreenTweak>(&process.region);
                this.add_tweak::<DevModeTweak>(&process.region);
                this.add_tweak::<TransformEditTweak>(&process.region);
            },
            Err(err) => this.errors.entry("Process".to_owned()).or_default().push(err),
        }

        if let Err(e) = this.load_config() {
            this.errors.entry("Config".to_owned()).or_default().push(e);
        }

        this
    }

    fn add_tweak<T: Tweak + TweakConfig + Send + Sync + 'static>(&mut self, region: &MemoryRegion) {
        let tw = TweakWrapper::new::<T>(region);
        match tw {
            Ok(tw) => {
                self.tweaks.push((tw, T::CONFIG_ID));
            },
            Err(e) => self.errors.entry(T::CONFIG_ID.to_owned()).or_default().push(e),
        }
    }

    fn load_config(&mut self) -> anyhow::Result<()> {
        let figment = Figment::new().merge(Toml::file("swQolSuite.toml"));

        for (tw, id) in &mut self.tweaks {
            let config = figment.find_value(id).ok();
            if let Some(config) = config {
                let config = config.deserialize()?;
                tw.load_config(&config)?;
            }
        }

        Ok(())
    }

    fn save_config(&mut self) -> anyhow::Result<()> {
        let mut map = BTreeMap::new();

        map.insert(
            "swQolSuite",
            std::iter::once((
                "version".to_owned(),
                toml::Value::try_from(self.simple_version_string.clone())?,
            ))
            .collect(),
        );

        for (tw, id) in &mut self.tweaks {
            let saved = tw.save_config()?;
            if !saved.is_empty() {
                map.insert(id, saved);
            }
        }

        let toml_string = toml::to_string_pretty(&map)?;
        std::fs::write("swQolSuite.toml", &toml_string)?;

        Ok(())
    }

    fn uninit(&mut self) {
        let mut ok = true;

        let map = std::mem::take(&mut self.tweaks);

        for (mut tw, _) in map {
            if let Err(e) = tw.uninit() {
                self.errors.entry(tw.title().to_owned()).or_default().push(e);
                ok = false;
            }
        }

        if ok {
            eject();
        }
    }
}

impl ImguiRenderLoop for MainHud {
    // fn before_render<'a>(
    //     &'a mut self,
    //     ctx: &mut Context,
    //     _render_context: &'a mut dyn RenderContext,
    // ) {
    //     ctx.io_mut().mouse_draw_cursor = self.show;
    // }

    #[allow(clippy::too_many_lines)]
    fn render(&mut self, ui: &mut Ui) {
        if ui.is_key_pressed_no_repeat(Key::GraveAccent) {
            self.show = !self.show;
        }

        let style_padding = ui.push_style_var(StyleVar::WindowPadding([2., 2.]));
        ui.window("##version")
            .no_decoration()
            .no_inputs()
            .draw_background(false)
            .save_settings(false)
            .always_use_window_padding(true)
            .always_auto_resize(true)
            .size_constraints([-1., 18.], [-1., 18.])
            .position_pivot([1., 1.])
            .position(ui.io().display_size, Condition::Always)
            .build(|| {
                let text_color = ui.push_style_color(
                    StyleColor::Text,
                    [0.8, 0.8, 0.8, if self.show { 0.9 } else { 0.4 }],
                );
                if self.show {
                    ui.text(format!("{} [~]", self.version_string));
                } else {
                    ui.text("  [~]");
                }
                text_color.end();
            });
        style_padding.end();

        for (tw, _) in &mut self.tweaks {
            if let Err(e) = tw.constant_render(ui) {
                self.errors.entry(tw.title().to_owned()).or_default().push(e);
                self.show = true;
            }
        }

        if !self.show {
            return;
        }

        // let bg_color = ui.push_style_color(StyleColor::WindowBg, [0.1, 0.1, 0.1, 0.25]);
        // ui.window("##background")
        //     .focus_on_appearing(false)
        //     .bring_to_front_on_focus(false)
        //     .focused(false)
        //     .title_bar(false)
        //     .resizable(false)
        //     .position_pivot([0., 0.])
        //     .position([0., 0.], Condition::Always)
        //     .size(ui.io().display_size, Condition::Always)
        //     .build(|| {});
        // bg_color.end();

        ui.window("swQolSuite")
            .no_nav()
            .always_auto_resize(true)
            .resizable(false)
            .position([50., 50.], Condition::FirstUseEver)
            .build(|| {
                ui.text(&self.version_string);
                ui.text("[~] Visibility");
                if ui.button("Eject") {
                    self.uninit();
                }
            });

        ui.window("Tweaks")
            .no_nav()
            .always_auto_resize(true)
            .resizable(false)
            .position([250., 50.], Condition::FirstUseEver)
            .size_constraints([0.0, 0.0], [-1.0, ui.io().display_size[1] * 0.8])
            .build(|| {
                if ui.button("Reset to Default") {
                    for (tw, _) in &mut self.tweaks {
                        if let Err(e) = tw.reset_to_default() {
                            self.errors.entry(tw.title().to_owned()).or_default().push(e);
                            self.show = true;
                        }
                    }
                };
                ui.same_line();
                if ui.button("Reset to Vanilla") {
                    for (tw, _) in &mut self.tweaks {
                        if let Err(e) = tw.reset_to_vanilla() {
                            self.errors.entry(tw.title().to_owned()).or_default().push(e);
                            self.show = true;
                        }
                    }
                };

                if ui.button("Load Config") {
                    if let Err(e) = self.load_config() {
                        self.errors.entry("Config".to_owned()).or_default().push(e);
                        self.show = true;
                    }
                };
                ui.same_line();
                let config_hovered = ui.is_item_hovered();
                if ui.button("Save Config") {
                    if let Err(e) = self.save_config() {
                        self.errors.entry("Config".to_owned()).or_default().push(e);
                        self.show = true;
                    }
                };
                if config_hovered || ui.is_item_hovered() {
                    ui.tooltip_text("swQolSuite.toml, next to game's exe");
                }

                ui.separator();

                let categories = self
                    .tweaks
                    .iter_mut()
                    .enumerate()
                    .map(|(i, (tw, _))| (tw.category().clone(), i))
                    .into_group_map();

                for (category, mut tweak_indices) in categories.into_iter().sorted_by(|a, b| {
                    if a.0.is_none() || b.0.is_none() {
                        b.0.cmp(&a.0)
                    } else {
                        a.0.cmp(&b.0)
                    }
                }) {
                    let mut render = || {
                        for i in &mut tweak_indices {
                            if let Err(e) = self.tweaks[*i].0.render(ui) {
                                self.errors.entry(self.tweaks[*i].0.title().to_owned()).or_default().push(e);
                                self.show = true;
                            }
                        }

                        for (tw, _) in &mut self.tweaks {
                            if let Err(e) = tw.render_category(ui, category.as_deref()) {
                                self.errors.entry(tw.title().to_owned()).or_default().push(e);
                                self.show = true;
                            }
                        }
                    };

                    if let Some(category) = &category {
                        if ui.collapsing_header(category, TreeNodeFlags::empty()) {
                            ui.indent_by(8.0);
                            render();
                            ui.unindent_by(8.0);
                        }
                    } else {
                        render();
                    }
                }
            });

        if !self.errors.is_empty() {
            ui.window("Errors")
                .no_nav()
                .size([300., 200.], Condition::FirstUseEver)
                .position([50., 250.], Condition::FirstUseEver)
                .build(|| {
                    for (title, errs) in &self.errors {
                        ui.text(format!("{title}:"));
                        for err in errs {
                            ui.text(format!("{err:?}"));
                            ui.separator();
                        }
                        ui.separator();
                    }
                });
        }
    }

    fn message_filter(&self, io: &Io) -> MessageFilter {
        if self.show && (io.want_capture_keyboard || io.want_capture_mouse) {
            return MessageFilter::InputKeyboard | MessageFilter::InputMouse;
        }

        MessageFilter::empty()
    }
}
