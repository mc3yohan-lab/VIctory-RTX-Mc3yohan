//! The legacy multiply and multiply-add as the IEEE operation alone where that is exact (a
//! child module of `lower`, sharing the emitter's state).
//!
//! `v_mul_legacy_f32` gives +0 where either operand is a zero, whatever the other is,
//! infinity and NaN included, and the IEEE product otherwise; `v_mad_legacy_f32` and
//! `v_mac_legacy_f32` add to that product, fused (`Lower::legacy_fmad`: `+0 + c` where a
//! factor is a zero). The default forms are the host's operation, a compare of each factor
//! with zero, their OR and a select. Where what is known of the factors ([`Facts`],
//! `bitcode::facts`) makes the IEEE operation give the same bits for every input, it is taken
//! alone (`VICTORY_LEGACY_PLAIN`, on; `=0` turns it off): every zero either factor may be
//! meets a finite other whose sign makes the IEEE product +0 (a +0 only finite numbers of
//! clear sign, +0 included; a -0 only finite numbers of set sign). Then each zero case is +0
//! by IEEE, and every other case is the product as before. In the multiply-add a product
//! that is exactly +0 is added to `c` as `+0 + c` is, rounded once, so the fused operation
//! alone is exact under the same condition. (A converted unsigned integer times a positive
//! constant, the title's most frequent such pair; the reciprocal of one times 2^32 - 256, the
//! division idiom; a square times the smoothstep factor 3 - 2x.)
//!
//! The switch is part of the translation (the shader cache keys on it); with it off each
//! program is as before, bit for bit.

use super::{Bin, Lower, V};
use crate::emit::bitcode::facts::{Facts, Made};

/// The IEEE operation alone where what is known of the factors makes it exact
/// (`VICTORY_LEGACY_PLAIN=0`: never).
pub(super) fn plain_on() -> bool {
    #[cfg(test)]
    if let Some(on) = TEST_PLAIN.get() {
        return on;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_LEGACY_PLAIN").is_none_or(|v| v != "0"))
}

#[cfg(test)]
thread_local! {
    /// A test's choice for [`plain_on`], for its thread only.
    pub(crate) static TEST_PLAIN: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    /// The legacy multiplies and multiply-adds lowered in the shorter form on this thread.
    pub(crate) static TEST_SHORT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Whether the product alone is exact for factors with facts `fa` and `fb`: every zero
/// either may be makes +0 against every value the other may take.
pub(super) fn plain_exact(fa: &Facts, fb: &Facts) -> bool {
    let zeros_fine = |x: &Facts, y: &Facts| {
        (!x.pz || (y.finite() && y.plus())) && (!x.nz || (y.finite() && y.minus()))
    };
    zeros_fine(fa, fb) && zeros_fine(fb, fa)
}

impl Lower<'_> {
    /// Whether the legacy rule may be left out for factors `a` and `b` ([`plain_exact`]).
    fn legacy_plain(&mut self, a: V, b: V) -> bool {
        if !plain_on() {
            return false;
        }
        let (fa, fb) = (self.s.m.facts(a), self.s.m.facts(b));
        let plain = plain_exact(&fa, &fb);
        #[cfg(test)]
        if plain {
            TEST_SHORT.set(TEST_SHORT.get() + 1);
        }
        plain
    }

    /// The legacy multiply of `a` and `b` as the product alone, where that is exact; `None`
    /// leaves it to the default form.
    pub(super) fn legacy_short(&mut self, a: V, b: V) -> Option<V> {
        if !self.legacy_plain(a, b) {
            return None;
        }
        let r = self.s.m.bin(self.cur, Bin::Mul, a, b);
        self.s.m.made(r, Made::Legacy(a, b));
        Some(r)
    }

    /// The legacy multiply-add of `a`, `b` and `c` as the fused multiply-add alone, where the
    /// product alone is exact; `None` leaves it to the default form.
    pub(super) fn legacy_short_fmad(&mut self, a: V, b: V, c: V) -> Option<V> {
        if !self.legacy_plain(a, b) {
            return None;
        }
        Some(self.fmad(a, b, c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(x: f32) -> Facts {
        Facts::constant(x.to_bits())
    }

    const UINT: Facts = Facts {
        lo: 0.0,
        hi: 4_294_967_296.0,
        nan: false,
        pz: true,
        nz: false,
        least: 1.0,
    };

    #[test]
    fn the_product_alone_where_every_zero_makes_plus_zero() {
        let any = Facts::ANY;
        assert!(plain_exact(&UINT, &k(0.5)));
        assert!(plain_exact(&k(0.5), &UINT));
        assert!(plain_exact(&UINT, &UINT));
        // +0 times -0.5 is -0.
        assert!(!plain_exact(&UINT, &k(-0.5)));
        // Nothing known of one: it may be a zero of either sign, or infinite.
        assert!(!plain_exact(&any, &k(0.5)));
        assert!(!plain_exact(&any, &any));
        // Infinite, NaN and negative zero constants (a negative denormal reads as one)
        // against a value that may be +0.
        for x in [f32::INFINITY, f32::NAN, -0.0, -1e-40] {
            assert!(!plain_exact(&UINT, &k(x)), "{x}");
        }
        // A +0 constant (or a positive denormal) times a finite value of clear sign: +0.
        assert!(plain_exact(&UINT, &k(0.0)) && plain_exact(&UINT, &k(1e-40)));
        // Two values of set sign: -0 times a negative number is +0.
        let below = Facts {
            lo: -4.0,
            hi: 0.0,
            nan: false,
            pz: false,
            nz: true,
            least: 0.0,
        };
        assert!(plain_exact(&below, &below));
        assert!(!plain_exact(&below, &UINT), "-0 times a positive number is -0");
    }
}
