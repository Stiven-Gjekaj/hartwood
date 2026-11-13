//! IEEE 754 binary32 and binary64 arithmetic in software, with the five
//! rounding modes and the five exception flags of RISC-V.
//!
//! The host FPU rounds to nearest only, and it does not report the flags in
//! a portable way. This module computes each result exactly in integers,
//! then rounds it once.
//!
//! A finite value is `(-1)^sign * sig * 2^exp`, with `sig` an integer in a
//! `u128`. Each operation makes an exact `sig` with spare low bits, or folds
//! the bits that it drops into the lowest bit (a "sticky" bit). Then
//! [`round_pack`] rounds to the precision of the format.
//!
//! RISC-V has its own NaN rules: an operation that makes a NaN gives the
//! canonical NaN, and tininess is detected after rounding.

/// A rounding mode, with the encoding of the `rm` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rm {
    /// To nearest, ties to even.
    Rne = 0,
    /// Toward zero.
    Rtz = 1,
    /// Down, toward negative infinity.
    Rdn = 2,
    /// Up, toward positive infinity.
    Rup = 3,
    /// To nearest, ties away from zero.
    Rmm = 4,
}

impl Rm {
    /// The rounding mode for a 3-bit field. 5 and 6 are reserved, and 7
    /// (dynamic) is resolved by the caller.
    pub fn from_bits(bits: u64) -> Option<Rm> {
        Some(match bits {
            0 => Rm::Rne,
            1 => Rm::Rtz,
            2 => Rm::Rdn,
            3 => Rm::Rup,
            4 => Rm::Rmm,
            _ => return None,
        })
    }
}

// The exception flags, as in `fflags`.
pub const NX: u8 = 1 << 0;
pub const UF: u8 = 1 << 1;
pub const OF: u8 = 1 << 2;
pub const DZ: u8 = 1 << 3;
pub const NV: u8 = 1 << 4;

/// A floating point format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    exp_bits: u32,
    /// The bits of the fraction, without the hidden bit.
    man_bits: u32,
}

pub const F32: Format = Format {
    exp_bits: 8,
    man_bits: 23,
};
pub const F64: Format = Format {
    exp_bits: 11,
    man_bits: 52,
};

impl Format {
    fn bias(self) -> i32 {
        (1 << (self.exp_bits - 1)) - 1
    }

    /// The precision: the bits of the significand with the hidden bit.
    fn precision(self) -> u32 {
        self.man_bits + 1
    }

    fn exp_field_max(self) -> u64 {
        (1 << self.exp_bits) - 1
    }

    fn man_mask(self) -> u64 {
        (1 << self.man_bits) - 1
    }

    fn sign_bit(self) -> u64 {
        1 << (self.exp_bits + self.man_bits)
    }

    /// The exponent of the lowest bit of a subnormal value.
    fn min_lsb_exp(self) -> i32 {
        1 - self.bias() - self.man_bits as i32
    }

    /// The canonical NaN of RISC-V: a quiet NaN with a positive sign and
    /// no payload.
    pub fn canonical_nan(self) -> u64 {
        (self.exp_field_max() << self.man_bits) | (1 << (self.man_bits - 1))
    }

    fn infinity(self, sign: bool) -> u64 {
        self.pack_sign(sign) | (self.exp_field_max() << self.man_bits)
    }

    fn zero(self, sign: bool) -> u64 {
        self.pack_sign(sign)
    }

    /// The largest finite value.
    fn max_finite(self, sign: bool) -> u64 {
        self.pack_sign(sign) | ((self.exp_field_max() - 1) << self.man_bits) | self.man_mask()
    }

    fn pack_sign(self, sign: bool) -> u64 {
        if sign { self.sign_bit() } else { 0 }
    }
}

/// The class of a value, as the decoder of an operand sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Zero,
    Finite,
    Inf,
    QuietNan,
    SignalingNan,
}

/// One operand, taken apart.
#[derive(Debug, Clone, Copy)]
struct Value {
    sign: bool,
    kind: Kind,
    /// For a finite value: the value is `sig * 2^exp`.
    exp: i32,
    sig: u128,
}

impl Value {
    fn is_nan(self) -> bool {
        matches!(self.kind, Kind::QuietNan | Kind::SignalingNan)
    }
}

