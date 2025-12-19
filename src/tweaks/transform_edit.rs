use std::{
    arch::asm,
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::Context;
use hudhook::imgui::Key;
use memory_rs::{
    generate_aob_pattern,
    internal::injections::{Inject, Injection},
};
use retour::GenericDetour;

use crate::types::{ComponentBase, FlipParent, GameStateEditor, Transform};

use super::{Defaults, InjectAt, MemoryRegionExt, Tweak, TweakConfig};

const EDIT_TRANSFORM_DEFAULTS: Defaults<bool> = Defaults::new(true, false);
const SHIFT_COPY_TRANSFORM_DEFAULTS: Defaults<bool> = Defaults::new(true, false);

type UpdateQuaternionFn = extern "fastcall" fn(*mut Transform);
type EditorDestructorFn = extern "fastcall" fn(*mut GameStateEditor, *mut ());
type SetPlacingComponentFn = extern "fastcall" fn(*mut GameStateEditor, *mut ());
type SetFlipFn = extern "fastcall" fn(*mut FlipParent, u8);

static mut UPDATE_QUATERNION_FN: Option<UpdateQuaternionFn> = None;
static mut EDITOR_DESTRUCTOR_FN: Option<EditorDestructorFn> = None;
static mut SET_PLACING_COMPONENT_FN: Option<SetPlacingComponentFn> = None;
static mut SET_FLIP_FN: Option<SetFlipFn> = None;

static mut TRANSFORM: Option<*mut Transform> = None;
static SHIFT_HELD: AtomicBool = AtomicBool::new(false);
static FORCE_UPDATE_NEXT_TICK: AtomicBool = AtomicBool::new(false);
static DISABLE_NEXT_TICK: AtomicBool = AtomicBool::new(false);

pub struct TransformEditTweak {
    disable_quaternion_slerp: bool,
    disable_quaternion_slerp_injection: Injection,
    safety_check_inject: Injection,
}

impl TransformEditTweak {
    fn check_orthonormal(&mut self, matrix: &[i32; 9]) {
        let orthonormal = is_orthonormal(matrix);
        if orthonormal == self.disable_quaternion_slerp {
            self.disable_quaternion_slerp = !self.disable_quaternion_slerp;
            if self.disable_quaternion_slerp {
                self.disable_quaternion_slerp_injection.inject();
                self.safety_check_inject.inject();
            } else {
                self.disable_quaternion_slerp_injection.remove_injection();
                self.safety_check_inject.remove_injection();
                Self::force_update_quaternion();
            }
        }
    }

    fn reset_transform(&mut self) {
        if let Some(tr) = unsafe { TRANSFORM } {
            #[allow(clippy::cast_precision_loss)]
            unsafe {
                (*tr).rotation_mat3i_cur = [1, 0, 0, 0, 1, 0, 0, 0, 1];
                (*tr).rotation_mat3i_prev = (*tr).rotation_mat3i_cur;
                (*tr).rotation_mat3f_cur = (*tr).rotation_mat3i_cur.map(|i| i as _);
                self.check_orthonormal(&(*tr).rotation_mat3i_cur);
                Self::force_update_quaternion();
            }
        }
    }

    fn force_update_quaternion() {
        unsafe {
            if let (Some(update_quaternion), Some(tr)) = (UPDATE_QUATERNION_FN, TRANSFORM) {
                update_quaternion(tr);
            }
        }
    }
}

impl TweakConfig for TransformEditTweak {
    const CONFIG_ID: &'static str = "transform_edit_tweak";
}

impl Tweak for TransformEditTweak {
    #[allow(clippy::too_many_lines)]
    fn new(builder: &mut super::TweakBuilder) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        builder.set_category(Some("Editor"));

        let update_quaternion_detour = unsafe {
            #[no_mangle]
            extern "fastcall" fn update_quaternion_hook(tr: *mut Transform) {
                unsafe {
                    let update_quaternion: UpdateQuaternionFn =
                        UPDATE_QUATERNION_FN.unwrap_unchecked();
                    update_quaternion(tr);

                    #[allow(clippy::cast_precision_loss)]
                    if !is_orthonormal(&(*tr).rotation_mat3i_cur) {
                        (*tr).rotation_mat3f_cur = (*tr).rotation_mat3i_cur.map(|i| i as _);
                    }
                    TRANSFORM = Some(tr);
                }
            }

            // update quaternion function
            #[rustfmt::skip]
            let memory_pattern = generate_aob_pattern![
                0x40, 0x53,                                                // PUSH       RBX
                0x48, 0x81, 0xec, 0x80, 0x00, 0x00, 0x00,                  // SUB        RSP,0x80
                0x48, 0x8b, 0x05, _, _, _, _,                              // MOV        RAX,qword ptr _
                0x48, 0x33, 0xc4,                                          // XOR        RAX,RSP
                0x48, 0x89, 0x44, 0x24, 0x70,                              // MOV        qword ptr [RSP + local_18],RAX
                0x48, 0x8b, 0xd9,                                          // MOV        RBX,transform
                0xc7, 0x81, 0x1c, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00 // MOV        dword ptr [RCX + transform->field33_0x11c],0x0

            ];
            let update_quaternion_fn_addr = builder
                .region
                .scan_aob_single(&memory_pattern)
                .context("Error finding update_quaternion addr")?;

            let det = GenericDetour::new(
                std::mem::transmute::<usize, UpdateQuaternionFn>(update_quaternion_fn_addr),
                update_quaternion_hook,
            )?;
            UPDATE_QUATERNION_FN = Some(std::mem::transmute::<&(), UpdateQuaternionFn>(
                det.trampoline(),
            ));

            det
        };

        let editor_destructor_detour = unsafe {
            #[no_mangle]
            extern "fastcall" fn editor_destructor_hook(
                editor: *mut GameStateEditor,
                param_2: *mut (),
            ) {
                unsafe {
                    TRANSFORM = None;
                    let editor_destructor: EditorDestructorFn =
                        EDITOR_DESTRUCTOR_FN.unwrap_unchecked();
                    editor_destructor(editor, param_2);
                }
            }

            // vehicle editor destructor
            #[rustfmt::skip]
            #[allow(unused_parens)]
            let memory_pattern = generate_aob_pattern![
                0x48, 0x89, 0x5c, 0x24, 0x08,         // MOV        qword ptr [RSP + local_res8],RBX
                0x57,                                 // PUSH       RDI
                0x48, 0x83, 0xec, 0x20,               // SUB        RSP,0x20
                0x8b, 0xda,                           // MOV        EBX,param_2
                0x48, 0x8b, 0xf9,                     // MOV        RDI,param_1
                0xe8, _, _, _, _,                     // CALL       _
                0xf6, 0xc3, 0x01,                     // TEST       BL,0x1
                0x74, _,                              // JZ         _
                0xba, 0xf8, 0x9c, 0x00, 0x00          // MOV        param_2,0x9cf0 on =>1.15.8
            ];
            let editor_destructor_fn_addr = builder
                .region
                .scan_aob_single(&memory_pattern)
                .context("Error finding editor_destructor addr")?;

            let det = GenericDetour::new(
                std::mem::transmute::<usize, EditorDestructorFn>(editor_destructor_fn_addr),
                editor_destructor_hook,
            )?;
            EDITOR_DESTRUCTOR_FN = Some(std::mem::transmute::<&(), EditorDestructorFn>(
                det.trampoline(),
            ));

            det
        };

        #[rustfmt::skip]
        let disable_quaternion_slerp_injection = builder.injection(
            // call that sets display matrix to interpolated quaternion
            generate_aob_pattern![
                0x48, 0x8d, 0x93, 0xc0, 0x00, 0x00, 0x00, // LEA        RDX,[RBX + 0xc0]
                0x48, 0x8d, 0x4c, 0x24, 0x20,             // LEA        RCX,[RSP + 0x20]
                0xe8, _, _, _, _                          // CALL       quaternion_to_matrix3f
            ],
            // NOP the CALL
            vec![0x90; 5],
            InjectAt::End,
        ).context("Error finding quaternion slerp copy addr")?;

        // EAX here ends up being an increment count for a loop
        // but some matrices cause it to calculate 0 so the loop hangs
        #[rustfmt::skip]
        let memory_pattern = generate_aob_pattern![
            0x41, 0x03, 0xc6,       // ADD        EAX,R14D
            0x8b, 0x4d, 0x18,       // MOV        ECX,dword ptr [RBP + 0x18]
            0x41, 0x0f, 0xaf, 0xc9, // IMUL       ECX,R9D
            0x03, 0xc1,             // ADD        EAX,ECX
            0x99,                   // CDQ
            0x33, 0xc2,             // XOR        EAX,EDX
            0x2b, 0xc2              // SUB        EAX,EDX
        ];

        let mut safety_check_inject = builder
            .injection(
                &memory_pattern,
                {
                    // CALL safety_check
                    let mut inject = vec![0xff, 0x15, 0x02, 0x00, 0x00, 0x00, 0xeb, 0x08];
                    inject.extend_from_slice(&(safety_check as usize).to_le_bytes());
                    // pad with NOP
                    inject.resize(memory_pattern.size, 0x90);
                    inject
                },
                InjectAt::Start,
            )
            .context("Error finding safety check addr")?;

        safety_check_inject.inject();

        #[rustfmt::skip]
        let memory_pattern = generate_aob_pattern![
            0x48, 0x8b, 0x51, 0x58,      // MOV        RDX,qword ptr [RDX + 0x58] on 1.13.1
            0x48, 0x8b, 0x12,            // MOV        RDX,qword ptr [RDX]
            0x48, 0x8b, 0xce,            // MOV        RCX,RSI
            0xe8, _, _, _, _,            // CALL       set_placing_component
            0x48, 0x8b, 0x4c, 0x24, 0x68 // MOV        RCX,qword ptr [RSP + 0x68]
        ];

        #[rustfmt::skip]
        let hook_ctrl_click_injection = builder
            .injection(
                &memory_pattern,
                {
                    // CALL hook_ctrl_click
                    let mut inject = vec![
                        0x48, 0x8b, 0xce,                               // MOV        RCX,RSI
                        0xff, 0x15, 0x02, 0x00, 0x00, 0x00, 0xeb, 0x08, // start of long absolute JMP
                    ];
                    inject.extend_from_slice(&(hook_ctrl_click as usize).to_le_bytes()); // JMP target
                    // pad with NOP
                    inject.resize(memory_pattern.size, 0x90);
                    inject
                },
                InjectAt::Start,
            )
            .context("Error finding ctrl click addr")?;

        unsafe {
            let set_placing_component_fn_addr =
                (hook_ctrl_click_injection.entry_point + memory_pattern.size - 5)
                    .wrapping_add_signed(
                        *((hook_ctrl_click_injection.entry_point + memory_pattern.size - 5 - 4)
                            as *const i32) as isize,
                    );

            SET_PLACING_COMPONENT_FN = Some(std::mem::transmute::<usize, SetPlacingComponentFn>(
                set_placing_component_fn_addr,
            ));
        }

        //builder
        //    .toggle("Shift Copies Transform", SHIFT_COPY_TRANSFORM_DEFAULTS)
        //    .tooltip("If enabled, holding Shift while using Ctrl+Click to eyedrop component will copy the component's transform.")
        //    .config_key("shift_copies_transform")
        //    .injection(hook_ctrl_click_injection, false)
        //    .build()?;

        // set_flip function
        #[rustfmt::skip]
        let memory_pattern = generate_aob_pattern![
            0x48, 0x89, 0x5c, 0x24, 0x08,            // MOV        qword ptr [RSP + 0x8],RBX
            0x48, 0x89, 0x6c, 0x24, 0x10,            // MOV        qword ptr [RSP + 0x10],RBP
            0x48, 0x89, 0x74, 0x24, 0x18,            // MOV        qword ptr [RSP + 0x18],RSI
            0x57,                                    // PUSH       RDI
            0x41, 0x56,                              // PUSH       R14
            0x41, 0x57,                              // PUSH       R15
            0x48, 0x83, 0xec, 0x20,                  // SUB        RSP,0x20
            0x0f, 0xb6, 0x81, 0xe8, 0x01, 0x00, 0x00 // MOVZX      EAX,byte ptr [RCX + 0x1e8]
        ];
        let set_flip_fn_addr = builder
            .region
            .scan_aob_single(&memory_pattern)
            .context("Error finding set_flip addr")?;

        unsafe {
            SET_FLIP_FN = Some(std::mem::transmute::<usize, SetFlipFn>(set_flip_fn_addr));
        }

        builder
            .toggle("Placement Transform Editing", EDIT_TRANSFORM_DEFAULTS)
            .tooltip("If enabled, allows editing the placement transform.\nA grid of numbers allow you to edit the 3x3 rotation matrix of the component being placed (same as in xml).\nYou can also increment (or hold Alt to decrement) using Numpad 1-9 (make sure NumLock is on).\nNumpad 0 resets the matrix.\n(You may have to rotate a component in-editor once before it works)")
            .config_key("placement_transform_editing")
            .detour(update_quaternion_detour, false)
            .detour(editor_destructor_detour, false)
            .on_value_changed(|enabled| {
                if !enabled {
                    FORCE_UPDATE_NEXT_TICK.store(true, Ordering::Release);
                    DISABLE_NEXT_TICK.store(true, Ordering::Release);
                }
            })
            .build()?;

        Ok(Self {
            disable_quaternion_slerp: false,
            disable_quaternion_slerp_injection,
            safety_check_inject,
        })
    }

    fn uninit(&mut self) -> anyhow::Result<()> {
        self.reset_transform();

        Ok(())
    }

    fn render_category(
        &mut self,
        ui: &hudhook::imgui::Ui,
        category: Option<&str>,
    ) -> anyhow::Result<()> {
        if category.is_none() {
            if let Some(tr) = unsafe { TRANSFORM } {
                ui.separator();
                ui.text("Editor Placement Transform");
                if ui.is_item_hovered() {
                    ui.tooltip_text("These numbers represent the rotation matrix of the component being placed (same as in XML).\nYou can also increment (or hold Alt to decrement) using the Numpad (make sure NumLock is on).\nNumpad 0 resets the matrix.");
                }

                let mut next = unsafe { (*tr).rotation_mat3i_cur };
                let mut changed = false;

                #[allow(clippy::identity_op)]
                for r in 0..3 {
                    ui.set_next_item_width(80.0);
                    if ui
                        .input_int(format!("{}", r * 3 + 1), &mut next[r * 3 + 0])
                        .build()
                    {
                        changed = true;
                    }
                    ui.same_line();
                    ui.set_next_item_width(80.0);
                    if ui
                        .input_int(format!("{}", r * 3 + 2), &mut next[r * 3 + 1])
                        .build()
                    {
                        changed = true;
                    }
                    ui.same_line();
                    ui.set_next_item_width(80.0);
                    if ui
                        .input_int(format!("{}", r * 3 + 3), &mut next[r * 3 + 2])
                        .build()
                    {
                        changed = true;
                    }
                }
                if changed {
                    #[allow(clippy::cast_precision_loss)]
                    unsafe {
                        (*tr).rotation_mat3i_cur = next;
                        (*tr).rotation_mat3i_prev = (*tr).rotation_mat3i_cur;
                        (*tr).rotation_mat3f_cur = next.map(|i| i as _);
                    }
                }

                self.check_orthonormal(&next);
            }
        }

        Ok(())
    }

    fn constant_render(&mut self, ui: &hudhook::imgui::Ui) -> anyhow::Result<()> {
        let mut update = |idx: u8, add: i32| {
            if let Some(tr) = unsafe { TRANSFORM } {
                #[allow(clippy::cast_precision_loss)]
                unsafe {
                    (*tr).rotation_mat3i_cur[idx as usize] =
                        (*tr).rotation_mat3i_cur[idx as usize].saturating_add(add);
                    (*tr).rotation_mat3i_prev = (*tr).rotation_mat3i_cur;
                    (*tr).rotation_mat3f_cur = (*tr).rotation_mat3i_cur.map(|i| i as _);
                    self.check_orthonormal(&(*tr).rotation_mat3i_cur);
                }
            }
        };

        let add = if ui.is_key_down(Key::LeftAlt) { -1 } else { 1 };

        if ui.is_key_pressed(Key::Keypad7) {
            update(0, add);
        } else if ui.is_key_pressed(Key::Keypad8) {
            update(1, add);
        } else if ui.is_key_pressed(Key::Keypad9) {
            update(2, add);
        } else if ui.is_key_pressed(Key::Keypad4) {
            update(3, add);
        } else if ui.is_key_pressed(Key::Keypad5) {
            update(4, add);
        } else if ui.is_key_pressed(Key::Keypad6) {
            update(5, add);
        } else if ui.is_key_pressed(Key::Keypad1) {
            update(6, add);
        } else if ui.is_key_pressed(Key::Keypad2) {
            update(7, add);
        } else if ui.is_key_pressed(Key::Keypad3) {
            update(8, add);
        } else if ui.is_key_pressed(Key::Keypad0) {
            self.reset_transform();
        }

        if let Some(tr) = unsafe { TRANSFORM } {
            unsafe {
                self.check_orthonormal(&(*tr).rotation_mat3i_cur);
            }
        }

        if FORCE_UPDATE_NEXT_TICK.load(Ordering::Acquire) {
            FORCE_UPDATE_NEXT_TICK.store(false, Ordering::Release);

            if let Some(tr) = unsafe { TRANSFORM } {
                #[allow(clippy::cast_precision_loss)]
                unsafe {
                    (*tr).rotation_mat3i_prev = (*tr).rotation_mat3i_cur;
                    (*tr).rotation_mat3f_cur = (*tr).rotation_mat3i_cur.map(|i| i as _);
                    self.check_orthonormal(&(*tr).rotation_mat3i_cur);
                }
            }
            Self::force_update_quaternion();
        }

        if DISABLE_NEXT_TICK.load(Ordering::Acquire) {
            DISABLE_NEXT_TICK.store(false, Ordering::Release);
            self.reset_transform();
            unsafe {
                TRANSFORM = None;
            }
        }

        SHIFT_HELD.store(ui.is_key_down(Key::LeftShift), Ordering::Release);

        Ok(())
    }
}

