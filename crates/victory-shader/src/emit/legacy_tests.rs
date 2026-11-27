//! The legacy multiply's and multiply-add's IEEE forms (`lower::legacy`, `VICTORY_LEGACY_PLAIN`)
//! against their default forms, end to end: synthetic console programs lowered with the switch
//! on and off, validated by `dxil.dll` when present, and run on the host GPU and WARP over
//! zeros of both signs, denormals, infinities, NaNs and numbers. Each program takes the IEEE
//! form with the switch on where the test says so, and gives the same bits as the default form
//! for every input; the console's own results are checked where the host does not decide them
//! (no denormal operand).

use super::lower::legacy::{TEST_PLAIN, TEST_SHORT};
use super::valu_tests::{build_cs, run_cases};

/// VOP1 `op vdst, src0` (`src0`: 0..105 an SGPR, 128.. inline constants, 256.. a VGPR).
const fn vop1(op: u32, vdst: u32, src0: u32) -> u32 {
    0x7E00_0000 | vdst << 17 | op << 9 | src0
}

/// VOP2 `op vdst, src0, vsrc1`.
const fn vop2(op: u32, vdst: u32, src0: u32, vsrc1: u32) -> u32 {
    op << 25 | vdst << 17 | vsrc1 << 9 | src0
}

/// VOP3 `op vdst, src0, src1, src2`, no modifiers.
const fn vop3(op: u32, vdst: u32, src: [u32; 3]) -> [u32; 2] {
    [
        0x35 << 26 | op << 16 | vdst,
        src[0] | src[1] << 9 | src[2] << 18,
    ]
}

const fn v(n: u32) -> u32 {
    256 + n
}

const MUL_LEGACY: u32 = 0x07;
const MAC_LEGACY: u32 = 0x06;
const MAD_LEGACY: u32 = 0x140;
const MAD: u32 = 0x141;
const MED3: u32 = 0x157;
const MOV: u32 = 0x01;
const CVT_F32_U32: u32 = 0x06;
const RCP: u32 = 0x2A;
/// Inline constants: 0.5, -0.5, 1.0, -2.0, 4.0, the integer 0, a literal.
const HALF: u32 = 240;
const MINUS_HALF: u32 = 241;
const ONE: u32 = 242;
const MINUS_TWO: u32 = 245;
const FOUR: u32 = 246;
const ZERO: u32 = 128;
const LITERAL: u32 = 255;

/// f32 inputs: zeros of both signs, denormals, the smallest normal, numbers, the largest,
/// infinities and NaNs.
const F32_EDGES: [u32; 18] = [
    0x0000_0000,
    0x8000_0000,
    0x0000_0001,
    0x8000_0001,
    0x007F_FFFF,
    0x0080_0000,
    0x3F80_0000,
    0xBF80_0000,
    0x3F00_0000,
    0x4040_0000,
    0x1E3C_E508,
    0x60AD_78EC,
    0xE0AD_78EC,
    0x7F7F_FFFF,
    0x7F80_0000,
    0xFF80_0000,
    0x7FC0_0000,
    0xFFC0_0000,
];

/// u32 inputs, for the converted integers.
const U32_EDGES: [u32; 10] = [
    0,
    1,
    2,
    3,
    255,
    65_535,
    12_345_678,
    0x7FFF_FFFF,
    0x8000_0000,
    0xFFFF_FFFF,
];

fn denormal(bits: u32) -> bool {
    bits & 0x7F80_0000 == 0 && bits & 0x007F_FFFF != 0
}

/// The console's legacy multiply of two values that are not denormals: +0 where either is a
/// zero, the IEEE product otherwise (`None` for a NaN product, whose payload the host picks,
/// and for a denormal one, which the host may flush).
fn mul_legacy(a: u32, b: u32) -> Option<u32> {
    let (x, y) = (f32::from_bits(a), f32::from_bits(b));
    if x == 0.0 || y == 0.0 {
        return Some(0);
    }
    let p = x * y;
    (!p.is_nan() && !denormal(p.to_bits())).then_some(p.to_bits())
}

