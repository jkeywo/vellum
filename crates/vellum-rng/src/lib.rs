//! PCG32, with room for more than one house style.
//!
//! Both games hand-rolled the same generator rather than depend on the `rand`
//! ecosystem, for the same reason: in a game whose save file is its command
//! log, the byte sequence is part of the save format. A dependency bump that
//! improved a distribution would silently invalidate every recorded run, and
//! it would do it quietly.
//!
//! # Why this crate has four entry points and not one
//!
//! Read the two implementations side by side and they look like the same code.
//! They are not, and the differences are exactly the kind that survive a
//! careless merge:
//!
//! | | canonical | splitmix-fixed |
//! |---|---|---|
//! | seeding | PCG's two-step warm-up | SplitMix64, no warm-up |
//! | increment | `(stream << 1) \| 1` | a fixed constant |
//! | streams | several, selectable | one |
//!
//! And the bounded draw, which is the trap. Both compute
//! `let threshold = bound.wrapping_neg() % bound;` — which is why they read
//! alike — and then diverge completely: one multiplies and returns the high
//! word ([`Pcg32::below_lemire`]), the other takes a remainder
//! ([`Pcg32::below_modulo`]). Both are unbiased. Neither substitutes for the
//! other, and swapping them changes every value a game draws.
//!
//! So both policies are here, named for what they do, with no default. There
//! is deliberately no `Rng` trait: two implementations and no third caller
//! does not need an abstraction, and a trait object would let a game pick up
//! the wrong policy by accident, which is the one failure this crate exists to
//! prevent.

use serde::{Deserialize, Serialize};

const MULTIPLIER: u64 = 6364136223846793005;

/// The increment used by the SplitMix-seeded, single-stream construction.
/// Knuth's LCG addend, and odd, as PCG requires.
const FIXED_INCREMENT: u64 = 1442695040888963407;

/// PCG-XSH-RR 64/32: 64 bits of state, an odd increment selecting the stream.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    /// The canonical PCG construction: zero the state, step, add the seed,
    /// step again, with the stream selector shifted into the increment.
    ///
    /// This is what the reference implementation does, so a generator built
    /// this way reproduces the published test vectors — see
    /// `canonical_matches_the_published_reference_vector`.
    pub fn canonical(seed: u64, stream: u64) -> Self {
        let inc = (stream << 1) | 1;
        let mut rng = Self { state: 0, inc };
        rng.step();
        rng.state = rng.state.wrapping_add(seed);
        rng.step();
        rng
    }

    /// A single stream, seeded through SplitMix64 and using a fixed
    /// increment.
    ///
    /// The SplitMix pass is what lets a low-entropy seed a player typed in —
    /// `42`, `7`, `0` — start from well-mixed state instead of walking
    /// through a predictable neighbourhood of the sequence.
    pub fn splitmix_fixed_inc(seed: u64) -> Self {
        Self {
            state: split_mix_64(seed),
            inc: FIXED_INCREMENT,
        }
    }

    /// Rebuild a generator from raw state, for a consumer that stores its own.
    ///
    /// This exists because both games serialise their generator *inside* saved
    /// state, in shapes that predate this crate and differ from each other and
    /// from [`Pcg32`]: one keeps a single field, the other two. Adopting this
    /// struct wholesale would rewrite both save formats, so the games keep
    /// their own types and borrow only the arithmetic — construct, draw, hand
    /// the state back.
    ///
    /// New code should prefer [`Pcg32::canonical`] or
    /// [`Pcg32::splitmix_fixed_inc`] and store this type directly.
    pub const fn from_parts(state: u64, inc: u64) -> Self {
        Self { state, inc }
    }

    /// The raw state, to be stored by a consumer that owns its own layout.
    pub const fn into_parts(self) -> (u64, u64) {
        (self.state, self.inc)
    }

    /// The increment used by [`Pcg32::splitmix_fixed_inc`], for a consumer
    /// storing only the state half.
    pub const FIXED_INCREMENT: u64 = FIXED_INCREMENT;

    fn step(&mut self) {
        self.state = self.state.wrapping_mul(MULTIPLIER).wrapping_add(self.inc);
    }

    /// The next 32 uniformly distributed bits.
    ///
    /// The one piece both games genuinely shared: the XSH-RR output
    /// permutation, applied to the state *before* advancing it.
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.step();
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in `0..bound`, by Lemire's multiply-and-shift.
    ///
    /// Draws a word, multiplies by the bound, and returns the high half,
    /// rejecting the short interval that would bias the result. Not
    /// interchangeable with [`Self::below_modulo`]: for the same generator
    /// state the two return different values.
    pub fn below_lemire(&mut self, bound: u32) -> u32 {
        debug_assert!(bound > 0, "below_lemire requires a non-zero bound");
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let value = self.next_u32();
            let product = u64::from(value) * u64::from(bound);
            if (product as u32) >= threshold {
                return (product >> 32) as u32;
            }
        }
    }

    /// Uniform in `0..bound`, by rejection then remainder.
    ///
    /// Rejects the short leading interval that would make low values slightly
    /// more likely, then takes the remainder. Not interchangeable with
    /// [`Self::below_lemire`].
    pub fn below_modulo(&mut self, bound: u32) -> u32 {
        debug_assert!(bound > 0, "below_modulo requires a non-zero bound");
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let value = self.next_u32();
            if value >= threshold {
                return value % bound;
            }
        }
    }
}

