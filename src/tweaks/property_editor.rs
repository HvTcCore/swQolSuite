//! Illegal Property Editor (ported from SWRIPE).
//!
//! Lets you edit component properties (float sliders) beyond the editor's normal
//! limits. It watches the game's property-write instruction with a hardware
//! breakpoint + vectored exception handler: when a component is selected the game
//! writes its properties through that instruction, and we capture the addresses
//! (in RAX) so they can be read/written directly.
//!
//! Differences from the original SWRIPE (for stability / clean ejection):
//! - The target instruction is found with an AOB, not a hard-coded RVA.
//! - The exception handler is lock-free (atomics only) — no mutex/alloc inside it.
//! - The VEH **and** the breakpoint are removed when the toggle is turned off and in
//!   `uninit()`, before the DLL unloads (the original left the VEH registered, which
//!   crashed the game after eject as soon as a property write happened).

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use anyhow::Context as _;
use hudhook::imgui::{Key, Ui};
use memory_rs::generate_aob_pattern;

use winapi::ctypes::c_void;
use winapi::shared::minwindef::DWORD;
use winapi::um::errhandlingapi::{AddVectoredExceptionHandler, RemoveVectoredExceptionHandler};
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::{GetThreadContext, OpenThread, SetThreadContext};
use winapi::um::sysinfoapi::GetTickCount64;
use winapi::um::tlhelp32::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use winapi::um::winnt::{CONTEXT, CONTEXT_DEBUG_REGISTERS, EXCEPTION_POINTERS};

use super::{Defaults, MemoryRegionExt, Tweak, TweakConfig};

const MAX_PROPS: usize = 5;
/// Properties of one component arrive in a quick burst; a gap larger than this (ms)
/// means a new component was selected, so the captured set is reset.
const BURST_MS: u64 = 25;
const EXCEPTION_SINGLE_STEP: DWORD = 0x8000_0004;
const EXCEPTION_CONTINUE_EXECUTION: i32 = -1;
const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

const DEFAULTS: Defaults<bool> = Defaults::new(false, false);

// ---- shared state (all lock-free so the exception handler is safe) ----
static TARGET: AtomicUsize = AtomicUsize::new(0); // address of the watched movss
static ENABLED: AtomicBool = AtomicBool::new(false); // feature on (VEH registered)
static CAPTURE: AtomicBool = AtomicBool::new(false); // capturing addresses (BP set)
static BLOCK_WRITES: AtomicBool = AtomicBool::new(false);
static BP_SET: AtomicBool = AtomicBool::new(false);
static VEH_HANDLE: AtomicUsize = AtomicUsize::new(0);
static SERIAL: AtomicU32 = AtomicU32::new(0); // bumped on any capture change
static LAST_MS: AtomicU64 = AtomicU64::new(0);
#[allow(clippy::declare_interior_mutable_const)]
static PROP_ADDRS: [AtomicUsize; MAX_PROPS] = [
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
    AtomicUsize::new(0),
];

const PROP_NAMES: [&str; MAX_PROPS] = [
    "Stiffness / Rotor Size / Burn Rate / Grip",
    "Damping / Fuel Amount / Radius",
    "Grip / Pressure",
    "Radius",
    "Pressure",
];