/// A program of the test: its body (the result in v0; the inputs in v1..v4), its inputs, and
/// whether the switch gives it the IEEE form.
struct Case {
    name: &'static str,
    body: Vec<u32>,
    inputs: Vec<[u32; 4]>,
    short: bool,
    /// The console's result, where the test knows it (not for a denormal operand, which the
    /// host may read as a zero or not).
    console: Option<fn([u32; 4]) -> Option<u32>>,
}

fn cases() -> Vec<Case> {
    let pairs = |a: &[u32], b: &[u32]| -> Vec<[u32; 4]> {
        a.iter()
            .flat_map(|&x| b.iter().map(move |&y| [x, y, 0, 0]))
            .collect()
    };
    let unary = |a: &[u32]| -> Vec<[u32; 4]> { a.iter().map(|&x| [x, 0, 0, 0]).collect() };
    let accumulators = [0x0000_0000u32, 0x8000_0000, 0x3FC0_0000, 0xFF80_0000];
    let mut smooth = vop3(MED3, 6, [v(1), ZERO, ONE]).to_vec();
    smooth.push(vop2(MUL_LEGACY, 7, v(6), 6));
    smooth.extend(vop3(MAD, 8, [v(6), MINUS_TWO, FOUR]));
    smooth.push(vop2(MUL_LEGACY, 0, v(7), 8));
    vec![
        Case {
            name: "v_mul_legacy_f32 v0, v1, v1 (a square of an unknown value: no IEEE form here)",
            body: vec![vop2(MUL_LEGACY, 0, v(1), 1)],
            inputs: unary(&F32_EDGES),
            short: false,
            console: Some(|i| {
                if denormal(i[0]) {
                    None
                } else {
                    mul_legacy(i[0], i[0])
                }
            }),
        },
        Case {
            name: "v_cvt_f32_u32 v5, v1; v_mov_b32 v0, v2; v_mac_legacy_f32 v0, 0.5, v5",
            body: vec![
                vop1(CVT_F32_U32, 5, v(1)),
                vop1(MOV, 0, v(2)),
                vop2(MAC_LEGACY, 0, HALF, 5),
            ],
            inputs: pairs(&U32_EDGES, &accumulators),
            short: true,
            console: None,
        },
        Case {
            name: "v_cvt_f32_u32 v5, v1; v_mad_legacy_f32 v0, v5, 4.0, v2",
            body: [vec![vop1(CVT_F32_U32, 5, v(1))], vop3(MAD_LEGACY, 0, [v(5), FOUR, v(2)]).to_vec()].concat(),
            inputs: pairs(&U32_EDGES, &accumulators),
            short: true,
            console: None,
        },
        Case {
            name: "v_mad_legacy_f32 v0, v1, v1, v2 (nothing known: no IEEE form)",
            body: vop3(MAD_LEGACY, 0, [v(1), v(1), v(2)]).to_vec(),
            inputs: pairs(&F32_EDGES, &accumulators),
            short: false,
            console: None,
        },
        Case {
            name: "v_cvt_f32_u32 v5, v1; v_mul_legacy_f32 v0, 0.5, v5",
            body: vec![vop1(CVT_F32_U32, 5, v(1)), vop2(MUL_LEGACY, 0, HALF, 5)],
            inputs: unary(&U32_EDGES),
            short: true,
            console: Some(|i| mul_legacy(0x3F00_0000, (i[0] as f32).to_bits())),
        },
        Case {
            name: "v_cvt_f32_u32 v5, v1; v_mul_legacy_f32 v0, 2^127, v5 (products that overflow)",
            body: vec![
                vop1(CVT_F32_U32, 5, v(1)),
                vop2(MUL_LEGACY, 0, LITERAL, 5),
                0x7F00_0000,
            ],
            inputs: unary(&U32_EDGES),
            short: true,
            console: Some(|i| mul_legacy(0x7F00_0000, (i[0] as f32).to_bits())),
        },
        Case {
            name: "two converted integers",
            body: vec![
                vop1(CVT_F32_U32, 5, v(1)),
                vop1(CVT_F32_U32, 6, v(2)),
                vop2(MUL_LEGACY, 0, v(5), 6),
            ],
            inputs: pairs(&U32_EDGES, &U32_EDGES),
            short: true,
            console: Some(|i| mul_legacy((i[0] as f32).to_bits(), (i[1] as f32).to_bits())),
        },
        Case {
            name: "the reciprocal of a converted integer times 2^32 - 256 (the division idiom)",
            body: vec![
                vop1(CVT_F32_U32, 5, v(1)),
                vop1(RCP, 6, v(5)),
                vop2(MUL_LEGACY, 0, LITERAL, 6),
                0x4F7F_FFFE,
            ],
            inputs: unary(&U32_EDGES),
            short: true,
            console: None,
        },
        Case {
            name: "a smoothstep: x * x * (4 - 2x), x = v_med3_f32(v1, 0, 1)",
            body: smooth,
            inputs: unary(&F32_EDGES),
            short: true,
            console: None,
        },
        Case {
            name: "v_cvt_f32_u32 v5, v1; v_mul_legacy_f32 v0, -0.5, v5 (+0 times -0.5: no short form)",
            body: vec![
                vop1(CVT_F32_U32, 5, v(1)),
                vop2(MUL_LEGACY, 0, MINUS_HALF, 5),
            ],
            inputs: unary(&U32_EDGES),
            short: false,
            console: Some(|i| mul_legacy(0xBF00_0000, (i[0] as f32).to_bits())),
        },
        Case {
            name: "v_mul_legacy_f32 v0, v1, v2 (nothing known: no short form)",
            body: vec![vop2(MUL_LEGACY, 0, v(1), 2)],
            inputs: pairs(&F32_EDGES, &F32_EDGES),
            short: false,
            console: Some(|i| {
                if denormal(i[0]) || denormal(i[1]) {
                    None
                } else {
                    mul_legacy(i[0], i[1])
                }
            }),
        },
    ]
}