fn unpack(f: Format, bits: u64) -> Value {
    let sign = bits & f.sign_bit() != 0;
    let exp_field = (bits >> f.man_bits) & f.exp_field_max();
    let frac = bits & f.man_mask();
    let quiet_bit = 1 << (f.man_bits - 1);
    let (kind, exp, sig) = if exp_field == f.exp_field_max() {
        if frac == 0 {
            (Kind::Inf, 0, 0)
        } else if frac & quiet_bit != 0 {
            (Kind::QuietNan, 0, 0)
        } else {
            (Kind::SignalingNan, 0, 0)
        }
    } else if exp_field == 0 {
        if frac == 0 {
            (Kind::Zero, 0, 0)
        } else {
            (Kind::Finite, f.min_lsb_exp(), u128::from(frac))
        }
    } else {
        let exp = exp_field as i32 - f.bias() - f.man_bits as i32;
        (Kind::Finite, exp, u128::from(frac | (1 << f.man_bits)))
    };
    Value {
        sign,
        kind,
        exp,
        sig,
    }
}

/// The number of bits in `x`.
fn bit_len(x: u128) -> i32 {
    128 - x.leading_zeros() as i32
}

/// Shifts right by `n`, and folds each dropped one bit into bit 0.
fn shift_right_jam(x: u128, n: u32) -> u128 {
    if n == 0 {
        x
    } else if n >= 128 {
        u128::from(x != 0)
    } else {
        (x >> n) | u128::from(x & ((1 << n) - 1) != 0)
    }
}

/// Tells if a rounding moves the kept part up by one unit.
///
/// `odd` is the lowest kept bit. `cmp` compares the dropped part with half
/// a unit. `inexact` says if the dropped part is not zero.
fn rounds_up(rm: Rm, sign: bool, odd: bool, cmp: std::cmp::Ordering, inexact: bool) -> bool {
    use std::cmp::Ordering::*;
    match rm {
        Rm::Rne => cmp == Greater || (cmp == Equal && odd),
        Rm::Rmm => cmp != Less,
        Rm::Rtz => false,
        Rm::Rdn => sign && inexact,
        Rm::Rup => !sign && inexact,
    }
}

/// Rounds `sig * 2^exp` so that its lowest kept bit has the exponent
/// `lsb_exp`. Gives the kept integer, which can have one bit more than
/// before when the rounding carries, and whether the rounding was inexact.
fn round_at(sig: u128, exp: i32, lsb_exp: i32, rm: Rm, sign: bool) -> (u128, bool) {
    let drop = lsb_exp - exp;
    if drop <= 0 {
        return (sig << (-drop) as u32, false);
    }
    let drop = drop as u32;
    let (kept, rest, cmp) = if drop >= 128 {
        // Each bit is dropped. `sig` has fewer than 128 bits, so it is
        // below half a unit.
        (0, sig, std::cmp::Ordering::Less)
    } else {
        let rest = sig & ((1 << drop) - 1);
        (sig >> drop, rest, rest.cmp(&(1u128 << (drop - 1))))
    };
    let inexact = rest != 0;
    let up = rounds_up(rm, sign, kept & 1 == 1, cmp, inexact);
    (kept + u128::from(up), inexact)
}

/// Rounds an exact nonzero value `(-1)^sign * sig * 2^exp` to the format,
/// and gives its bits and the flags that the rounding raises.
fn round_pack(f: Format, sign: bool, exp: i32, sig: u128, rm: Rm) -> (u64, u8) {
    debug_assert!(sig != 0);
    let p = f.precision() as i32;
    // The exponent of the leading bit.
    let lead = exp + bit_len(sig) - 1;
    let lsb_exp = (lead - (p - 1)).max(f.min_lsb_exp());
    let (mut q, inexact) = round_at(sig, exp, lsb_exp, rm, sign);
    let mut lsb_exp = lsb_exp;
    if bit_len(q) > p {
        q >>= 1;
        lsb_exp += 1;
    }
    let mut flags = if inexact { NX } else { 0 };

    // Tininess after rounding: round again with no lower limit on the
    // exponent, and see if that result is below the smallest normal value.
    if inexact {
        let lsb_unbounded = lead - (p - 1);
        let (q2, _) = round_at(sig, exp, lsb_unbounded, rm, sign);
        let lead2 = lsb_unbounded + bit_len(q2) - 1;
        if lead2 < 1 - f.bias() {
            flags |= UF;
        }
    }

    if q == 0 {
        return (f.zero(sign), flags);
    }
    let lead = lsb_exp + bit_len(q) - 1;
    let exp_field = lead + f.bias();
    if exp_field >= f.exp_field_max() as i32 {
        let to_inf = match rm {
            Rm::Rne | Rm::Rmm => true,
            Rm::Rtz => false,
            Rm::Rdn => sign,
            Rm::Rup => !sign,
        };
        let bits = if to_inf {
            f.infinity(sign)
        } else {
            f.max_finite(sign)
        };
        return (bits, OF | NX);
    }
    let bits = if bit_len(q) < p {
        // Subnormal: the exponent field is zero.
        q as u64
    } else {
        ((exp_field as u64) << f.man_bits) | (q as u64 & f.man_mask())
    };
    (f.pack_sign(sign) | bits, flags)
}

