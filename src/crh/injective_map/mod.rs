use crate::{CryptoError, Error};
use ark_ff::bytes::{ToBytes,FromBytes};
use ark_serialize::{CanonicalSerialize,SerializationError,CanonicalDeserialize};
use ark_std::rand::Rng;
use ark_std::{fmt::Debug, hash::Hash, marker::PhantomData};

use super::{pedersen, FixedLengthCRH};
use ark_ec::{
    models::{ModelParameters, TEModelParameters},
    twisted_edwards_extended::{GroupAffine as TEAffine, GroupProjective as TEProjective},
    ProjectiveCurve,
};

#[cfg(feature = "r1cs")]
pub mod constraints;

pub trait InjectiveMap<C: ProjectiveCurve> {
    type Output: ToBytes + Clone + Eq + Hash + Default + Debug + FromBytes + CanonicalSerialize;

    fn injective_map(ge: &C::Affine) -> Result<Self::Output, CryptoError>;
}

pub struct TECompressor;

impl<P: TEModelParameters> InjectiveMap<TEProjective<P>> for TECompressor {
    type Output = <P as ModelParameters>::BaseField;

    fn injective_map(ge: &TEAffine<P>) -> Result<Self::Output, CryptoError> {
        debug_assert!(ge.is_in_correct_subgroup_assuming_on_curve());
        Ok(ge.x)
    }
}

pub struct PedersenCRHCompressor<C: ProjectiveCurve, I: InjectiveMap<C>, W: pedersen::Window> {
    _group: PhantomData<C>,
    _compressor: PhantomData<I>,
    _crh: pedersen::CRH<C, W>,
}

impl<C: ProjectiveCurve, I: InjectiveMap<C>, W: pedersen::Window> FixedLengthCRH
    for PedersenCRHCompressor<C, I, W>
{
    const INPUT_SIZE_BITS: usize = pedersen::CRH::<C, W>::INPUT_SIZE_BITS;
    type Output = I::Output;
    type Parameters = pedersen::Parameters<C>;

    fn setup<R: Rng>(rng: &mut R) -> Result<Self::Parameters, Error> {
        let time = start_timer!(|| format!("PedersenCRHCompressor::Setup"));
        let params = pedersen::CRH::<C, W>::setup(rng);
        end_timer!(time);
        params
    }

    fn evaluate(parameters: &Self::Parameters, input: &[u8]) -> Result<Self::Output, Error> {
        let eval_time = start_timer!(|| "PedersenCRHCompressor::Eval");
        let result = I::injective_map(&pedersen::CRH::<C, W>::evaluate(parameters, input)?)?;
        end_timer!(eval_time);
        Ok(result)
    }
}