// ---- exception handler (runs on a game thread; must stay lock-free) ----
unsafe extern "system" fn exception_handler(info: *mut EXCEPTION_POINTERS) -> i32 {
    if info.is_null() {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    let info = &*info;
    if (*info.ExceptionRecord).ExceptionCode != EXCEPTION_SINGLE_STEP {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    let ctx = &mut *info.ContextRecord;
    let target = TARGET.load(Ordering::Relaxed) as u64;
    if target == 0 || ctx.Rip != target {
        // Spurious single step (shouldn't happen for our execute breakpoint).
        return EXCEPTION_CONTINUE_EXECUTION;
    }

    let prop = ctx.Rax as usize;

    if CAPTURE.load(Ordering::Relaxed) && prop != 0 {
        let known = PROP_ADDRS.iter().any(|a| a.load(Ordering::Relaxed) == prop);
        if !known {
            let now = GetTickCount64();
            let last = LAST_MS.swap(now, Ordering::Relaxed);
            if last != 0 && now.saturating_sub(last) > BURST_MS {
                for a in &PROP_ADDRS {
                    a.store(0, Ordering::Relaxed);
                }
                SERIAL.fetch_add(1, Ordering::Relaxed);
            }
            for a in &PROP_ADDRS {
                if a.compare_exchange(0, prop, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
                {
                    SERIAL.fetch_add(1, Ordering::Relaxed);
                    break;
                }
            }
        }
    }

    if BLOCK_WRITES.load(Ordering::Relaxed) {
        ctx.Rip += 4; // skip the 4-byte `movss [rax],xmm2`
    }
    ctx.EFlags |= 0x1_0000; // resume flag: run one instruction without re-triggering
    EXCEPTION_CONTINUE_EXECUTION
}

// ---- hardware breakpoint management ----
unsafe fn for_each_thread(mut f: impl FnMut(*mut c_void)) {
    let pid = std::process::id();
    let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
    if snap as isize == -1 {
        return;
    }
    let mut te: THREADENTRY32 = std::mem::zeroed();
    te.dwSize = std::mem::size_of::<THREADENTRY32>() as DWORD;
    if Thread32First(snap, &mut te) != 0 {
        loop {
            if te.th32OwnerProcessID == pid {
                // THREAD_GET/SET_CONTEXT | QUERY | SUSPEND_RESUME
                let thread = OpenThread(0x0010 | 0x0008 | 0x0002 | 0x0004, 0, te.th32ThreadID);
                if !thread.is_null() {
                    f(thread);
                    CloseHandle(thread);
                }
            }
            if Thread32Next(snap, &mut te) == 0 {
                break;
            }
        }
    }
    CloseHandle(snap);
}

unsafe fn set_breakpoint(addr: usize) {
    for_each_thread(|thread| {
        let mut ctx: CONTEXT = std::mem::zeroed();
        ctx.ContextFlags = CONTEXT_DEBUG_REGISTERS;
        if GetThreadContext(thread, &mut ctx) != 0 {
            ctx.Dr0 = addr as u64;
            ctx.Dr7 = 1; // enable local exec breakpoint #0
            SetThreadContext(thread, &ctx);
        }
    });
}

unsafe fn clear_breakpoint() {
    for_each_thread(|thread| {
        let mut ctx: CONTEXT = std::mem::zeroed();
        ctx.ContextFlags = CONTEXT_DEBUG_REGISTERS;
        if GetThreadContext(thread, &mut ctx) != 0 {
            ctx.Dr0 = 0;
            ctx.Dr7 = 0;
            SetThreadContext(thread, &ctx);
        }
    });
}

unsafe fn set_capture(on: bool) {
    CAPTURE.store(on, Ordering::Relaxed);
    if on && !BP_SET.load(Ordering::Relaxed) {
        set_breakpoint(TARGET.load(Ordering::Relaxed));
        BP_SET.store(true, Ordering::Relaxed);
    } else if !on && BP_SET.load(Ordering::Relaxed) {
        clear_breakpoint();
        BP_SET.store(false, Ordering::Relaxed);
    }
}

unsafe fn enable_feature() {
    if VEH_HANDLE.load(Ordering::Relaxed) == 0 {
        let h = AddVectoredExceptionHandler(1, Some(exception_handler));
        VEH_HANDLE.store(h as usize, Ordering::Relaxed);
    }
    ENABLED.store(true, Ordering::Relaxed);
}

unsafe fn disable_feature() {
    ENABLED.store(false, Ordering::Relaxed);
    set_capture(false);
    BLOCK_WRITES.store(false, Ordering::Relaxed);
    let h = VEH_HANDLE.swap(0, Ordering::Relaxed);
    if h != 0 {
        RemoveVectoredExceptionHandler(h as *mut c_void);
    }
}

pub struct PropertyEditorTweak {
    values: [f32; MAX_PROPS],
    last_serial: u32,
    read_on_select: bool,
    instant_update: bool,
    show_debug: bool,
}

impl PropertyEditorTweak {
    fn apply(&self) {
        for i in 0..MAX_PROPS {
            let a = PROP_ADDRS[i].load(Ordering::Relaxed);
            if a != 0 {
                unsafe {
                    *(a as *mut f32) = self.values[i];
                }
            }
        }
    }
}

impl TweakConfig for PropertyEditorTweak {
    const CONFIG_ID: &'static str = "property_editor_tweak";
}

impl Tweak for PropertyEditorTweak {
    fn new(builder: &mut super::TweakBuilder) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        builder.set_category(Some("Experimental"));

        // The component property write: `movss [rax],xmm2` (RAX = property address),
        // followed by `add rsp,0x30`. Unique in the binary.
        #[rustfmt::skip]
        let pattern = generate_aob_pattern![
            0xf3, 0x0f, 0x11, 0x10, // MOVSS [RAX],XMM2
            0x48, 0x83, 0xc4, 0x30  // ADD   RSP,0x30
        ];
        let target = builder
            .region
            .scan_aob_single(&pattern)
            .context("Error finding property-write instruction")?;
        TARGET.store(target, Ordering::Relaxed);

        builder
            .toggle("SWRIPE - Illegal Property Editor", DEFAULTS)
            .tooltip(
                "Edit component properties past the editor's normal limits (ported from\n\
                 SWRIPE). When enabled, turn on Capture Mode and select a component with\n\
                 the normal select tool to capture its properties, then edit the values.\n\
                 Uses a hardware breakpoint + exception handler - more invasive than the\n\
                 other tweaks, so save often.",
            )
            .config_key("illegal_property_editor")
            .on_value_changed(|on| unsafe {
                if on {
                    enable_feature();
                } else {
                    disable_feature();
                }
            })
            .build()?;

        Ok(Self {
            values: [1.0; MAX_PROPS],
            last_serial: 0,
            read_on_select: true,
            instant_update: false,
            show_debug: false,
        })
    }

    #[allow(clippy::too_many_lines)]
    fn render(&mut self, ui: &Ui) -> anyhow::Result<()> {
        if !ENABLED.load(Ordering::Relaxed) {
            return Ok(());
        }

        // F2 toggles capture (matches SWRIPE).
        if ui.is_key_pressed_no_repeat(Key::F3) {
            unsafe { set_capture(!CAPTURE.load(Ordering::Relaxed)) };
        }

        let mut capture = CAPTURE.load(Ordering::Relaxed);
        if ui.checkbox("Capture Mode (F3)", &mut capture) {
            unsafe { set_capture(capture) };
        }
        if ui.is_item_hovered() {
            ui.tooltip_text("Select a component to capture its property addresses.");
        }

        let mut block = BLOCK_WRITES.load(Ordering::Relaxed);
        if ui.checkbox("Prevent property writes", &mut block) {
            BLOCK_WRITES.store(block, Ordering::Relaxed);
        }
        if ui.is_item_hovered() {
            ui.tooltip_text(
                "Stops the game from writing/clamping the selected component's\n\
                 properties so illegal values stick (disables the normal sliders).",
            );
        }

        ui.checkbox("Instant update", &mut self.instant_update);

        let has_selection = PROP_ADDRS[0].load(Ordering::Relaxed) != 0;

        // Refresh shown values when a new component is captured.
        let serial = SERIAL.load(Ordering::Relaxed);
        if self.read_on_select && has_selection && serial != self.last_serial {
            for i in 0..MAX_PROPS {
                let a = PROP_ADDRS[i].load(Ordering::Relaxed);
                if a != 0 {
                    self.values[i] = unsafe { *(a as *const f32) };
                }
            }
            self.last_serial = serial;
        }

        ui.separator();

        if !has_selection {
            ui.text_disabled("No component captured.");
            ui.text_disabled("Enable Capture Mode and select a component.");
            return Ok(());
        }

        for i in 0..MAX_PROPS {
            let addr = PROP_ADDRS[i].load(Ordering::Relaxed);
            if addr == 0 {
                continue;
            }
            ui.text(PROP_NAMES[i]);
            ui.set_next_item_width(160.0);
            let mut val = self.values[i];
            if ui
                .input_float(format!("##prop{i}"), &mut val)
                .step(0.1)
                .build()
            {
                self.values[i] = val;
                if self.instant_update {
                    self.apply();
                }
            }
        }

        ui.separator();

        ui.text_disabled(
            "Deselect the block before Apply,\n\
             or it may not stick.\n\
             (or use \"Prevent property writes\")",
        );

        if ui.button("Apply") {
            self.apply();
        }
        ui.same_line();
        if ui.button("Clear") {
            for a in &PROP_ADDRS {
                a.store(0, Ordering::Relaxed);
            }
            LAST_MS.store(0, Ordering::Relaxed);
            self.last_serial = 0;
        }

        ui.checkbox("Show captured addresses", &mut self.show_debug);
        if self.show_debug {
            for i in 0..MAX_PROPS {
                let a = PROP_ADDRS[i].load(Ordering::Relaxed);
                if a != 0 {
                    ui.text_disabled(format!("Property {} @ 0x{a:X}", i + 1));
                }
            }
        }

        Ok(())
    }

    fn uninit(&mut self) -> anyhow::Result<()> {
        // Remove the breakpoint and the exception handler before the DLL unloads.
        unsafe { disable_feature() };
        Ok(())
    }
}