/// The result of an operation: the bits and the flags.
pub type Out = (u64, u8);

/// A NaN result: the canonical NaN, and NV if an input was signaling.
fn nan_result(f: Format, inputs: &[Value]) -> Out {
    let nv = inputs.iter().any(|v| v.kind == Kind::SignalingNan);
    (f.canonical_nan(), if nv { NV } else { 0 })
}

/// Adds two exact values and rounds. Each `sig` has at most 126 bits.
fn add_values(f: Format, a: Value, b: Value, rm: Rm) -> Out {
    // Put each leading bit at bit 125. Then the larger exponent sets the
    // scale, and the smaller value shifts right with a sticky bit.
    let norm = |v: Value| -> (i32, u128) {
        if v.sig == 0 {
            return (i32::MIN, 0);
        }
        let s = 126 - bit_len(v.sig);
        (v.exp - s, v.sig << s as u32)
    };
    let (ea, sa) = norm(a);
    let (eb, sb) = norm(b);
    let (exp, a_sig, b_sig) = if ea >= eb {
        (ea, sa, shift_right_jam(sb, ea.saturating_sub(eb) as u32))
    } else {
        (eb, shift_right_jam(sa, eb.saturating_sub(ea) as u32), sb)
    };
    let (sign, sig) = if a.sign == b.sign {
        (a.sign, a_sig + b_sig)
    } else if a_sig >= b_sig {
        (a.sign, a_sig - b_sig)
    } else {
        (b.sign, b_sig - a_sig)
    };
    if sig == 0 {
        // An exact zero sum is +0, or -0 when rounding down.
        let zero_sign = if a.sign == b.sign {
            a.sign
        } else {
            rm == Rm::Rdn
        };
        return (f.zero(zero_sign), 0);
    }
    round_pack(f, sign, exp, sig, rm)
}

pub fn add(f: Format, a: u64, b: u64, rm: Rm) -> Out {
    let (a, b) = (unpack(f, a), unpack(f, b));
    if a.is_nan() || b.is_nan() {
        return nan_result(f, &[a, b]);
    }
    match (a.kind, b.kind) {
        (Kind::Inf, Kind::Inf) if a.sign != b.sign => (f.canonical_nan(), NV),
        (Kind::Inf, _) => (f.infinity(a.sign), 0),
        (_, Kind::Inf) => (f.infinity(b.sign), 0),
        _ => add_values(f, a, b, rm),
    }
}

pub fn sub(f: Format, a: u64, b: u64, rm: Rm) -> Out {
    add(f, a, b ^ f.sign_bit(), rm)
}

pub fn mul(f: Format, a: u64, b: u64, rm: Rm) -> Out {
    let (a, b) = (unpack(f, a), unpack(f, b));
    if a.is_nan() || b.is_nan() {
        return nan_result(f, &[a, b]);
    }
    let sign = a.sign != b.sign;
    match (a.kind, b.kind) {
        (Kind::Inf, Kind::Zero) | (Kind::Zero, Kind::Inf) => (f.canonical_nan(), NV),
        (Kind::Inf, _) | (_, Kind::Inf) => (f.infinity(sign), 0),
        (Kind::Zero, _) | (_, Kind::Zero) => (f.zero(sign), 0),
        _ => round_pack(f, sign, a.exp + b.exp, a.sig * b.sig, rm),
    }
}

pub fn div(f: Format, a: u64, b: u64, rm: Rm) -> Out {
    let (a, b) = (unpack(f, a), unpack(f, b));
    if a.is_nan() || b.is_nan() {
        return nan_result(f, &[a, b]);
    }
    let sign = a.sign != b.sign;
    match (a.kind, b.kind) {
        (Kind::Inf, Kind::Inf) | (Kind::Zero, Kind::Zero) => (f.canonical_nan(), NV),
        (Kind::Inf, _) => (f.infinity(sign), 0),
        (_, Kind::Inf) => (f.zero(sign), 0),
        (_, Kind::Zero) => (f.infinity(sign), DZ),
        (Kind::Zero, _) => (f.zero(sign), 0),
        _ => {
            // Shift the dividend up so the quotient has far more bits than
            // the precision. A remainder becomes the sticky bit.
            let s = 125 - bit_len(a.sig);
            let num = a.sig << s as u32;
            let q = num / b.sig;
            let sticky = u128::from(!num.is_multiple_of(b.sig));
            round_pack(f, sign, a.exp - s - b.exp, q | sticky, rm)
        }
    }
}

