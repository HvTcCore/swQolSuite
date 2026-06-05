use std::{borrow::Borrow, fmt, marker::PhantomData};

use hudhook::imgui;
use memory_rs::internal::{
    injections::{Inject, Injection},
    memory::MemoryPattern,
    memory_region::MemoryRegion,
};
use num_traits::ToBytes;
use retour::GenericDetour;
use serde::{Deserialize, Serialize};
use settings::{slider::SliderBuilder, toggle::ToggleBuilder, SettingUntyped};

pub mod dev_mode;
pub mod editor_camera_speed;
pub mod editor_placement;
pub mod editor_show_hidden;
pub mod fast_loading_animations;
pub mod fullscreen;
pub mod map_lag;
pub mod microcontroller_size;
pub mod multithreaded_loading;
pub mod settings;
pub mod transform_edit;

pub trait TweakConfig {
    const CONFIG_ID: &'static str;
}

pub trait Tweak {
    fn new(builder: &mut TweakBuilder) -> anyhow::Result<Self>
    where
        Self: Sized;

    fn uninit(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn render(&mut self, _ui: &hudhook::imgui::Ui) -> anyhow::Result<()> {
        Ok(())
    }

    fn render_category(
        &mut self,
        _ui: &hudhook::imgui::Ui,
        _category: Option<&str>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn constant_render(&mut self, _ui: &hudhook::imgui::Ui) -> anyhow::Result<()> {
        Ok(())
    }

    fn reset_to_default(&mut self) {}
    fn reset_to_vanilla(&mut self) {}

    fn load_config(&mut self, _value: &toml::value::Table) -> anyhow::Result<()> {
        Ok(())
    }

    fn save_config(&mut self) -> anyhow::Result<toml::value::Table> {
        Ok(toml::value::Table::default())
    }
}

#[derive(Debug)]
pub enum ScanAOBSingleError {
    Error(anyhow::Error),
    NoMatches,
    MultipleMatches,
}

pub trait MemoryRegionExt {
    fn scan_aob_single(
        &self,
        pat: &memory_rs::internal::memory::MemoryPattern,
    ) -> anyhow::Result<usize>;
}

impl MemoryRegionExt for MemoryRegion {
    fn scan_aob_single(
        &self,
        pat: &memory_rs::internal::memory::MemoryPattern,
    ) -> anyhow::Result<usize> {
        let matches = self.scan_aob_all_matches(pat)?;

        match matches.len() {
            0 => anyhow::bail!("No Matches"),
            1 => Ok(matches[0]),
            _ => anyhow::bail!("Multiple Matches"),
        }
    }
}

pub struct TweakWrapper {
    inner: Box<dyn Tweak + Send + Sync>,
    settings: Vec<Box<dyn SettingUntyped + Send + Sync>>,
    category: Option<String>,
    title: String,
}

impl TweakWrapper {
    pub fn new<T: Tweak + TweakConfig + Send + Sync + 'static>(region: &MemoryRegion) -> anyhow::Result<Self> {
        let mut builder = TweakBuilder { region, settings: vec![], category: None };

        let tw = T::new(&mut builder)?;

        Ok(Self {
            inner: Box::new(tw),
            settings: builder.settings,
            category: builder.category,
            title: T::CONFIG_ID.to_owned()
        })
    }

    pub fn uninit(&mut self) -> anyhow::Result<()> {
        self.inner.uninit()
    }

    #[must_use]
    pub fn category(&self) -> &Option<String> {
        &self.category
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn render(&mut self, ui: &hudhook::imgui::Ui) -> anyhow::Result<()> {
        for t in &mut self.settings {
            t.render(ui)?;
        }
        self.inner.render(ui)?;

        Ok(())
    }

    pub fn render_category(
        &mut self,
        ui: &hudhook::imgui::Ui,
        category: Option<&str>,
    ) -> anyhow::Result<()> {
        self.inner.render_category(ui, category)
    }

    pub fn constant_render(&mut self, ui: &hudhook::imgui::Ui) -> anyhow::Result<()> {
        self.inner.constant_render(ui)
    }

    pub fn reset_to_default(&mut self) -> anyhow::Result<()> {
        for t in &mut self.settings {
            t.reset_to_default()?;
        }
        self.inner.reset_to_default();

        Ok(())
    }

    pub fn reset_to_vanilla(&mut self) -> anyhow::Result<()> {
        for t in &mut self.settings {
            t.reset_to_vanilla()?;
        }
        self.inner.reset_to_vanilla();

        Ok(())
    }

    pub fn load_config(&mut self, value: &toml::value::Table) -> anyhow::Result<()> {
        for setting in &mut self.settings {
            setting.load_config(value)?;
        }

        self.inner.load_config(value)
    }

    pub fn save_config(&mut self) -> anyhow::Result<toml::value::Table> {
        let mut config = self.inner.save_config()?;

        for setting in &self.settings {
            setting.save_config(&mut config)?;
        }

        Ok(config)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Defaults<T> {
    pub default: T,
    pub vanilla: T,
}

impl<T> Defaults<T> {
    pub const fn new(default: T, vanilla: T) -> Self {
        Self { default, vanilla }
    }
}

impl<T> From<(T, T)> for Defaults<T> {
    fn from(value: (T, T)) -> Self {
        Self { default: value.0, vanilla: value.1 }
    }
}

pub struct TweakBuilder<'a> {
    pub region: &'a MemoryRegion,
    settings: Vec<Box<dyn SettingUntyped + Send + Sync>>,
    category: Option<String>,
}

impl<'b> TweakBuilder<'b> {
    pub fn set_category(&mut self, category: Option<impl Into<String>>) {
        self.category = category.map(Into::into);
    }

    pub fn injection(
        &self,
        scan_pattern: impl Borrow<MemoryPattern>,
        inject: Vec<u8>,
        at: InjectAt,
    ) -> anyhow::Result<Injection> {
        let scan_pattern = scan_pattern.borrow();
        let addr = self.region.scan_aob_single(scan_pattern)?;
        let entry = match at {
            InjectAt::Start => addr,
            InjectAt::StartOffset(ofs) => addr.wrapping_add_signed(ofs),
            InjectAt::End => addr + scan_pattern.size - inject.len(),
            InjectAt::EndOffset(ofs) => {
                (addr + scan_pattern.size - inject.len()).wrapping_add_signed(ofs)
            },
        };
        Ok(Injection::new(entry, inject))
    }

    pub fn number_injection<N: ToBytes + Default>(
        &self,
        scan_pattern: impl Borrow<MemoryPattern>,
        at: InjectAt,
    ) -> anyhow::Result<NumberInjection<N>> {
        let scan_pattern = scan_pattern.borrow();
        let placeholder_inject = N::default().to_le_bytes().as_ref().to_vec();
        let addr = self.region.scan_aob_single(scan_pattern)?;
        let entry = match at {
            InjectAt::Start => addr,
            InjectAt::StartOffset(ofs) => addr.wrapping_add_signed(ofs),
            InjectAt::End => addr + scan_pattern.size - placeholder_inject.len(),
            InjectAt::EndOffset(ofs) => {
                (addr + scan_pattern.size - placeholder_inject.len()).wrapping_add_signed(ofs)
            },
        };
        Ok(NumberInjection::new(Injection::new(
            entry,
            placeholder_inject,
        )))
    }

    #[must_use]
    pub fn toggle<'r>(
        &'r mut self,
        display: impl Into<String>,
        defaults: impl Into<Defaults<bool>>,
    ) -> ToggleBuilder<'b, 'r> {
        ToggleBuilder::new(self, display, defaults)
    }

    #[must_use]
    pub fn slider<
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
    >(
        &'r mut self,
        display: impl Into<String>,
        defaults: impl Into<Defaults<N>>,
        min: N,
        max: N,
    ) -> SliderBuilder<'b, 'r, N> {
        SliderBuilder::new(self, display, defaults, min, max)
    }

    pub fn add_setting(
        &mut self,
        mut setting: impl SettingUntyped + Send + Sync + 'static,
    ) -> anyhow::Result<()> {
        setting.reset_to_default()?;
        self.settings.push(Box::new(setting));

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectAt {
    Start,
    StartOffset(isize),
    /// Injects at the end of the region minus the length of the injection
    End,
    EndOffset(isize),
}

pub struct NumberInjection<N: ToBytes> {
    injection: Injection,
    _phantom: PhantomData<N>,
}

impl<N: ToBytes> NumberInjection<N> {
    #[must_use]
    pub fn new(injection: Injection) -> Self {
        Self { injection, _phantom: PhantomData }
    }

    pub fn inject(&mut self, value: impl Borrow<N>) {
        self.injection.f_new = value.borrow().to_le_bytes().as_ref().to_vec();
        self.injection.inject();
    }

    pub fn remove_injection(&mut self) {
        self.injection.remove_injection();
    }
}

pub trait DetourUntyped {
    fn enable(&mut self) -> anyhow::Result<()>;
    fn disable(&mut self) -> anyhow::Result<()>;
}

impl<T: retour::Function> DetourUntyped for GenericDetour<T> {
    fn enable(&mut self) -> anyhow::Result<()> {
        unsafe { Ok(GenericDetour::enable(self)?) }
    }

    fn disable(&mut self) -> anyhow::Result<()> {
        unsafe { Ok(GenericDetour::disable(self)?) }
    }
}