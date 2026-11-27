use super::*;
use crate::io;
use crate::ir;
use crate::native::{ComputeIo, HwStage, NativeProgram, StageIo};
use crate::resource;
use crate::structure;

const S_ENDPGM: u32 = 0xBF81_0000;

#[test]
fn constant_lane_masks_in_compute_omit_wave_size_and_wave_ops() {
    // 2D [8, 8, 1] Wave64 compute shader using:
    // 1) `v_cndmask_b32` with an inline constant mask (`5` = lanes 0 and 2)
    // 2) `Atom::Constant` via `s_mov_b64 s[20:21], 12` fed to `v_cndmask_b32`
    let vop3_cndmask = |vdst: u32, src0: u32, src1: u32, src2: u32| -> [u32; 2] {
        [0xD501_0000 | vdst, src0 | (src1 << 9) | (src2 << 18)]
    };
    let mut words = vec![
        0xBE94_048C, // s_mov_b64 s[20:21], 12 (lanes 2 and 3)
    ];
    words.extend_from_slice(&vop3_cndmask(3, 0x8A, 256 + 1, 0x85));
    words.extend_from_slice(&vop3_cndmask(4, 0x8B, 256 + 3, 20));
    words.push(S_ENDPGM);
    let code: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let native = NativeProgram {
        stage: HwStage::Cs,
        wave64: true,
        vgprs: 8,
        layout: vec![],
        used: vec![],
        io: StageIo::Cs(ComputeIo {
            rsrc2: 0,
            threads: [8, 8, 1],
        }),
        code,
    };
    let p = ir::build(&native.code, true, native.vgprs).unwrap();
    let st = structure::structure(&p).unwrap();
    let res = resource::track(&p, &native.layout, native.stage, Some(&st)).unwrap();
    let io = io::raise(&p, &native).unwrap();
    let lowered = lower(&Parts {
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
    .unwrap();
    assert_eq!(lowered.wave_size, None);
    assert!(!lowered.wave_ops);
}

#[cfg(windows)]
#[test]
fn constant_lane_masks_2d_wave64_match_console_lanes_on_gpu_and_warp() {
    use crate::dxbc::{Container, FourCC};
    use crate::emit::host::{Dispatch, Host, Root, View};
    use crate::native::{ResourceType, RootParam, RootType, UsedResource};

    let vop3_cndmask = |vdst: u32, src0: u32, src1: u32, src2: u32| -> [u32; 2] {
        [0xD501_0000 | vdst, src0 | (src1 << 9) | (src2 << 18)]
    };
    let mut prog = vec![
        0xBE84_0300, // s_mov_b32 s4, s0
        0xBE85_0380, // s_mov_b32 s5, 0
        0xF408_0202,
        0xFA00_0000, // s_load_dwordx4 s[8:11], s[4:5], null
        // `s_mov_b64 s[20:21], -16` (inline -16 = 0xD0 -> 0xFFFF_FFFF_FFFF_FFF0, lanes 4..63)
        0xBE94_04D0,
        0x3404_0283, // v_lshlrev_b32 v2, 3, v1 (y * 8)
        0x4A04_0500, // v_add_nc_u32 v2, v0, v2 (flat = y * 8 + x)
    ];
    // v3 = (lane in inline mask 5: lanes 0 and 2) ? 16 : 1
    prog.extend_from_slice(&vop3_cndmask(3, 0x81, 0x90, 0x85));
    // v4 = (lane in s[20:21] = !0xF: lanes 4..63, including 32..63) ? 32 : 0
    prog.extend_from_slice(&vop3_cndmask(4, 0x80, 0xA0, 20));
    prog.push(0x4A06_0903); // v_add_nc_u32 v3, v3, v4
    prog.push(0x340A_0482); // v_lshlrev_b32 v5, 2, v2 (flat * 4)
    prog.push(0xE070_1000);
    prog.push(0x8002_0305); // buffer_store_dword v3, v5, s[8:11], 0 offen
    prog.push(S_ENDPGM);

    let native = NativeProgram {
        stage: HwStage::Cs,
        wave64: true,
        vgprs: 8,
        layout: vec![RootParam {
            root: RootType::Table,
            table_offset_dwords: 0,
            sgprs: 1,
            base_register: 0,
            user_data: 0,
            slot: 0,
        }],
        used: vec![UsedResource {
            kind: ResourceType::Uav,
            root: RootType::Table,
            table_offset_dwords: 0,
            lower_bound: 0,
            stride: 0,
            dwords: 4,
            user_data: 0,
            slot: 0,
        }],
        io: StageIo::Cs(ComputeIo {
            rsrc2: 1 << 11, // TIDIG_COMP_CNT = 1 (v0 = x, v1 = y)
            threads: [8, 8, 1],
        }),
        code: prog.iter().flat_map(|w| w.to_le_bytes()).collect(),
    };
    let empty = crate::dxbc::build(&[(FourCC(*b"SHDR"), &[])]);
    let c = Container::parse(&empty).unwrap();
    let p = ir::build(&native.code, true, native.vgprs).unwrap();
    let st = structure::structure(&p).unwrap();
    let res = resource::track(&p, &native.layout, native.stage, Some(&st)).unwrap();
    let io = io::raise(&p, &native).unwrap();
    let cs = crate::emit::emit(&c, &native, &p, &st, &io, &res).unwrap();

    let _one = crate::emit::valu_tests::ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    for (label, warp) in [("hardware", false), ("warp", true)] {
        let Ok(host) = Host::new(warp) else {
            continue;
        };
        let (root, pso) = host
            .compute_pipeline(&cs)
            .unwrap_or_else(|e| panic!("{label}: compute_pipeline failed: {e}"));
        let mut ud = vec![0u32; cs.user_data_dwords().max(1) as usize];
        for u in &cs.user_data {
            ud[u.index as usize] = 0;
        }
        let out = host
            .dispatch(
                &root,
                &pso,
                &Dispatch {
                    roots: vec![(0, Root::Constants(ud))],
                    views: &[View::RawUav(64)],
                    samplers: &[],
                    groups: [1, 1, 1],
                },
            )
            .unwrap_or_else(|e| panic!("{label}: dispatch failed: {e}"));
        for (lane, &got) in out[0].iter().enumerate().take(64) {
            let want =
                (if lane == 0 || lane == 2 { 16 } else { 1 }) + (if lane >= 4 { 32 } else { 0 });
            assert_eq!(got, want, "{label}: mismatch at console lane {lane}");
        }
    }
}