/// The integer square root of `x`, rounded down.
fn isqrt(x: u128) -> u128 {
    if x == 0 {
        return 0;
    }
    // Newton's method from a start above the root.
    let mut r = 1u128 << ((bit_len(x) + 1) / 2);
    loop {
        let next = (r + x / r) / 2;
        if next >= r {
            return r;
        }
        r = next;
    }
}

pub fn sqrt(f: Format, a: u64, rm: Rm) -> Out {
    let a = unpack(f, a);
    if a.is_nan() {
        return nan_result(f, &[a]);
    }
    match a.kind {
        Kind::Zero => (f.zero(a.sign), 0),
        _ if a.sign => (f.canonical_nan(), NV),
        Kind::Inf => (f.infinity(false), 0),
        _ => {
            // Shift so the exponent is even and the root has many spare
            // bits.
            let mut s = 124 - bit_len(a.sig);
            if (a.exp - s) % 2 != 0 {
                s -= 1;
            }
            let x = a.sig << s as u32;
            let r = isqrt(x);
            let sticky = u128::from(r * r != x);
            round_pack(f, false, (a.exp - s) / 2, r | sticky, rm)
        }
    }
}

/// Computes `(a * b) + c` with one rounding. `negate_product` and
/// `negate_addend` give the four fused forms.
pub fn fma(
    f: Format,
    a: u64,
    b: u64,
    c: u64,
    negate_product: bool,
    negate_addend: bool,
    rm: Rm,
) -> Out {
    let (a, b, mut c) = (unpack(f, a), unpack(f, b), unpack(f, c));
    let inf_times_zero = matches!(
        (a.kind, b.kind),
        (Kind::Inf, Kind::Zero) | (Kind::Zero, Kind::Inf)
    );
    // RISC-V raises NV for infinity times zero even when the addend is a
    // quiet NaN.
    if inf_times_zero {
        return (f.canonical_nan(), NV);
    }
    if a.is_nan() || b.is_nan() || c.is_nan() {
        return nan_result(f, &[a, b, c]);
    }
    c.sign ^= negate_addend;
    let psign = (a.sign != b.sign) ^ negate_product;
    let product_inf = a.kind == Kind::Inf || b.kind == Kind::Inf;
    if product_inf {
        if c.kind == Kind::Inf && c.sign != psign {
            return (f.canonical_nan(), NV);
        }
        return (f.infinity(psign), 0);
    }
    if c.kind == Kind::Inf {
        return (f.infinity(c.sign), 0);
    }
    let product_zero = a.kind == Kind::Zero || b.kind == Kind::Zero;
    let product = Value {
        sign: psign,
        kind: if product_zero {
            Kind::Zero
        } else {
            Kind::Finite
        },
        exp: a.exp + b.exp,
        sig: if product_zero { 0 } else { a.sig * b.sig },
    };
    if product_zero && c.kind == Kind::Zero {
        let sign = if psign == c.sign {
            psign
        } else {
            rm == Rm::Rdn
        };
        return (f.zero(sign), 0);
    }
    add_values(f, product, c, rm)
}

/// The result of a minimum or a maximum. A NaN loses to a number, and -0 is
/// below +0.
fn min_max(f: Format, a_bits: u64, b_bits: u64, max: bool) -> Out {
    let (a, b) = (unpack(f, a_bits), unpack(f, b_bits));
    let nv = if a.kind == Kind::SignalingNan || b.kind == Kind::SignalingNan {
        NV
    } else {
        0
    };
    let bits = match (a.is_nan(), b.is_nan()) {
        (true, true) => f.canonical_nan(),
        (true, false) => b_bits,
        (false, true) => a_bits,
        _ => {
            let a_less = less(f, a_bits, b_bits) || (a.sign && !b.sign);
            if a_less != max { a_bits } else { b_bits }
        }
    };
    (bits, nv)
}

pub fn min(f: Format, a: u64, b: u64) -> Out {
    min_max(f, a, b, false)
}