fn is_orthonormal(matrix: &[i32; 9]) -> bool {
    fn dot(a: &[i32], b: &[i32]) -> i32 {
        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }

    fn magnitude_sq(v: &[i32]) -> i32 {
        dot(v, v)
    }

    fn det(matrix: &[i32; 9]) -> i32 {
        matrix[0] * (matrix[4] * matrix[8] - matrix[7] * matrix[5])
            - matrix[1] * (matrix[3] * matrix[8] - matrix[5] * matrix[6])
            + matrix[2] * (matrix[3] * matrix[7] - matrix[4] * matrix[6])
    }

    let col1 = [matrix[0], matrix[3], matrix[6]];
    let col2 = [matrix[1], matrix[4], matrix[7]];
    let col3 = [matrix[2], matrix[5], matrix[8]];

    let is_normalized =
        magnitude_sq(&col1) == 1 && magnitude_sq(&col2) == 1 && magnitude_sq(&col3) == 1;
    let is_orthogonal = dot(&col1, &col2) == 0 && dot(&col1, &col3) == 0 && dot(&col2, &col3) == 0;
    let is_det_positive = det(matrix) >= 0;

    is_normalized && is_orthogonal && is_det_positive
}

#[no_mangle]
extern "stdcall" fn safety_check() {
    unsafe {
        // safety check that increments are at least 1
        // EDX is overwritten immediately after so safe to use for work
        asm!(
            // original
            "ADD        EAX,R14D",
            "MOV        ECX,dword ptr [RBP + 0x18]",
            "IMUL       ECX,R9D",
            "ADD        EAX,ECX",
            "CDQ",
            "XOR        EAX,EDX",
            "SUB        EAX,EDX",
            // if eax == 0 { eax = 1; }
            "TEST       EAX,EAX",
            "MOV        EDX,1",
            "CMOVZ      EAX,EDX",
            // save RAX
            "PUSH       RAX",
            // if [RSP + 0x78] == 0 { [RSP + 0x78] = 1 }
            "MOV        EAX,[RSP + 0x8 + 0x8 + 0x78]",
            "TEST       EAX,EAX",
            "MOV        EDX,1",
            "CMOVZ      EAX,EDX",
            "MOV        [RSP + 0x8 + 0x8 + 0x78],EAX",
            // if [RBP + -0x68] == 0 { [RBP + -0x68] = 1 }
            "MOV        EAX,[RBP + -0x68]",
            "TEST       EAX,EAX",
            "MOV        EDX,1",
            "CMOVZ      EAX,EDX",
            "MOV        [RBP + -0x68],EAX",
            // restore RAX
            "POP        RAX",
            options(nostack),
        );
    }
}

