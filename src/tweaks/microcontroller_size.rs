use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

use anyhow::Context;
use memory_rs::generate_aob_pattern;
use retour::GenericDetour;

use super::{Defaults, MemoryRegionExt, Tweak, TweakConfig};

// On/off for the unlock. Off by default (opt-in).
const ENABLED_DEFAULTS: Defaults<bool> = Defaults::new(false, false);
// Configurable slider bounds (integers). `vanilla` is the stock game value (1 / 6).
const MIN_DEFAULTS: Defaults<i32> = Defaults::new(1, 1);
const MAX_DEFAULTS: Defaults<i32> = Defaults::new(6, 6);

// Stock grid slider range, restored when the unlock is off / on eject.
const VANILLA_MIN: f32 = 1.0;
const VANILLA_MAX: f32 = 6.0;

// Live state the detour reads each frame.
static UNLOCKED: AtomicBool = AtomicBool::new(false);
static MIN_SIZE: AtomicI32 = AtomicI32::new(1);
static MAX_SIZE: AtomicI32 = AtomicI32::new(6);

// Last-seen slider objects (singletons, stable address) so we can restore the
// vanilla range when the tweak is ejected/unloaded.
static WIDTH_SLIDER: AtomicUsize = AtomicUsize::new(0);
static LENGTH_SLIDER: AtomicUsize = AtomicUsize::new(0);

// The per-frame microcontroller-editor update function: `void update(this)`
// (fastcall, `this` in RCX). `this + 0xB8` = microprocessor object (null when not
// editing an MC). `this + 0x98` = editor sub-object `O`; the width/length size
// sliders live at:
//   width_slider  = *(*(O + 0x98) + 0x90)
//   length_slider = *(*(O + 0xA0) + 0x90)
// A slider stores min @+0x68 and max @+0x6c (f32). Vanilla grid sliders are 1..6.
type EditorUpdateFn = extern "fastcall" fn(*mut u8, usize, usize, usize);
static mut EDITOR_UPDATE_FN: Option<EditorUpdateFn> = None;

unsafe fn slider_ptr(o: usize, widget_off: usize) -> usize {
    let widget = *((o + widget_off) as *const usize);
    if widget == 0 {
        return 0;
    }
    *((widget + 0x90) as *const usize)
}

unsafe fn set_bounds(slider: usize, min: f32, max: f32) {
    if slider == 0 {
        return;
    }
    *((slider + 0x68) as *mut f32) = min; // min
    *((slider + 0x6c) as *mut f32) = max; // max
}

pub struct MicrocontrollerSizeTweak {
    // Kept alive so the detour stays installed; dropping it restores the original bytes.
    _detour: GenericDetour<EditorUpdateFn>,
}

impl TweakConfig for MicrocontrollerSizeTweak {
    const CONFIG_ID: &'static str = "microcontroller_size_tweak";
}

impl Tweak for MicrocontrollerSizeTweak {
    fn new(builder: &mut super::TweakBuilder) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        builder.set_category(Some("Editor"));