pub fn max(f: Format, a: u64, b: u64) -> Out {
    min_max(f, a, b, true)
}

/// Compares two numbers that are not NaN. +0 and -0 are equal.
fn less(f: Format, a_bits: u64, b_bits: u64) -> bool {
    let (a, b) = (unpack(f, a_bits), unpack(f, b_bits));
    let mag = |bits: u64| bits & !f.sign_bit();
    if a.kind == Kind::Zero && b.kind == Kind::Zero {
        return false;
    }
    match (a.sign, b.sign) {
        (true, false) => true,
        (false, true) => false,
        (false, false) => mag(a_bits) < mag(b_bits),
        (true, true) => mag(a_bits) > mag(b_bits),
    }
}

/// `feq`: a quiet compare. Only a signaling NaN raises NV.
pub fn eq(f: Format, a_bits: u64, b_bits: u64) -> Out {
    let (a, b) = (unpack(f, a_bits), unpack(f, b_bits));
    if a.is_nan() || b.is_nan() {
        return (0, nan_result(f, &[a, b]).1);
    }
    let equal = a_bits == b_bits || (a.kind == Kind::Zero && b.kind == Kind::Zero);
    (u64::from(equal), 0)
}

/// `flt`: a signaling compare. Any NaN raises NV.
pub fn lt(f: Format, a_bits: u64, b_bits: u64) -> Out {
    let (a, b) = (unpack(f, a_bits), unpack(f, b_bits));
    if a.is_nan() || b.is_nan() {
        return (0, NV);
    }
    (u64::from(less(f, a_bits, b_bits)), 0)
}

/// `fle`: a signaling compare. Any NaN raises NV.
pub fn le(f: Format, a_bits: u64, b_bits: u64) -> Out {
    let (lt_bits, nv) = lt(f, a_bits, b_bits);
    if nv != 0 {
        return (0, nv);
    }
    let (eq_bits, _) = eq(f, a_bits, b_bits);
    (lt_bits | eq_bits, 0)
}

/// `fclass`: one bit of ten that names the class of the value.
pub fn classify(f: Format, bits: u64) -> u64 {
    let v = unpack(f, bits);
    let subnormal = v.kind == Kind::Finite && (bits >> f.man_bits) & f.exp_field_max() == 0;
    let bit = match (v.kind, v.sign) {
        (Kind::Inf, true) => 0,
        (Kind::Finite, true) if !subnormal => 1,
        (Kind::Finite, true) => 2,
        (Kind::Zero, true) => 3,
        (Kind::Zero, false) => 4,
        (Kind::Finite, false) if subnormal => 5,
        (Kind::Finite, false) => 6,
        (Kind::Inf, false) => 7,
        (Kind::SignalingNan, _) => 8,
        (Kind::QuietNan, _) => 9,
    };
    1 << bit
}

/// Converts between the two formats. A NaN becomes the canonical NaN.
pub fn convert(from: Format, to: Format, bits: u64, rm: Rm) -> Out {
    let v = unpack(from, bits);
    match v.kind {
        Kind::QuietNan | Kind::SignalingNan => nan_result(to, &[v]),
        Kind::Inf => (to.infinity(v.sign), 0),
        Kind::Zero => (to.zero(v.sign), 0),
        Kind::Finite => round_pack(to, v.sign, v.exp, v.sig, rm),
    }
}

/// Converts an integer to a float. `signed` says how to read `x`.
pub fn from_int(f: Format, x: u64, signed: bool, rm: Rm) -> Out {
    let (sign, mag) = if signed && (x as i64) < 0 {
        (true, u128::from((x as i64).unsigned_abs()))
    } else {
        (false, u128::from(x))
    };
    if mag == 0 {
        return (f.zero(false), 0);
    }
    round_pack(f, sign, 0, mag, rm)
}