#[test]
fn legacy_ieee_forms_give_the_default_forms_bits() {
    for case in cases() {
        TEST_PLAIN.set(Some(true));
        let before = TEST_SHORT.get();
        let on = build_cs(&case.body, 1, 0);
        let taken = TEST_SHORT.get() > before;
        TEST_PLAIN.set(Some(false));
        let off = build_cs(&case.body, 1, 0);
        TEST_PLAIN.set(None);
        assert_eq!(taken, case.short, "{}: the IEEE form taken", case.name);
        assert_eq!(
            on.dxil != off.dxil,
            case.short,
            "{}: the program changes with the switch",
            case.name
        );
        let mut results: Vec<(String, usize, u32)> = Vec::new();
        run_cases(&off, &case.inputs, |label, i, _, out| {
            results.push((label.to_string(), i, out))
        });
        let mut k = 0;
        run_cases(&on, &case.inputs, |label, i, inputs, out| {
            let (was_label, was_i, was) = &results[k];
            k += 1;
            assert_eq!((label, i), (was_label.as_str(), *was_i));
            assert_eq!(
                out, *was,
                "{} on {label}, inputs {inputs:08x?}: {out:#010x} with the switch, {was:#010x} without",
                case.name
            );
            if let Some(console) = case.console
                && let Some(want) = console(inputs)
            {
                assert_eq!(
                    out, want,
                    "{} on {label}, inputs {inputs:08x?}: not the console's",
                    case.name
                );
            }
        });
        assert_eq!(
            k,
            results.len(),
            "{}: as many results with the switch",
            case.name
        );
    }
}
