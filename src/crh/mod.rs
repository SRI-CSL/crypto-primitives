use ark_ff::bytes::{ToBytes,FromBytes};
use ark_std::hash::Hash;
use ark_std::rand::Rng;
use ark_serialize::{CanonicalSerialize,SerializationError,CanonicalDeserialize};
pub mod bowe_hopwood;
pub mod injective_map;
pub mod pedersen;
pub mod poseidon;

use crate::Error;

#[cfg(feature = "r1cs")]
pub mod constraints;
#[cfg(feature = "r1cs")]
pub use constraints::*;

pub trait FixedLengthCRH {
    const INPUT_SIZE_BITS: usize;

    type Output: ToBytes + Clone + Eq + core::fmt::Debug + Hash + Default + FromBytes;
    type Parameters: Clone + Default;

    fn setup<R: Rng>(r: &mut R) -> Result<Self::Parameters, Error>;
    fn evaluate(parameters: &Self::Parameters, input: &[u8]) -> Result<Self::Output, Error>;
}