#[no_mangle]
extern "stdcall" fn hook_ctrl_click() {
    unsafe {
        let editor: *mut GameStateEditor;
        let component: *mut ComponentBase;
        asm!(
            // original
            // "MOV        RDX,qword ptr [RDX + 0x58]", // don't want these lines since we want to have the reference to RDX before
            // "MOV        RDX,qword ptr [RDX]",        // |
            // "MOV        RCX,RSI",                    // handled outside
            "",
            out("rcx") editor,
            out("rdx") component,
            options(nostack),
        );

        #[allow(clippy::ptr_as_ptr)]
        let component_type = (*component).flip_type as *mut *mut ();
        let set_placing_component: SetPlacingComponentFn =
            SET_PLACING_COMPONENT_FN.unwrap_unchecked();
        set_placing_component(editor, *component_type);

        // if shift held, make eyedrop copy transform
        let tr = &mut (*editor).place_transform;
        if SHIFT_HELD.load(Ordering::Acquire) {
            let flip = ((*component).flip_type as usize + 0x40) as *mut u8;
            let matrix = &mut (*component).matrix;
            let ortho = is_orthonormal(&*matrix);
            if ortho || TRANSFORM.is_some() {
                tr.rotation_mat3i_cur = *matrix;
                // (*tr).rotation_mat3i_prev = (*tr).rotation_mat3i_cur;
                // (*tr).rotation_mat3f_cur = (*tr).rotation_mat3i_cur.map(|i| i as _);
                // (*tr).rotation_mat3i_cur = [1, 0, 0, 0, 1, 0, 0, 0, 1];
            }

            if let Some(update_quaternion) = UPDATE_QUATERNION_FN {
                update_quaternion(tr);
            }

            if let Some(set_flip) = SET_FLIP_FN {
                let flip_parent = &mut (*editor).flip_parent;
                let cur_flip = flip_parent.cur_flip;
                set_flip(flip_parent, *flip);
                if cur_flip & 0x1 != (*flip) & 0x1 {
                    let unk_flip_type = (*editor).placing_flip_type as usize;
                    (*editor).placing_flip_type = *((unk_flip_type
                        + ((*((unk_flip_type + 0x40) as *mut u8) ^ 0x1) * 0x8) as usize)
                        as *mut *mut ());
                }
                if cur_flip & 0x2 != (*flip) & 0x2 {
                    let unk_flip_type = (*editor).placing_flip_type as usize;
                    (*editor).placing_flip_type = *((unk_flip_type
                        + ((*((unk_flip_type + 0x40) as *mut u8) ^ 0x2) * 0x8) as usize)
                        as *mut *mut ());
                }
                if cur_flip & 0x4 != (*flip) & 0x4 {
                    let unk_flip_type = (*editor).placing_flip_type as usize;
                    (*editor).placing_flip_type = *((unk_flip_type
                        + ((*((unk_flip_type + 0x40) as *mut u8) ^ 0x4) * 0x8) as usize)
                        as *mut *mut ());
                }
            }

            if !is_orthonormal(&*matrix) {
                FORCE_UPDATE_NEXT_TICK.store(true, Ordering::Release);
            }
        }

        asm!(
            // original
            "MOV        RCX,qword ptr [RSP + 0x28 + 0x8 + 0x8 + 0x8 + 0x8 + 0x8 + 0x68]",
            options(nostack),
        );
    }
}