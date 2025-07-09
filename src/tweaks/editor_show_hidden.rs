use anyhow::Context;
use memory_rs::generate_aob_pattern;

use super::{Defaults, InjectAt, Tweak, TweakConfig};

const SHOW_HIDDEN_COMPONENTS_DEFAULTS: Defaults<bool> = Defaults::new(true, false);

pub struct ShowHiddenComponents;

impl TweakConfig for ShowHiddenComponents {
    const CONFIG_ID: &'static str = "show_hidden_components_tweak";
}

impl Tweak for ShowHiddenComponents {
    fn new(builder: &mut super::TweakBuilder) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        builder.set_category(Some("Editor"));

        // --- check 1

        #[rustfmt::skip]
        let injection_1 = builder.injection(
            // check for hidden flag
            generate_aob_pattern![
                0x48, 0xa9, 0x00, 0x00, 0x00, 0x20, // TEST       RAX,0x20000000
                0x77, 0x67                          // JA
            ],
            // NOP the JA
            vec![0x90; 2],
            InjectAt::End,
        ).context("Error finding hidden component check 1 addr")?;

        // --- check 2

        #[rustfmt::skip]
        let injection_2 = builder.injection(
            // check for hidden flag
            generate_aob_pattern![
                0x48, 0x8b, 0x86, 0xb8, 0x02, 0x00, 0x00, // MOV        RAX,dword ptr [RSI + 0x2b8]
                0x48, 0xa9, 0x00, 0x00, 0x00, 0x20        // TEST       RAX,0x20000000
            ],
            // replace 0x20000000 with 0x00000000 (so it'll always skip the JA after)
            vec![0x00, 0x00, 0x00, 0x00],
            InjectAt::End,
        ).context("Error finding hidden component check 2 addr")?;

        builder
            .toggle("Show Hidden Components (reload save)", SHOW_HIDDEN_COMPONENTS_DEFAULTS)
            .tooltip("Forces editor to show components flagged as hidden.\nChanging this setting requires reloading your save to apply.")
            .config_key("show_hidden_components")
            .injection(injection_1, false)
            .injection(injection_2, false)
            .build()?;

        Ok(Self)
    }
}