/// Converts a float to an integer of `width` bits (32 or 64). A NaN or a
/// value out of range gives the nearest end of the range and NV. The result
/// is sign extended to 64 bits, as RISC-V writes it.
pub fn to_int(f: Format, bits: u64, width: u32, signed: bool, rm: Rm) -> Out {
    let v = unpack(f, bits);
    let (min, max): (i128, i128) = if signed {
        (-(1i128 << (width - 1)), (1i128 << (width - 1)) - 1)
    } else {
        (0, (1i128 << width) - 1)
    };
    let saturate = |value: i128| -> u64 {
        if width == 32 {
            value as i32 as i64 as u64
        } else {
            value as u64
        }
    };
    match v.kind {
        Kind::QuietNan | Kind::SignalingNan => return (saturate(max), NV),
        Kind::Inf => return (saturate(if v.sign { min } else { max }), NV),
        Kind::Zero => return (0, 0),
        Kind::Finite => {}
    }
    // A value of 2^128 or more is out of every range.
    if v.exp + bit_len(v.sig) > 127 {
        return (saturate(if v.sign { min } else { max }), NV);
    }
    let (mag, inexact) = round_at(v.sig, v.exp, 0, rm, v.sign);
    let value = if v.sign { -(mag as i128) } else { mag as i128 };
    if value < min || value > max {
        return (saturate(if v.sign { min } else { max }), NV);
    }
    (saturate(value), if inexact { NX } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RMS: [Rm; 5] = [Rm::Rne, Rm::Rtz, Rm::Rdn, Rm::Rup, Rm::Rmm];

    /// A small random generator, so the tests repeat exactly.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        /// A double with an exponent near 0, so most results are normal,
        /// and sometimes a special value.
        fn f64(&mut self) -> f64 {
            let r = self.next();
            match r % 32 {
                0 => 0.0,
                1 => f64::INFINITY,
                2 => f64::MIN_POSITIVE / 8.0,
                3 => -f64::MAX,
                _ => {
                    let exp = (r >> 8) % 120;
                    let bits = ((1023 - 60 + exp) << 52) | (self.next() & ((1 << 52) - 1));
                    let v = f64::from_bits(bits);
                    if r & 0x10000 != 0 { -v } else { v }
                }
            }
        }

        /// Any bit pattern of a double, with subnormal and huge values.
        fn any_f64(&mut self) -> f64 {
            f64::from_bits(self.next())
        }
    }

    /// The host rounds to nearest, ties to even, and its results are
    /// correctly rounded. Each result in that mode must match the host, bit
    /// for bit, except that a NaN is the canonical NaN.
    fn host(v: f64) -> u64 {
        if v.is_nan() {
            F64.canonical_nan()
        } else {
            v.to_bits()
        }
    }

    #[test]
    fn nearest_even_matches_the_host_for_random_doubles() {
        let mut rng = Rng(0x1234_5678_9abc_def1);
        for n in 0..200_000 {
            let (a, b, c) = if n % 2 == 0 {
                (rng.f64(), rng.f64(), rng.f64())
            } else {
                (rng.any_f64(), rng.any_f64(), rng.any_f64())
            };
            let (ab, bb, cb) = (a.to_bits(), b.to_bits(), c.to_bits());
            assert_eq!(add(F64, ab, bb, Rm::Rne).0, host(a + b), "{a:e} + {b:e}");
            assert_eq!(sub(F64, ab, bb, Rm::Rne).0, host(a - b), "{a:e} - {b:e}");
            assert_eq!(mul(F64, ab, bb, Rm::Rne).0, host(a * b), "{a:e} * {b:e}");
            assert_eq!(div(F64, ab, bb, Rm::Rne).0, host(a / b), "{a:e} / {b:e}");
            assert_eq!(sqrt(F64, ab, Rm::Rne).0, host(a.sqrt()), "sqrt {a:e}");
            assert_eq!(
                fma(F64, ab, bb, cb, false, false, Rm::Rne).0,
                host(a.mul_add(b, c)),
                "{a:e} * {b:e} + {c:e}"
            );
        }
    }

    #[test]
    fn nearest_even_matches_the_host_for_random_singles() {
        let mut rng = Rng(0x0fed_cba9_8765_4321);
        let h = |v: f32| {
            if v.is_nan() {
                F32.canonical_nan()
            } else {
                u64::from(v.to_bits())
            }
        };
        for _ in 0..200_000 {
            let (a, b, c) = (
                f32::from_bits(rng.next() as u32),
                f32::from_bits(rng.next() as u32),
                f32::from_bits(rng.next() as u32),
            );
            let (ab, bb, cb) = (
                u64::from(a.to_bits()),
                u64::from(b.to_bits()),
                u64::from(c.to_bits()),
            );
            assert_eq!(add(F32, ab, bb, Rm::Rne).0, h(a + b));
            assert_eq!(mul(F32, ab, bb, Rm::Rne).0, h(a * b));
            assert_eq!(div(F32, ab, bb, Rm::Rne).0, h(a / b));
            assert_eq!(sqrt(F32, ab, Rm::Rne).0, h(a.sqrt()));
            assert_eq!(
                fma(F32, ab, bb, cb, false, false, Rm::Rne).0,
                h(a.mul_add(b, c))
            );
            assert_eq!(convert(F32, F64, ab, Rm::Rne).0, host(f64::from(a)));
        }
    }

    /// The exact error of `a + b` against its rounded sum, by Knuth's
    /// TwoSum. Its sign says on which side of the exact sum the rounded
    /// result lies.
    fn two_sum_error(a: f64, b: f64) -> f64 {
        let s = a + b;
        let bb = s - a;
        (a - (s - bb)) + (b - bb)
    }

    #[test]
    fn each_directed_mode_rounds_to_its_side() {
        let mut rng = Rng(0x5555_aaaa_3333_cccc);
        for _ in 0..100_000 {
            let (a, b) = (rng.f64(), rng.f64());
            let s = a + b;
            if !s.is_finite() || s == 0.0 {
                continue;
            }
            let err = two_sum_error(a, b);
            let (ab, bb) = (a.to_bits(), b.to_bits());
            let down = f64::from_bits(add(F64, ab, bb, Rm::Rdn).0);
            let up = f64::from_bits(add(F64, ab, bb, Rm::Rup).0);
            let zero = f64::from_bits(add(F64, ab, bb, Rm::Rtz).0);
            let exact = err == 0.0;
            assert_eq!(add(F64, ab, bb, Rm::Rdn).1 & NX == 0, exact);
            if exact {
                assert_eq!((down, up, zero), (s, s, s));
            } else if err > 0.0 {
                // The exact sum is above s, so s is the result that rounds
                // down, and its upper neighbour is the one that rounds up.
                assert_eq!((down, up), (s, s.next_up()));
            } else {
                assert_eq!((down, up), (s.next_down(), s));
            }
            if !exact {
                let toward_zero = if s > 0.0 { down } else { up };
                assert_eq!(zero, toward_zero);
            }
            assert_eq!(f64::from_bits(add(F64, ab, bb, Rm::Rne).0), s);
            for rm in RMS {
                let r = f64::from_bits(add(F64, ab, bb, rm).0);
                assert!(r == down || r == up, "{rm:?}");
            }
        }
    }

    #[test]
    fn overflow_follows_the_rounding_mode() {
        let max = f64::MAX.to_bits();
        let (inf, nmax) = (f64::INFINITY.to_bits(), (-f64::MAX).to_bits());
        assert_eq!(add(F64, max, max, Rm::Rne), (inf, OF | NX));
        assert_eq!(add(F64, max, max, Rm::Rtz), (max, OF | NX));
        assert_eq!(add(F64, nmax, nmax, Rm::Rup), (nmax, OF | NX));
        assert_eq!(
            add(F64, nmax, nmax, Rm::Rdn),
            ((-f64::INFINITY).to_bits(), OF | NX)
        );
    }

    #[test]
    fn an_exact_tiny_result_does_not_raise_underflow() {
        let tiny = 1u64; // the smallest subnormal
        assert_eq!(add(F64, tiny, tiny, Rm::Rne), (2, 0));
        // Half of the smallest subnormal rounds to zero: inexact and tiny.
        let half = (0.5f64).to_bits();
        assert_eq!(mul(F64, tiny, half, Rm::Rne), (0, UF | NX));
    }

    #[test]
    fn tininess_is_detected_after_rounding() {
        // The largest subnormal times (1 + 2^-52) rounds up to the smallest
        // normal value. Before the rounding it is tiny, after it is not, so
        // there is no underflow.
        let big_sub = f64::MIN_POSITIVE.to_bits() - 1;
        let one_plus = (1.0f64).to_bits() + 1;
        let (bits, flags) = mul(F64, big_sub, one_plus, Rm::Rup);
        assert_eq!(bits, f64::MIN_POSITIVE.to_bits());
        assert_eq!(flags, NX);
    }

    #[test]
    fn the_special_cases_raise_the_right_flags() {
        let (zero, one) = (0f64.to_bits(), 1f64.to_bits());
        let inf = f64::INFINITY.to_bits();
        let snan = 0x7ff0_0000_0000_0001u64;
        let qnan = F64.canonical_nan();
        assert_eq!(div(F64, one, zero, Rm::Rne), (inf, DZ));
        assert_eq!(div(F64, zero, zero, Rm::Rne), (qnan, NV));
        assert_eq!(sqrt(F64, (-1f64).to_bits(), Rm::Rne), (qnan, NV));
        assert_eq!(
            sqrt(F64, (-0f64).to_bits(), Rm::Rne),
            ((-0f64).to_bits(), 0)
        );
        assert_eq!(add(F64, snan, one, Rm::Rne), (qnan, NV));
        assert_eq!(add(F64, qnan, one, Rm::Rne), (qnan, 0));
        assert_eq!(fma(F64, inf, zero, qnan, false, false, Rm::Rne), (qnan, NV));
        assert_eq!(sub(F64, one, one, Rm::Rdn), ((-0f64).to_bits(), 0));
        assert_eq!(sub(F64, one, one, Rm::Rne), (zero, 0));
    }

    #[test]
    fn min_and_max_prefer_a_number_and_order_the_zeros() {
        let (p0, n0) = (0f64.to_bits(), (-0f64).to_bits());
        let one = 1f64.to_bits();
        let qnan = F64.canonical_nan();
        let snan = 0x7ff0_0000_0000_0001u64;
        assert_eq!(min(F64, p0, n0), (n0, 0));
        assert_eq!(max(F64, n0, p0), (p0, 0));
        assert_eq!(min(F64, qnan, one), (one, 0));
        assert_eq!(max(F64, snan, one), (one, NV));
        assert_eq!(min(F64, qnan, qnan), (qnan, 0));
    }

    #[test]
    fn compares_handle_nan_and_signed_zero() {
        let (p0, n0) = (0f64.to_bits(), (-0f64).to_bits());
        let qnan = F64.canonical_nan();
        assert_eq!(eq(F64, p0, n0), (1, 0));
        assert_eq!(lt(F64, n0, p0), (0, 0));
        assert_eq!(le(F64, n0, p0), (1, 0));
        assert_eq!(eq(F64, qnan, p0), (0, 0));
        assert_eq!(lt(F64, qnan, p0), (0, NV));
    }

    #[test]
    fn a_conversion_to_an_integer_saturates() {
        let big = 1e30f64.to_bits();
        let nan = F64.canonical_nan();
        assert_eq!(to_int(F64, big, 32, true, Rm::Rne), (i32::MAX as u64, NV));
        assert_eq!(
            to_int(F64, (-1e30f64).to_bits(), 64, true, Rm::Rne),
            (i64::MIN as u64, NV)
        );
        assert_eq!(to_int(F64, nan, 32, false, Rm::Rne), (u64::MAX, NV));
        assert_eq!(to_int(F64, (-1f64).to_bits(), 64, false, Rm::Rne), (0, NV));
        // -0.5 rounds to -0, which is in range for an unsigned target.
        assert_eq!(
            to_int(F64, (-0.5f64).to_bits(), 32, false, Rm::Rtz),
            (0, NX)
        );
        assert_eq!(to_int(F64, 2.5f64.to_bits(), 64, true, Rm::Rne), (2, NX));
        assert_eq!(to_int(F64, 2.5f64.to_bits(), 64, true, Rm::Rmm), (3, NX));
        assert_eq!(
            to_int(F64, (-2.5f64).to_bits(), 64, true, Rm::Rdn),
            (-3i64 as u64, NX)
        );
    }

    #[test]
    fn a_conversion_from_an_integer_rounds() {
        assert_eq!(
            from_int(F32, u64::MAX, false, Rm::Rne),
            (u64::from(1.8446744e19f32.to_bits()), NX)
        );
        assert_eq!(
            from_int(F64, -7i64 as u64, true, Rm::Rne),
            ((-7f64).to_bits(), 0)
        );
        assert_eq!(
            from_int(F64, (1 << 53) + 1, false, Rm::Rup).0,
            ((1u64 << 53) as f64 + 2.0).to_bits()
        );
    }

    #[test]
    fn classify_names_each_class() {
        assert_eq!(classify(F64, (-f64::INFINITY).to_bits()), 1 << 0);
        assert_eq!(classify(F64, (-1f64).to_bits()), 1 << 1);
        assert_eq!(classify(F64, (1u64 << 63) | 1), 1 << 2);
        assert_eq!(classify(F64, (-0f64).to_bits()), 1 << 3);
        assert_eq!(classify(F64, 0), 1 << 4);
        assert_eq!(classify(F64, 1), 1 << 5);
        assert_eq!(classify(F64, 1f64.to_bits()), 1 << 6);
        assert_eq!(classify(F64, f64::INFINITY.to_bits()), 1 << 7);
        assert_eq!(classify(F64, 0x7ff0_0000_0000_0001), 1 << 8);
        assert_eq!(classify(F64, F64.canonical_nan()), 1 << 9);
    }
}
