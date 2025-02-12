use anyhow::Context;
use hudhook::windows::Win32::UI::WindowsAndMessaging::{HWND_NOTOPMOST, WS_POPUP};
use memory_rs::generate_aob_pattern;

use super::{Defaults, InjectAt, Tweak, TweakConfig};

const NO_MINIMIZE_DEFAULTS: Defaults<bool> = Defaults::new(true, false);
const BORDERLESS_DEFAULTS: Defaults<bool> = Defaults::new(true, false);

pub struct FullscreenTweak;

impl TweakConfig for FullscreenTweak {
    const CONFIG_ID: &'static str = "fullscreen_tweak";
}

impl Tweak for FullscreenTweak {
    fn new(builder: &mut super::TweakBuilder) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        // remove auto minimize

        #[rustfmt::skip]
        let no_minimize_injection = builder.injection(
            // code that minimizes the window on lost focus
            generate_aob_pattern![
                0x48, 0x8b, 0x8e, 0x48, 0x03, 0x00, 0x00, // MOV        RCX,qword ptr [RSI + 0x348]
                0xba, 0x06, 0x00, 0x00, 0x00,             // MOV        EDX,0x6
                0xff, 0x15, _, _, _, _                    // CALL       qword ptr [->USER32.DLL::ShowWindow]
            ],
            // NOP the ShowWindow call
            vec![0x90; 6],
            InjectAt::End,
        ).context("Error finding minimize addr")?;

        // remove forced top level

        #[rustfmt::skip]
        let no_topmost_injection = builder.injection(
            // hWndInsertAfter arg for SetWindowPos
            generate_aob_pattern![
                0x8b, _, 0xe0,                        // MOV        param_1,dword ptr [RBP + local_54[12]]
                0x48, 0xc7, _, 0xff, 0xff, 0xff, 0xff // MOV        param_2,-0x1   (HWND_TOPMOST)
            ],
            // change HWND_TOPMOST to HWND_NOTOPMOST
            HWND_NOTOPMOST.0.to_le_bytes()[0..4].to_vec(),
            InjectAt::End,
        ).context("Error finding topmost arg addr")?;

        // force borderless

        #[rustfmt::skip]
        let borderless_injection = builder.injection(
            // the MOV is flags for SetWindowLongW
            generate_aob_pattern![
                _, 0x83, _, _, _,            // CMP (unimportant)
                0x74, _,                     // JE (unimportant)
                0xbb, 0x00, 0x00, 0x00, 0x86 // MOV        EBX,86000000
            ],
            // remove WS_POPUP flag
            (0x86000000 & !WS_POPUP.0).to_le_bytes().to_vec(),
            InjectAt::End,
        ).context("Error finding borderless args addr")?;

        builder
            .toggle("Disable Minimize on Lost Focus", NO_MINIMIZE_DEFAULTS)
            .tooltip("Prevents the window from automatically minimizing when you tab out in fullscreen.\nTurn fullscreen off and back on while enabled to fix window stuck on top")
            .config_key("disable_minimize_on_lost_focus")
            .injection(no_minimize_injection, false)
            .injection(no_topmost_injection, false)
            .build()?;

        builder
            .toggle("Force Borderless Fullscreen", BORDERLESS_DEFAULTS)
            .tooltip("Forces the window to open in borderless fullscreen instead of exclusive.\nYou need to toggle fullscreen for it to update.")
            .config_key("force_borderless_fullscreen")
            .injection(borderless_injection, false)
            .build()?;

        Ok(Self)
    }
    fn uninit(&mut self) -> anyhow::Result<()> {
        self.reset_to_vanilla();
        Ok(())
    }
}
