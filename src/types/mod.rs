#[repr(C)]
pub struct Transform {
    _pad1: [u8; 0x78],
    pub rotation_mat3i_prev: [i32; 9],
    pub rotation_mat3i_cur: [i32; 9],
    pub rotation_mat3f_cur: [f32; 9],
}

#[memory_layout::memory_layout]
#[repr(C)]
pub struct GameStateEditor {
    #[field_offset(0x1420)]
    pub place_transform: Transform,
    #[field_offset(0x1580)]
    pub placing_flip_type: *mut (),
    #[field_offset(0x1588)]
    pub flip_parent: FlipParent,
    #[field_offset(0x26f0)]
    pub dev_ui_visible: bool,
}

#[memory_layout::memory_layout]
#[repr(C)]
pub struct FlipParent {
    #[field_offset(0x1e8)]
    pub cur_flip: u8,
}

#[memory_layout::memory_layout]
#[repr(C)]
pub struct ComponentBase {
    #[field_offset(0x30)]
    pub matrix: [i32; 9],
    #[field_offset(0x58)]
    pub flip_type: *mut (),
}
