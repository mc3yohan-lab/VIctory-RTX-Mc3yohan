use super::{Lowered, Parts, lower};
use crate::native::{ComputeIo, HwStage, NativeProgram, StageIo};
use crate::{io, ir, resource, structure};

const S_ENDPGM: u32 = 0xBF81_0000;

fn lower_cs_wave64_in_group(words: &[u32], threads: [u32; 3]) -> Lowered {
    try_lower_cs_wave64(words, threads).unwrap()
}

fn try_lower_cs_wave64(words: &[u32], threads: [u32; 3]) -> Result<Lowered, crate::ShaderError> {
    let code: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let native = NativeProgram {
        stage: HwStage::Cs,
        wave64: true,
        vgprs: 256,
        layout: vec![],
        used: vec![],
        io: StageIo::Cs(ComputeIo { rsrc2: 0, threads }),
        code,
    };
    let p = ir::build(&native.code, true, 256).unwrap();
    let st = structure::structure(&p).unwrap();
    let res = resource::track(&p, &native.layout, native.stage, Some(&st)).unwrap();
    let io = io::raise(&p, &native).unwrap();
    lower(&Parts {
        native: &native,
        p: &p,
        st: &st,
        io: &io,
        res: &res,
        inputs: vec![],
        outputs: vec![],
        prims: vec![],
        mesh_stride: 0,
        native16: false,
    })
}

fn lower_cs_wave64(words: &[u32]) -> Lowered {
    lower_cs_wave64_in_group(words, [64, 1, 1])
}

#[test]
fn wave64_ds_swizzle_with_thread_id_omits_wave_size() {
    let words = [
        0xD8D4_80B1, // ds_swizzle_b32 v2, v0 offset:0x80b1
        0x0200_0000,
        S_ENDPGM,
    ];
    let lowered = lower_cs_wave64(&words);
    assert_eq!(lowered.wave_size, None);
    assert!(lowered.wave_ops);
}

#[test]
fn wave64_ds_swizzle_across_quad_keeps_wave_size() {
    let lowered = lower_cs_wave64(&[
        0xD8D4_401F, // ds_swizzle_b32 v2, v0 xor 16
        0x0200_0000,
        S_ENDPGM,
    ]);
    assert_eq!(lowered.wave_size, Some(64));
    assert!(lowered.wave_ops);
}

#[test]
fn wave64_dpp_omits_wave_size_only_for_unmasked_quad_ops() {
    for (control, wave_size) in [
        (0xFF00_B100, None),     // quad_perm, all rows and banks
        (0xCF00_B100, Some(64)), // quad_perm, upper rows only
        (0xF500_B100, Some(64)), // quad_perm, selected banks
        (0xFF01_0100, Some(64)), // row_shl:1
        (0x5F01_4000, Some(64)), // row_mirror, repeating row mask
    ] {
        let lowered = lower_cs_wave64(&[0x7E04_02FA, control, S_ENDPGM]);
        assert_eq!(lowered.wave_size, wave_size, "DPP control {control:#x}");
        assert!(lowered.wave_ops);
    }
}

#[test]
fn wave64_dpp_wave_shl1_keeps_wave_size_64() {
    let words = [
        0x7E04_02FA, // v_mov_b32_dpp v2, v0 wave_shl:1
        0xFF01_3000,
        S_ENDPGM,
    ];
    let lowered = lower_cs_wave64(&words);
    assert_eq!(lowered.wave_size, Some(64));
    assert!(lowered.wave_ops);
}

#[test]
fn wave64_quad_ops_preserve_multidimensional_group_lane_order() {
    let swizzle = [0xD8D4_804E, 0x0200_0000, S_ENDPGM];
    let dpp = [0x7E04_02FA, 0xFF00_4E00, S_ENDPGM];
    for threads in [[8, 2, 1], [4, 2, 2]] {
        for words in [swizzle, dpp] {
            assert_eq!(
                lower_cs_wave64_in_group(&words, threads).wave_size,
                Some(64)
            );
        }
    }
}

#[test]
fn wave64_quad_ops_preserve_a_whole_wave_first_lane_read() {
    let words = [
        0xD8D4_804E,
        0x0200_0000, // ds_swizzle_b32 v2, v0
        0x7E00_0500, // v_readfirstlane_b32 s0, v0
        S_ENDPGM,
    ];
    assert_eq!(lower_cs_wave64(&words).wave_size, Some(64));
}

#[test]
fn wave64_quad_dpp_fetch_inactive_keeps_its_wave_size() {
    let words = [0x7E04_02FA, 0xFF04_B100, S_ENDPGM]; // quad_perm, fi:1
    assert_eq!(lower_cs_wave64(&words).wave_size, Some(64));
}

#[test]
fn wave64_rotate_and_fft_swizzles_keep_their_wave_size() {
    for offset in [0xC020, 0xC421, 0xE000, 0xE010, 0xE01F] {
        let lowered = lower_cs_wave64(&[0xD8D4_0000 | offset, 0x0200_0000, S_ENDPGM]);
        assert_eq!(lowered.wave_size, Some(64), "swizzle {offset:#06x}");
    }
}

#[test]
fn a_live_rotate_or_fft_swizzle_is_rejected_until_supported() {
    for offset in [0xC020, 0xE000, 0xE01F] {
        let words = [
            0xD8D4_0000 | offset,
            0x0200_0000, // ds_swizzle_b32 v2, v0
            0x7E00_0502, // v_readfirstlane_b32 s0, v2
            0xBF07_8000, // s_cmp_lg_u32 s0, 0
            0xBF85_0001, // s_cbranch_scc1 -> s_endpgm
            0xBE83_0380, // s_mov_b32 s3, 0
            S_ENDPGM,
        ];
        assert!(matches!(
            try_lower_cs_wave64(&words, [64, 1, 1]),
            Err(crate::ShaderError::Unsupported {
                what: "a rotate or FFT DS swizzle",
                ..
            })
        ));
    }
}
