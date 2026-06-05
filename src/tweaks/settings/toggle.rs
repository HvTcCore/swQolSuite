use memory_rs::internal::injections::{Inject, Injection};

use crate::tweaks::{Defaults, DetourUntyped, TweakBuilder};

use super::{Setting, SettingImpl};

pub struct ToggleBuilder<'b, 'r> {
    tweak_builder: &'r mut TweakBuilder<'b>,
    defaults: Defaults<bool>,
    config_key: Option<String>,
    toggle: Toggle,
}

impl<'b, 'r> ToggleBuilder<'b, 'r> {
    #[must_use]
    pub fn new(
        tweak_builder: &'r mut TweakBuilder<'b>,
        label: impl Into<String>,
        defaults: impl Into<Defaults<bool>>,
    ) -> Self {
        let defaults = defaults.into();
        Self {
            tweak_builder,
            defaults,
            config_key: None,
            toggle: Toggle {
                tooltip: String::new(),
                label: label.into(),
                injections: vec![],
                detours: vec![],
                value_changed_listeners: vec![],
                disabled_predicate: None,
            },
        }
    }

    #[must_use]
    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        let mut tooltip: String = tooltip.into();
        if !tooltip.ends_with('\n') {
            tooltip = format!("{tooltip}\n");
        }
        self.toggle.tooltip = tooltip;
        self
    }

    #[must_use]
    pub fn config_key(mut self, config_key: impl Into<String>) -> Self {
        let config_key: String = config_key.into();
        self.config_key = Some(config_key);
        self
    }

    #[must_use]
    pub fn injection(mut self, injection: Injection, invert: bool) -> Self {
        self.toggle.injections.push((injection, invert));
        self
    }

    #[must_use]
    pub fn detour(
        mut self,
        detour: impl DetourUntyped + Send + Sync + 'static,
        invert: bool,
    ) -> Self {
        self.toggle.detours.push((Box::new(detour), invert));
        self
    }

    #[must_use]
    pub fn on_value_changed(mut self, callback: impl FnMut(bool) + Send + Sync + 'static) -> Self {
        self.toggle.value_changed_listeners.push(Box::new(callback));
        self
    }

    /// Grey out / disable the toggle when `predicate` returns `true`.
    #[must_use]
    pub fn disabled_when(mut self, predicate: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.toggle.disabled_predicate = Some(Box::new(predicate));
        self
    }

    pub fn build(self) -> anyhow::Result<()> {
        self.tweak_builder
            .add_setting(Setting::new(self.toggle, self.defaults, self.config_key))
    }
}

pub struct Toggle {
    label: String,
    tooltip: String,
    injections: Vec<(Injection, bool)>,
    detours: Vec<(Box<dyn DetourUntyped + Send + Sync>, bool)>,
    value_changed_listeners: Vec<Box<dyn FnMut(bool) + Send + Sync>>,
    disabled_predicate: Option<Box<dyn Fn() -> bool + Send + Sync>>,
}

impl SettingImpl<bool> for Toggle {
    fn set(&mut self, value: bool) -> anyhow::Result<()> {
        for (injection, invert) in &mut self.injections {
            #[allow(clippy::collapsible_else_if)]
            if value {
                if *invert {
                    injection.remove_injection();
                } else {
                    injection.inject();
                }
            } else {
                if *invert {
                    injection.inject();
                } else {
                    injection.remove_injection();
                }
            }
        }

        for (detour, invert) in &mut self.detours {
            #[allow(clippy::collapsible_else_if)]
            if value {
                if *invert {
                    detour.disable()?;
                } else {
                    detour.enable()?;
                }
            } else {
                if *invert {
                    detour.enable()?;
                } else {
                    detour.disable()?;
                }
            }
        }

        for listener in &mut self.value_changed_listeners {
            listener(value);
        }

        Ok(())
    }

    fn render(
        &mut self,
        value: &mut bool,
        defaults: &Defaults<bool>,
        ui: &hudhook::imgui::Ui,
    ) -> anyhow::Result<()> {
        let disabled = self
            .disabled_predicate
            .as_ref()
            .is_some_and(|predicate| predicate());

        let mut changed = false;
        ui.disabled(disabled, || {
            if ui.checkbox(&self.label, value) {
                changed = true;
            }
        });
        if ui.is_item_hovered() {
            ui.tooltip_text(format!(
                "{}(default: {}, vanilla: {})",
                self.tooltip, defaults.default, defaults.vanilla
            ));
        }
        if changed {
            self.set(*value)?;
        }

        Ok(())
    }
}
