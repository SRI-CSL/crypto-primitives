use crate::{Error, Vec};
use ark_serialize::{CanonicalSerialize,SerializationError,CanonicalDeserialize};
use digest::Digest;
use ark_std::{rand::Rng,io::{Read,Write}};
use ark_std::{
    fmt::{Debug, Formatter, Result as FmtResult},
    marker::PhantomData,
};
#[cfg(feature = "parallel")]
use rayon::prelude::*;

use crate::crh::FixedLengthCRH;
use ark_ec::ProjectiveCurve;
use ark_ff::{Field, ToConstraintField};
use ark_std::cfg_chunks;
use crate::prf::blake2s::Blake2s;
use blake2::{Blake2s as B2s};
#[cfg(feature = "r1cs")]
pub mod constraints;

pub trait Window: Clone {
    const WINDOW_SIZE: usize;
    const NUM_WINDOWS: usize;
}

#[derive(Clone, Default)]
pub struct Parameters<C: ProjectiveCurve> {
    pub generators: Vec<Vec<C>>,
}

pub struct CRH<C: ProjectiveCurve, W: Window> {
    group: PhantomData<C>,
    window: PhantomData<W>,
}

impl<C: ProjectiveCurve, W: Window> CRH<C, W> {
    pub fn create_generators<R: Rng>(rng: &mut R) -> Vec<Vec<C>> {
        let mut generators_powers = Vec::new();
        for _ in 0..W::NUM_WINDOWS {
            generators_powers.push(Self::generator_powers(W::WINDOW_SIZE, rng));
        }
        generators_powers
    }

    pub fn generator_powers<R: Rng>(num_powers: usize, rng: &mut R) -> Vec<C> {
        let mut cur_gen_powers = Vec::with_capacity(num_powers);
        let mut base = C::rand(rng);
        for _ in 0..num_powers {
            cur_gen_powers.push(base);
            base.double_in_place();
        }
        cur_gen_powers
    }
}

impl<C: ProjectiveCurve, W: Window> FixedLengthCRH for CRH<C, W> {
    const INPUT_SIZE_BITS: usize = W::WINDOW_SIZE * W::NUM_WINDOWS;
    type Output = C::Affine;
    type Parameters = Parameters<C>;

    fn setup<R: Rng>(rng: &mut R) -> Result<Self::Parameters, Error> {
        let time = start_timer!(|| format!(
            "PedersenCRH::Setup: {} {}-bit windows; {{0,1}}^{{{}}} -> C",
            W::NUM_WINDOWS,
            W::WINDOW_SIZE,
            W::NUM_WINDOWS * W::WINDOW_SIZE
        ));
        let generators = Self::create_generators(rng);
        end_timer!(time);
        Ok(Self::Parameters { generators })
    }

    fn evaluate(parameters: &Self::Parameters, input: &[u8]) -> Result<Self::Output, Error> {
        let eval_time = start_timer!(|| "PedersenCRH::Eval");

        if (input.len() * 8) > W::WINDOW_SIZE * W::NUM_WINDOWS {
            panic!(
                "incorrect input length {:?} for window params {:?}✕{:?}",
                input.len(),
                W::WINDOW_SIZE,
                W::NUM_WINDOWS
            );
        }

        let mut padded_input = Vec::with_capacity(input.len());
        let mut input = input;
        // Pad the input if it is not the current length.
        if (input.len() * 8) < W::WINDOW_SIZE * W::NUM_WINDOWS {
            padded_input.extend_from_slice(input);
            let padded_length = (W::WINDOW_SIZE * W::NUM_WINDOWS) / 8;
            padded_input.resize(padded_length, 0u8);
            input = padded_input.as_slice();
        }

        assert_eq!(
            parameters.generators.len(),
            W::NUM_WINDOWS,
            "Incorrect pp of size {:?}✕{:?} for window params {:?}✕{:?}",
            parameters.generators[0].len(),
            parameters.generators.len(),
            W::WINDOW_SIZE,
            W::NUM_WINDOWS
        );

        // Compute sum of h_i^{m_i} for all i.
        let bits = bytes_to_bits(input);
        let result = cfg_chunks!(bits, W::WINDOW_SIZE)
            .zip(&parameters.generators)
            .map(|(bits, generator_powers)| {
                let mut encoded = C::zero();
                for (bit, base) in bits.iter().zip(generator_powers.iter()) {
                    if *bit {
                        encoded += base;
                    }
                }
                encoded
            })
            .sum::<C>();

        end_timer!(eval_time);

        Ok(result.into())
    }
}

#[derive(Clone, Default)]
pub struct Blake2Params{
    seed: [u8;32]
}

impl FixedLengthCRH for Blake2s {
    //stub
    const INPUT_SIZE_BITS: usize = 0;
    type Output = [u8;32];
    type Parameters = Blake2Params;

    fn setup<R: Rng>(rng: &mut R) -> Result<Self::Parameters, Error> {
	//use rng to make seed of [u8;32]
	let seed: [u8; 32] = rng.gen();
        Ok(Blake2Params { seed })
    }

    fn evaluate(parameters: &Self::Parameters, input: &[u8]) -> Result<Self::Output, Error> {
        let mut h = B2s::new();
        h.update(parameters.seed.as_ref());
        h.update(input.as_ref());
        let mut result = [0u8;32];
        result.copy_from_slice(&h.finalize());
	Ok(result)
        
    }
}

pub fn bytes_to_bits(bytes: &[u8]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for byte in bytes {
        for i in 0..8 {
            let bit = (*byte >> i) & 1;
            bits.push(bit == 1)
        }
    }
    bits
}

impl<C: ProjectiveCurve> Debug for Parameters<C> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        writeln!(f, "Pedersen Hash Parameters {{")?;
        for (i, g) in self.generators.iter().enumerate() {
            writeln!(f, "\t  Generator {}: {:?}", i, g)?;
        }
        writeln!(f, "}}")
    }
}

impl<ConstraintF: Field, C: ProjectiveCurve + ToConstraintField<ConstraintF>>
    ToConstraintField<ConstraintF> for Parameters<C>
{
    #[inline]
    fn to_field_elements(&self) -> Option<Vec<ConstraintF>> {
        Some(Vec::new())
    }
}