/// SplitMix64: a finalising mix, used to spread a low-entropy seed before it
/// becomes generator state, and to derive one seed from another.
pub fn split_mix_64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published pcg32-demo output for seed 42, stream 54. This is not one
    /// game's expectation of itself — it is the reference implementation's, so
    /// passing it means this really is PCG32 and not merely self-consistent.
    #[test]
    fn canonical_matches_the_published_reference_vector() {
        let mut rng = Pcg32::canonical(42, 54);
        let expected = [
            0xa15c02b7u32,
            0x7b47f409,
            0xba1d3330,
            0x83d2f293,
            0xbfa4784b,
            0xcbed606e,
        ];
        for value in expected {
            assert_eq!(rng.next_u32(), value);
        }
    }

    /// The sequence rogue-hunter's saved runs were recorded against. Pinned
    /// here as well as in that game, so an engine change fails in both places.
    #[test]
    fn splitmix_fixed_inc_matches_the_pinned_sequence() {
        let mut rng = Pcg32::splitmix_fixed_inc(0);
        let first: Vec<u32> = (0..4).map(|_| rng.next_u32()).collect();
        assert_eq!(first, [1092706980, 278790474, 1039822109, 1377468856]);
    }

    /// The two constructions must not accidentally converge. If they ever
    /// produced the same stream, one game's saves would be readable as the
    /// other's and the distinction this crate is built around would be a lie.
    #[test]
    fn the_two_constructions_are_different_generators() {
        let mut canonical = Pcg32::canonical(0, 1);
        let mut splitmix = Pcg32::splitmix_fixed_inc(0);
        let a: Vec<u32> = (0..16).map(|_| canonical.next_u32()).collect();
        let b: Vec<u32> = (0..16).map(|_| splitmix.next_u32()).collect();
        assert_ne!(a, b);
    }

    /// The heart of it. Both draws are correct; they are not the same
    /// function. This test exists so that nobody "simplifies" the pair into
    /// one and silently rewrites every saved run in both games.
    #[test]
    fn the_two_bounded_draws_disagree() {
        let mut differed = 0;
        for bound in [3u32, 6, 100, 1000] {
            let mut lemire = Pcg32::splitmix_fixed_inc(0);
            let mut modulo = Pcg32::splitmix_fixed_inc(0);
            for _ in 0..256 {
                let a = lemire.below_lemire(bound);
                let b = modulo.below_modulo(bound);
                assert!(a < bound && b < bound, "a draw escaped its bound");
                if a != b {
                    differed += 1;
                }
            }
        }
        assert!(
            differed > 0,
            "the two bounded draws produced identical output; one of them has \
             been rewritten into the other"
        );
    }

    #[test]
    fn streams_are_independent() {
        let mut one = Pcg32::canonical(42, 1);
        let mut two = Pcg32::canonical(42, 2);
        let a: Vec<u32> = (0..8).map(|_| one.next_u32()).collect();
        let b: Vec<u32> = (0..8).map(|_| two.next_u32()).collect();
        assert_ne!(a, b);
    }

    #[test]
    fn equal_seeds_produce_equal_sequences() {
        let mut a = Pcg32::canonical(12345, 1);
        let mut b = Pcg32::canonical(12345, 1);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn bounded_draws_stay_in_range() {
        let mut rng = Pcg32::splitmix_fixed_inc(7);
        for _ in 0..2000 {
            assert!(rng.below_lemire(5) < 5);
            assert!(rng.below_modulo(5) < 5);
        }
    }

    /// A bound just above half of `u32::MAX` rejects nearly half of all draws,
    /// so it is where a rejection loop written the wrong way round would spin
    /// or bias. Both must still terminate and stay in range.
    #[test]
    fn the_worst_case_bound_terminates() {
        let mut rng = Pcg32::splitmix_fixed_inc(1);
        let bound = 0x8000_0001u32;
        for _ in 0..1000 {
            assert!(rng.below_lemire(bound) < bound);
            assert!(rng.below_modulo(bound) < bound);
        }
    }

    #[test]
    fn split_mix_64_is_pinned() {
        assert_eq!(split_mix_64(0), 16294208416658607535);
        assert_ne!(split_mix_64(0), split_mix_64(1));
    }

    /// The interop seam has to be lossless, because a consumer that stores its
    /// own state round-trips through it on every single draw.
    #[test]
    fn parts_round_trip_exactly() {
        let mut original = Pcg32::canonical(99, 3);
        original.next_u32();
        let (state, inc) = original.clone().into_parts();
        let rebuilt = Pcg32::from_parts(state, inc);
        assert_eq!(rebuilt, original);

        let mut a = original;
        let mut b = rebuilt;
        for _ in 0..64 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    /// A consumer storing only the state half must be able to reconstruct the
    /// single-stream generator exactly.
    #[test]
    fn a_state_only_consumer_can_rebuild_the_fixed_stream() {
        let mut original = Pcg32::splitmix_fixed_inc(0);
        let mut stored = original.clone().into_parts().0;

        for _ in 0..64 {
            let mut borrowed = Pcg32::from_parts(stored, Pcg32::FIXED_INCREMENT);
            let drawn = borrowed.next_u32();
            stored = borrowed.into_parts().0;
            assert_eq!(drawn, original.next_u32());
        }
    }
}
