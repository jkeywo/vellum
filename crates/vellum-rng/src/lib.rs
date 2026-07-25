//! PCG32 — the fleet's save-format RNG, with one house style.
//!
//! Both games hand-rolled the same generator rather than depend on the `rand`
//! ecosystem, for the same reason: in a game whose save file is its command
//! log, the byte sequence is part of the save format. A dependency bump that
//! improved a distribution would silently invalidate every recorded run, and
//! it would do it quietly.
//!
//! # One entry point
//!
//! This crate once carried four entry points — two seeding policies and two
//! bounded draws, the fossils of two games that wrote the same generator
//! independently — because merging them would have invalidated every saved
//! run in both. The fleet then decided to pay exactly that cost once, on
//! purpose (decision `rng-unification-breaks-saves` in vellum's spec), and
//! converge:
//!
//! - **Construction**: [`Pcg32::seeded`] — the canonical PCG warm-up over a
//!   SplitMix64-mixed seed. The mix is what lets a low-entropy seed a player
//!   typed in (`42`, `7`, `0`) start from well-mixed state; the canonical
//!   warm-up and shifted increment are what give independent, selectable
//!   streams.
//! - **Bounded draw**: [`Pcg32::below`] — Lemire's multiply-and-shift, the
//!   stronger of the two (no division on the accept path).
//! - **The type itself**: games store [`Pcg32`] in their saved state rather
//!   than private layouts around borrowed arithmetic; both serialize as
//!   `{ state, inc }`.
//! - **Derived draws**: the helpers both games duplicated —
//!   [`Pcg32::range_inclusive`], [`Pcg32::chance`], [`Pcg32::pick_index`],
//!   [`Pcg32::shuffle`] — live here, defined over the one `below`.
//!
//! The legacy entry points remain, deprecated, until both roguelikes finish
//! their migrations and re-bless their fixtures; then they go.
//!
//! There is deliberately still no `Rng` trait: one implementation does not
//! need an abstraction.

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
    /// The fleet construction: the canonical PCG warm-up over a
    /// SplitMix64-mixed seed, with a selectable stream.
    ///
    /// This is the one blessed entry point. The SplitMix pass spreads a
    /// low-entropy typed seed before it becomes state; the canonical warm-up
    /// keeps streams independent. It deliberately does *not* reproduce the
    /// published PCG vectors (the mix is in front) — [`Pcg32::canonical`]
    /// exists for that.
    pub fn seeded(seed: u64, stream: u64) -> Self {
        Self::canonical(split_mix_64(seed), stream)
    }

    /// The canonical PCG construction: zero the state, step, add the seed,
    /// step again, with the stream selector shifted into the increment.
    ///
    /// This is what the reference implementation does, so a generator built
    /// this way reproduces the published test vectors — see
    /// `canonical_matches_the_published_reference_vector`. Kept public as the
    /// reference-vector primitive; game code uses [`Pcg32::seeded`].
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
    #[deprecated(note = "the fleet converged on Pcg32::seeded; this stays only until \
                rogue-hunter's migration re-blesses its fixtures")]
    pub fn splitmix_fixed_inc(seed: u64) -> Self {
        Self {
            state: split_mix_64(seed),
            inc: FIXED_INCREMENT,
        }
    }

    /// Rebuild a generator from raw state, for tools and tests that store
    /// parts.
    ///
    /// This was the seam the games borrowed arithmetic through while their
    /// save formats predated this crate. The fleet's RNG unification retires
    /// that pattern: games store [`Pcg32`] directly and construct with
    /// [`Pcg32::seeded`].
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

    /// Uniform in `0..bound` — the fleet's one bounded draw, by Lemire's
    /// multiply-and-shift.
    ///
    /// Draws a word, multiplies by the bound, and returns the high half,
    /// rejecting the short interval that would bias the result.
    pub fn below(&mut self, bound: u32) -> u32 {
        debug_assert!(bound > 0, "below requires a non-zero bound");
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let value = self.next_u32();
            let product = u64::from(value) * u64::from(bound);
            if (product as u32) >= threshold {
                return (product >> 32) as u32;
            }
        }
    }

    /// Uniform in `0..bound`, by Lemire's multiply-and-shift.
    #[deprecated(note = "renamed to Pcg32::below — the fleet's one bounded draw")]
    pub fn below_lemire(&mut self, bound: u32) -> u32 {
        self.below(bound)
    }

    /// Uniform in `0..bound`, by rejection then remainder.
    ///
    /// Rejects the short leading interval that would make low values slightly
    /// more likely, then takes the remainder. Not interchangeable with
    /// [`Self::below`]: for the same generator state the two return
    /// different values.
    #[deprecated(
        note = "the fleet converged on Pcg32::below (Lemire); this stays only \
                until rogue-hunter's migration re-blesses its fixtures"
    )]
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

    /// Uniform in the inclusive range `lo..=hi`.
    pub fn range_inclusive(&mut self, lo: u32, hi: u32) -> u32 {
        debug_assert!(lo <= hi);
        lo + self.below(hi - lo + 1)
    }

    /// True with probability `numerator / denominator`.
    pub fn chance(&mut self, numerator: u32, denominator: u32) -> bool {
        self.below(denominator) < numerator
    }

    /// An index into a collection of `len` elements.
    pub fn pick_index(&mut self, len: usize) -> usize {
        debug_assert!(len > 0);
        self.below(len as u32) as usize
    }

    /// Fisher-Yates shuffle with deterministic order.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u32 + 1) as usize;
            items.swap(i, j);
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
    #[allow(deprecated)]
    fn splitmix_fixed_inc_matches_the_pinned_sequence() {
        let mut rng = Pcg32::splitmix_fixed_inc(0);
        let first: Vec<u32> = (0..4).map(|_| rng.next_u32()).collect();
        assert_eq!(first, [1092706980, 278790474, 1039822109, 1377468856]);
    }

    /// The two constructions must not accidentally converge. If they ever
    /// produced the same stream, one game's saves would be readable as the
    /// other's and the distinction this crate is built around would be a lie.
    #[test]
    #[allow(deprecated)]
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
    #[allow(deprecated)]
    fn the_two_bounded_draws_disagree() {
        let mut differed = 0;
        for bound in [3u32, 6, 100, 1000] {
            let mut lemire = Pcg32::splitmix_fixed_inc(0);
            let mut modulo = Pcg32::splitmix_fixed_inc(0);
            for _ in 0..256 {
                let a = lemire.below(bound);
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
    #[allow(deprecated)]
    fn bounded_draws_stay_in_range() {
        let mut rng = Pcg32::splitmix_fixed_inc(7);
        for _ in 0..2000 {
            assert!(rng.below(5) < 5);
            assert!(rng.below_modulo(5) < 5);
        }
    }

    /// A bound just above half of `u32::MAX` rejects nearly half of all draws,
    /// so it is where a rejection loop written the wrong way round would spin
    /// or bias. Both must still terminate and stay in range.
    #[test]
    #[allow(deprecated)]
    fn the_worst_case_bound_terminates() {
        let mut rng = Pcg32::splitmix_fixed_inc(1);
        let bound = 0x8000_0001u32;
        for _ in 0..1000 {
            assert!(rng.below(bound) < bound);
            assert!(rng.below_modulo(bound) < bound);
        }
    }

    #[test]
    fn split_mix_64_is_pinned() {
        assert_eq!(split_mix_64(0), 16294208416658607535);
        assert_ne!(split_mix_64(0), split_mix_64(1));
    }

    /// The fleet construction is pinned: these constants are what every
    /// migrated save format is recorded against. Moving them is moving the
    /// fleet's save format, and there is no quiet way to do that.
    #[test]
    fn seeded_is_pinned() {
        let mut rng = Pcg32::seeded(0, 0);
        let first: Vec<u32> = (0..4).map(|_| rng.next_u32()).collect();
        assert_eq!(first, [3234325189, 1963755818, 1465678534, 3792411884]);

        let mut typed = Pcg32::seeded(42, 1);
        let typed_first: Vec<u32> = (0..4).map(|_| typed.next_u32()).collect();
        assert_eq!(
            typed_first,
            [4176028549, 3950285441, 2197104919, 1103863609]
        );
    }

    #[test]
    fn seeded_streams_are_independent() {
        let mut one = Pcg32::seeded(42, 1);
        let mut two = Pcg32::seeded(42, 2);
        let a: Vec<u32> = (0..8).map(|_| one.next_u32()).collect();
        let b: Vec<u32> = (0..8).map(|_| two.next_u32()).collect();
        assert_ne!(a, b);
    }

    #[test]
    fn derived_draws_stay_in_range_and_are_deterministic() {
        let mut a = Pcg32::seeded(9, 1);
        let mut b = Pcg32::seeded(9, 1);
        for _ in 0..500 {
            let v = a.range_inclusive(2, 4);
            assert!((2..=4).contains(&v));
            assert_eq!(v, b.range_inclusive(2, 4));
        }
        let mut items_a: Vec<u32> = (0..20).collect();
        let mut items_b: Vec<u32> = (0..20).collect();
        a.shuffle(&mut items_a);
        b.shuffle(&mut items_b);
        assert_eq!(items_a, items_b);
        assert!(a.pick_index(7) < 7);
        let hits = (0..1000).filter(|_| a.chance(1, 4)).count();
        assert!((150..350).contains(&hits), "chance(1,4) hit {hits}/1000");
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
    #[allow(deprecated)]
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
