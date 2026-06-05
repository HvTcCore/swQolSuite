use std::fmt::{self};

use hudhook::imgui;
use num_traits::ToBytes;
use serde::{Deserialize, Serialize};

use crate::tweaks::{Defaults, NumberInjection, TweakBuilder};

use super::{Setting, SettingImpl};

pub struct SliderBuilder<'b, 'r, N: ToBytes> {
    tweak_builder: &'r mut TweakBuilder<'b>,
    defaults: Defaults<N>,
    config_key: Option<String>,
    slider: Slider<N>,
}

impl<
        'b,
        'r,
        N: ToBytes
            + Copy
            + PartialEq
            + fmt::Display
            + Serialize
            + for<'a> Deserialize<'a>
            + imgui::internal::DataTypeKind
            + Send
            + Sync
            + 'static,
    > SliderBuilder<'b, 'r, N>
{
    #[must_use]
    pub fn new(
        tweak_builder: &'r mut TweakBuilder<'b>,
        label: impl Into<String>,
        defaults: impl Into<Defaults<N>>,
        min: N,
        max: N,
    ) -> Self {
        let defaults = defaults.into();
        Self {
            tweak_builder,
            defaults,
            config_key: None,
            slider: Slider {
                min,
                max,
                tooltip: String::new(),
                label: label.into(),
                injections: vec![],
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
        self.slider.tooltip = tooltip;
        self
    }

    #[must_use]
    pub fn config_key(mut self, config_key: impl Into<String>) -> Self {
        let config_key: String = config_key.into();
        self.config_key = Some(config_key);
        self
    }

    #[must_use]
    pub fn injection(mut self, injection: NumberInjection<N>) -> Self {
        self.slider.injections.push(injection);
        self
    }

    #[must_use]
    pub fn on_value_changed(mut self, callback: impl FnMut(N) + Send + Sync + 'static) -> Self {
        self.slider.value_changed_listeners.push(Box::new(callback));
        self
    }

    /// Grey out / disable the slider when `predicate` returns `true`.
    #[must_use]
    pub fn disabled_when(mut self, predicate: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.slider.disabled_predicate = Some(Box::new(predicate));
        self
    }

    pub fn build(self) -> anyhow::Result<()> {
        self.tweak_builder
            .add_setting(Setting::new(self.slider, self.defaults, self.config_key))
    }
}

pub struct Slider<N: ToBytes> {
    min: N,
    max: N,
    label: String,
    tooltip: String,
    injections: Vec<NumberInjection<N>>,
    value_changed_listeners: Vec<Box<dyn FnMut(N) + Send + Sync>>,
    disabled_predicate: Option<Box<dyn Fn() -> bool + Send + Sync>>,
}

impl<N: ToBytes + Copy + fmt::Display + imgui::internal::DataTypeKind> SettingImpl<N>
    for Slider<N>
{
    fn set(&mut self, value: N) -> anyhow::Result<()> {
        for injection in &mut self.injections {
            injection.inject(value);
        }

        for listener in &mut self.value_changed_listeners {
            listener(value);
        }

        Ok(())
    }

    fn render(
        &mut self,
        value: &mut N,
        defaults: &Defaults<N>,
        ui: &imgui::Ui,
    ) -> anyhow::Result<()> {
        let disabled = self
            .disabled_predicate
            .as_ref()
            .is_some_and(|predicate| predicate());

        let mut changed = false;
        ui.disabled(disabled, || {
            ui.set_next_item_width(100.0);
            if ui.slider(&self.label, self.min, self.max, value) {
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
