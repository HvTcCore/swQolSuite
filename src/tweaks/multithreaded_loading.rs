use std::thread::JoinHandle;

use anyhow::Context;
use memory_rs::generate_aob_pattern;
use retour::GenericDetour;

use crate::tweaks::MemoryRegionExt;

use super::{Defaults, Tweak, TweakConfig};

type LoadRomFn = extern "fastcall" fn(*mut (), *mut (), usize, *mut ());
type LoadSaveFn = extern "fastcall" fn(*mut (), *mut (), *mut (), *mut (), *mut ());

const MULTITHREADED_LOADING_DEFAULTS: Defaults<bool> = Defaults::new(false, false);

static mut LOAD_ROM_FN: Option<LoadRomFn> = None;
static mut LOAD_SAVE_FN: Option<LoadSaveFn> = None;
static mut LOAD_ROM_THREAD: Option<JoinHandle<()>> = None;

pub struct MultithreadedLoadingTweak;

impl TweakConfig for MultithreadedLoadingTweak {
    const CONFIG_ID: &'static str = "multithreaded_loading_tweak";
}

impl Tweak for MultithreadedLoadingTweak {
    #[allow(clippy::too_many_lines)]
    fn new(builder: &mut super::TweakBuilder) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        builder.set_category(Some("Performance"));

        // --- multithreaded loading

        // move load_rom call to another thread
        // kind of a wildly unsafe change but I haven't had any issues with it so far
        // note that subdividing this further (ie. load_audio on another thread) DID lead to issues when exiting a world
        let load_rom_detour = unsafe {
            extern "fastcall" fn hook(
                param_1: *mut (),
                param_2: *mut (),
                param_3: usize,
                param_4: *mut (),
            ) {
                unsafe {
                    let param_1_ptr = param_1 as usize;
                    let param_2_ptr = param_2 as usize;
                    let param_3_ptr = param_3;
                    let param_4_ptr = param_4 as usize;
                    let thread = std::thread::Builder::new()
                        .name("load_rom".to_owned())
                        .spawn(move || {
                            let load_rom: LoadRomFn = LOAD_ROM_FN.unwrap_unchecked();
                            load_rom(
                                param_1_ptr as _,
                                param_2_ptr as _,
                                param_3_ptr as _,
                                param_4_ptr as _,
                            );
                        })
                        .unwrap();

                    LOAD_ROM_THREAD = Some(thread);
                }
            }

            #[rustfmt::skip]
            let load_rom_fn_addr = builder.region.scan_aob_single(&generate_aob_pattern![
                0x40, 0x55,                              // PUSH       RBP
                _,                                       // PUSH       _
                _,                                       // PUSH       _
                _,                                       // PUSH       _
                _, _,                                    // PUSH       _
                _, _,                                    // PUSH       _
                _, _,                                    // PUSH       _
                _, _,                                    // PUSH       _
                _, 0x8d, 0x6c, _, 0xe1,                  // LEA        RBP,[RSP + -0x1f]
                0x48, 0x81, 0xec, 0xf8, 0x00, 0x00, 0x00 // SUB        RSP,0xf8
            ]).context("Error finding load_rom fn addr")?;

            let det = GenericDetour::new(
                std::mem::transmute::<usize, LoadRomFn>(load_rom_fn_addr),
                hook,
            )
            .context("Failed to detour load_rom fn")?;

            LOAD_ROM_FN = Some(std::mem::transmute::<&(), LoadRomFn>(det.trampoline()));

            det.enable().context("Failed to enable load_rom detour")?;

            det
        };

        // join load_rom thread later on in loading to make sure we don't finish out of order
        let load_save_detour = unsafe {
            extern "fastcall" fn hook(
                param_1: *mut (),
                param_2: *mut (),
                param_3: *mut (),
                param_4: *mut (),
                param_5: *mut (),
            ) {
                unsafe {
                    let load_save: LoadSaveFn = LOAD_SAVE_FN.unwrap_unchecked();
                    load_save(param_1, param_2, param_3, param_4, param_5);
                    if let Some(handle) = LOAD_ROM_THREAD.take() {
                        handle.join().unwrap();
                    }
                }
            }

            #[rustfmt::skip]
            #[allow(unused_parens)]
            let load_save_fn_addr = builder.region.scan_aob_single(&generate_aob_pattern![
                _, 0x89, 0x5c, _, 0x18,                           // MOV        qword ptr [RSP + 0x18],RBX
                _, 0x89, 0x54, _, 0x10,                           // MOV        qword ptr [RSP + 0x10],RDX
                _,                                                // PUSH       _
                _,                                                // PUSH       _
                _,                                                // PUSH       _
                _, _,                                             // PUSH       _
                _, _,                                             // PUSH       _
                _, _,                                             // PUSH       _
                _, _,                                             // PUSH       _
                _, 0x8d, 0x6c, _, 0xe1,                           // LEA        RBP,[RSP + -0x1f]
                0x48, 0x81, 0xec, 0xa0, 0x00, 0x00, 0x00          // SUB        RSP,0xa0 on 1.12.7
            ]).context("Error finding load_save fn addr")?;

            let det = GenericDetour::new(
                std::mem::transmute::<usize, LoadSaveFn>(load_save_fn_addr),
                hook,
            )
            .context("Failed to detour load_save fn")?;

            LOAD_SAVE_FN = Some(std::mem::transmute::<&(), LoadSaveFn>(det.trampoline()));

            det.enable().context("Failed to enable load_save detour")?;

            det
        };

        builder
            .toggle("Multithreaded Loading (experimental)", MULTITHREADED_LOADING_DEFAULTS)
            .tooltip("EXPERIMENTAL!\nSplits asset loading into a separate thread, reducing world load time by ~40%.\nI haven't had any issues using this but I wouldn't be suprised if there are unknown edge cases.")
            .config_key("multithreaded_loading")
            .detour(load_rom_detour, false)
            .detour(load_save_detour, false)
            .build()?;

        Ok(Self)
    }
}