        let detour = unsafe {
            #[no_mangle]
            extern "fastcall" fn editor_update_hook(
                this: *mut u8,
                a: usize,
                b: usize,
                c: usize,
            ) {
                unsafe {
                    if !this.is_null() {
                        let mc = *(this.add(0xB8) as *const usize);
                        let o = *(this.add(0x98) as *const usize);
                        if mc != 0 && o != 0 {
                            let w = slider_ptr(o, 0x98); // width
                            let l = slider_ptr(o, 0xA0); // length
                            WIDTH_SLIDER.store(w, Ordering::Relaxed);
                            LENGTH_SLIDER.store(l, Ordering::Relaxed);

                            // When unlocked, force our configured range; otherwise
                            // keep the sliders at the stock 1..6.
                            let (min, max) = if UNLOCKED.load(Ordering::Relaxed) {
                                (
                                    MIN_SIZE.load(Ordering::Relaxed) as f32,
                                    MAX_SIZE.load(Ordering::Relaxed) as f32,
                                )
                            } else {
                                (VANILLA_MIN, VANILLA_MAX)
                            };
                            set_bounds(w, min, max);
                            set_bounds(l, min, max);
                        }
                    }
                    let orig = EDITOR_UPDATE_FN.unwrap_unchecked();
                    orig(this, a, b, c);
                }
            }

            // Start of the microcontroller-editor per-frame update function.
            // The leading saves + pushes anchor the match to the function entry (needed
            // for the detour). Version-floating bytes are wildcarded: the stack frame
            // size, the local-variable displacement, and the upper bytes of the
            // microprocessor-object offset (its low byte 0xB8 is kept as a partial anchor).
            #[rustfmt::skip]
            let pattern = generate_aob_pattern![
                0x48, 0x89, 0x5c, 0x24, 0x08,       // MOV  [RSP+8],RBX
                0x48, 0x89, 0x74, 0x24, 0x10,       // MOV  [RSP+10],RSI
                0x48, 0x89, 0x7c, 0x24, 0x18,       // MOV  [RSP+18],RDI
                0x55, 0x41, 0x54, 0x41, 0x55,       // PUSH RBP,R12,R13
                0x41, 0x56, 0x41, 0x57,             // PUSH R14,R15
                0x48, 0x8b, 0xec,                   // MOV  RBP,RSP
                0x48, 0x83, 0xec, _,                // SUB  RSP,<frame size>
                0x4c, 0x8b, 0xf1,                   // MOV  R14,RCX        (this)
                0x33, 0xff,                         // XOR  EDI,EDI
                0x89, 0x7d, _,                      // MOV  [RBP-?],EDI
                0x48, 0x39, 0xb9, 0xb8, _, _, _     // CMP  [RCX+..B8],RDI (microprocessor obj)
            ];
            let addr = builder
                .region
                .scan_aob_single(&pattern)
                .context("Error finding microcontroller editor update function")?;

            let det = GenericDetour::new(
                std::mem::transmute::<usize, EditorUpdateFn>(addr),
                editor_update_hook,
            )?;
            EDITOR_UPDATE_FN = Some(std::mem::transmute::<&(), EditorUpdateFn>(det.trampoline()));
            // Always installed; behaviour is gated by the UNLOCKED flag.
            det.enable()?;

            det
        };

        builder
            .toggle("Unlock Microcontroller Size", ENABLED_DEFAULTS)
            .tooltip(
                "Microcontrollers are normally capped at 6x6 in the editor.\n\
                 This raises the width/length size sliders' range so you can build\n\
                 larger microcontrollers, with the sliders still working normally.\n\
                 Use the Min/Max sliders below to set the range.",
            )
            .config_key("unlock_microcontroller_size")
            .on_value_changed(|enabled| UNLOCKED.store(enabled, Ordering::Relaxed))
            .build()?;

        builder
            .slider("MC Min Size", MIN_DEFAULTS, 1, 16)
            .tooltip("Minimum value for the microcontroller width/length sliders.")
            .config_key("min_size")
            .on_value_changed(|v| MIN_SIZE.store(v, Ordering::Relaxed))
            .disabled_when(|| !UNLOCKED.load(Ordering::Relaxed))
            .build()?;

        builder
            .slider("MC Max Size", MAX_DEFAULTS, 1, 64)
            .tooltip("Maximum value for the microcontroller width/length sliders.")
            .config_key("max_size")
            .on_value_changed(|v| MAX_SIZE.store(v, Ordering::Relaxed))
            .disabled_when(|| !UNLOCKED.load(Ordering::Relaxed))
            .build()?;

        Ok(Self { _detour: detour })
    }

    fn uninit(&mut self) -> anyhow::Result<()> {
        // Restore the stock 1..6 slider range so ejecting the mod leaves the game
        // in its original state (the detour itself is reverted when dropped).
        // Clear UNLOCKED first so any last detour run before it is dropped also
        // writes the vanilla range instead of the configured one.
        UNLOCKED.store(false, Ordering::Relaxed);
        unsafe {
            set_bounds(WIDTH_SLIDER.load(Ordering::Relaxed), VANILLA_MIN, VANILLA_MAX);
            set_bounds(LENGTH_SLIDER.load(Ordering::Relaxed), VANILLA_MIN, VANILLA_MAX);
        }
        Ok(())
    }
}
