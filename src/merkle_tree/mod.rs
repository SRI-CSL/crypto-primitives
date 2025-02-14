use crate::{crh::FixedLengthCRH, Vec};
use ark_ff::bytes::{ToBytes,FromBytes};
use ark_std::{fmt,io::{Read,Result as IoResult,Write}};
use ark_serialize::*;
use ark_serialize_derive::*;
#[cfg(feature = "r1cs")]
pub mod constraints;
pub trait Config {
    const HEIGHT: usize;
    type H: FixedLengthCRH;
}


/// Stores the hashes of a particular path (in order) from leaf to root.
/// Our path `is_left_child()` if the boolean in `path` is true.


pub struct Path<P: Config> {
    pub(crate) path: Vec<(Digest<P>, Digest<P>)>,
}
impl<C:Config> Path<C>{
    pub fn get_length(&self) -> usize{
	self.path.len()
    }
    pub fn get_path(&self) -> Vec<(Digest<C>,Digest<C>)>{
	let path = self.clone();
	path.path
    }
    pub fn set_path(path : Vec<(Digest<C>,Digest<C>)>) -> Self{
	Path{
	    path: path
	}
    }
}
impl<C:Config> PartialEq for Path<C>{
    fn eq(&self, other: &Self) -> bool {
	self.path == other.path
    }
}
impl<C:Config> Eq for Path<C> {}
impl<C:Config> ToBytes for Path<C>{
    fn  write<W:Write>(&self,mut writer:W) -> ark_std::io::Result<()>{
	for p in &self.path{
	    p.0.write(&mut writer)?;
	    p.1.write(&mut writer)?;
	}
	Ok(())
    }
}


impl<C:Config> Clone for Path<C>{
    //clone will clone each digest
    fn clone(&self) -> Self{
	let mut output = Vec::new();
	for s in &self.path{
	    output.push((s.0.clone(),s.1.clone()));
	}
	Path{
	    path: output
	}
    }
}

impl<C:Config> FromBytes for Path<C>{
    fn read<R:Read>(mut reader:R) -> IoResult<Self>{
	let mut pre_output = Digest::<C>::read(&mut reader)?;
	
	let mut output_vec:Vec<Digest<C>> = Vec::new();
	let mut output_1 = Some(pre_output);
	while let Some(o) = output_1{
	    output_vec.push(o);
	    output_1 = Digest::<C>::read(&mut reader).ok();
	}
	
	//now split into pairs
	let mut pairs = Vec::new();
	let mut i = 0;
	assert_eq!(pairs.len() % 2, 0);
	let mut length;
	if(output_vec.len() == 0){
	    length = 0;
	}
	else{
	    length = output_vec.len() - 1;
	}
	while i < length{
	    pairs.push((output_vec[i].clone(),output_vec[i+1].clone()));
	    i = i + 2;
	}
	Ok(Path{
	    path:pairs
	})
    }
}

pub type Parameters<P> = <<P as Config>::H as FixedLengthCRH>::Parameters;
pub type Digest<P> = <<P as Config>::H as FixedLengthCRH>::Output;

impl<P: Config> Default for Path<P> {
    fn default() -> Self {
        let mut path = Vec::with_capacity(P::HEIGHT as usize);
        for _i in 1..P::HEIGHT as usize {
            path.push((
                <P::H as FixedLengthCRH>::Output::default(),
                <P::H as FixedLengthCRH>::Output::default(),
            ));
        }
        Self { path }
    }
}

impl<P: Config> Path<P> {
    pub fn verify<L: ToBytes>(
        &self,
        parameters: &<P::H as FixedLengthCRH>::Parameters,
        root_hash: &<P::H as FixedLengthCRH>::Output,
        leaf: &L,
    ) -> Result<bool, crate::Error> {
        if self.path.len() != (P::HEIGHT - 1) as usize {
            return Ok(false);
        }
        // Check that the given leaf matches the leaf in the membership proof.
        let mut buffer = [0u8; 128];

        if !self.path.is_empty() {
            let claimed_leaf_hash = hash_leaf::<P::H, L>(parameters, leaf, &mut buffer)?;

            // Check if leaf is one of the bottom-most siblings.
            if claimed_leaf_hash != self.path[0].0 && claimed_leaf_hash != self.path[0].1 {
                return Ok(false);
            };

            let mut prev = claimed_leaf_hash;
            // Check levels between leaf level and root.
            for &(ref hash, ref sibling_hash) in &self.path {
                // Check if the previous hash matches the correct current hash.
                if &prev != hash && &prev != sibling_hash {
                    return Ok(false);
                };
                prev = hash_inner_node::<P::H>(parameters, hash, sibling_hash, &mut buffer)?;
            }

            if root_hash != &prev {
                return Ok(false);
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

pub struct MerkleTree<P: Config> {
    tree: Vec<Digest<P>>,
    padding_tree: Vec<(Digest<P>, Digest<P>)>,
    parameters: <P::H as FixedLengthCRH>::Parameters,
    root: Option<Digest<P>>,
}

impl<P: Config> MerkleTree<P> {
    pub const HEIGHT: u8 = P::HEIGHT as u8;

    pub fn blank(parameters: Parameters<P>) -> Self {
        MerkleTree {
            tree: Vec::new(),
            padding_tree: Vec::new(),
            root: None,
            parameters,
        }
    }

    pub fn new<L: ToBytes>(parameters: Parameters<P>, leaves: &[L]) -> Result<Self, crate::Error> {
        let new_time = start_timer!(|| "MerkleTree::New");

        let last_level_size = leaves.len().next_power_of_two();
        let tree_size = 2 * last_level_size - 1;
        let tree_height = tree_height(tree_size);
        assert!(tree_height as u8 <= Self::HEIGHT);

        // Initialize the merkle tree.
        let mut tree = Vec::with_capacity(tree_size);
        let empty_hash = hash_empty::<P::H>(&parameters)?;
        for _ in 0..tree_size {
            tree.push(empty_hash.clone());
        }

        // Compute the starting indices for each level of the tree.
        let mut index = 0;
        let mut level_indices = Vec::with_capacity(tree_height);
        for _ in 0..tree_height {
            level_indices.push(index);
            index = left_child(index);
        }

        // Compute and store the hash values for each leaf.
        let last_level_index = level_indices.pop().unwrap_or(0);
        let mut buffer = [0u8; 128];
        for (i, leaf) in leaves.iter().enumerate() {
            tree[last_level_index + i] = hash_leaf::<P::H, _>(&parameters, leaf, &mut buffer)?;
        }

        // Compute the hash values for every node in the tree.
        let mut upper_bound = last_level_index;
        let mut buffer = [0u8; 128];
        level_indices.reverse();
        for &start_index in &level_indices {
            // Iterate over the current level.
            for current_index in start_index..upper_bound {
                let left_index = left_child(current_index);
                let right_index = right_child(current_index);

                // Compute Hash(left || right).
                tree[current_index] = hash_inner_node::<P::H>(
                    &parameters,
                    &tree[left_index],
                    &tree[right_index],
                    &mut buffer,
                )?;
            }
            upper_bound = start_index;
        }
        // Finished computing actual tree.
        // Now, we compute the dummy nodes until we hit our HEIGHT goal.
        let mut cur_height = tree_height;
        let mut padding_tree = Vec::new();
        let mut cur_hash = tree[0].clone();
        let root_hash = if cur_height < Self::HEIGHT as usize {
            while cur_height < (Self::HEIGHT - 1) as usize {
                cur_hash =
                    hash_inner_node::<P::H>(&parameters, &cur_hash, &empty_hash, &mut buffer)?;
                padding_tree.push((cur_hash.clone(), empty_hash.clone()));
                cur_height += 1;
            }
            hash_inner_node::<P::H>(&parameters, &cur_hash, &empty_hash, &mut buffer)?
        } else {
            cur_hash
        };
        end_timer!(new_time);

        Ok(MerkleTree {
            tree,
            padding_tree,
            parameters,
            root: Some(root_hash),
        })
    }

    #[inline]
    pub fn root(&self) -> Digest<P> {
        self.root.clone().unwrap()
    }

    pub fn generate_proof<L: ToBytes>(
        &self,
        index: usize,
        leaf: &L,
    ) -> Result<Path<P>, crate::Error> {
        let prove_time = start_timer!(|| "MerkleTree::GenProof");
        let mut path = Vec::new();

        let mut buffer = [0u8; 128];
        let leaf_hash = hash_leaf::<P::H, _>(&self.parameters, leaf, &mut buffer)?;
        let tree_height = tree_height(self.tree.len());
        let tree_index = convert_index_to_last_level(index, tree_height);
        let empty_hash = hash_empty::<P::H>(&self.parameters)?;

        // Check that the given index corresponds to the correct leaf.
        if leaf_hash != self.tree[tree_index] {
            return Err(Error::IncorrectLeafIndex(tree_index).into());
        }

        // Iterate from the leaf up to the root, storing all intermediate hash values.
        let mut current_node = tree_index;
        while !is_root(current_node) {
            let sibling_node = sibling(current_node).unwrap();
            let (curr_hash, sibling_hash) = (
                self.tree[current_node].clone(),
                self.tree[sibling_node].clone(),
            );
            if is_left_child(current_node) {
                path.push((curr_hash, sibling_hash));
            } else {
                path.push((sibling_hash, curr_hash));
            }
            current_node = parent(current_node).unwrap();
        }

        // Store the root node. Set boolean as true for consistency with digest
        // location.
        assert!(path.len() < Self::HEIGHT as usize);
        if path.len() != (Self::HEIGHT - 1) as usize {
            path.push((self.tree[0].clone(), empty_hash));
            for &(ref hash, ref sibling_hash) in &self.padding_tree {
                path.push((hash.clone(), sibling_hash.clone()));
            }
        }
        end_timer!(prove_time);
        if path.len() != (Self::HEIGHT - 1) as usize {
            Err(Error::IncorrectPathLength(path.len()).into())
        } else {
            Ok(Path { path })
        }
    }
}

#[derive(Debug)]
pub enum Error {
    IncorrectLeafIndex(usize),
    IncorrectPathLength(usize),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let msg = match self {
            Error::IncorrectLeafIndex(index) => format!("incorrect leaf index: {}", index),
            Error::IncorrectPathLength(len) => format!("incorrect path length: {}", len),
        };
        write!(f, "{}", msg)
    }
}

impl ark_std::error::Error for Error {}

/// Returns the height of the tree, given the size of the tree.
#[inline]
fn tree_height(tree_size: usize) -> usize {
    if tree_size == 1 {
        return 1;
    }

    ark_std::log2(tree_size) as usize
}

/// Returns true iff the index represents the root.
#[inline]
fn is_root(index: usize) -> bool {
    index == 0
}

/// Returns the index of the left child, given an index.
#[inline]
fn left_child(index: usize) -> usize {
    2 * index + 1
}

/// Returns the index of the right child, given an index.
#[inline]
fn right_child(index: usize) -> usize {
    2 * index + 2
}

/// Returns the index of the sibling, given an index.
#[inline]
fn sibling(index: usize) -> Option<usize> {
    if index == 0 {
        None
    } else if is_left_child(index) {
        Some(index + 1)
    } else {
        Some(index - 1)
    }
}

/// Returns true iff the given index represents a left child.
#[inline]
fn is_left_child(index: usize) -> bool {
    index % 2 == 1
}

/// Returns the index of the parent, given an index.
#[inline]
fn parent(index: usize) -> Option<usize> {
    if index > 0 {
        Some((index - 1) >> 1)
    } else {
        None
    }
}

#[inline]
fn convert_index_to_last_level(index: usize, tree_height: usize) -> usize {
    index + (1 << (tree_height - 1)) - 1
}

/// Returns the output hash, given a left and right hash value.
pub(crate) fn hash_inner_node<H: FixedLengthCRH>(
    parameters: &H::Parameters,
    left: &H::Output,
    right: &H::Output,
    buffer: &mut [u8],
) -> Result<H::Output, crate::Error> {
    let bytes = ark_ff::to_bytes![left]?
        .into_iter()
        .chain(ark_ff::to_bytes![right]?);
    buffer.iter_mut().zip(bytes).for_each(|(b, l_b)| *b = l_b);
    H::evaluate(parameters, &buffer[..(H::INPUT_SIZE_BITS / 8)])
}

/// Returns the hash of a leaf.
pub(crate) fn hash_leaf<H: FixedLengthCRH, L: ToBytes>(
    parameters: &H::Parameters,
    leaf: &L,
    buffer: &mut [u8],
) -> Result<H::Output, crate::Error> {
    buffer
        .iter_mut()
        .zip(&ark_ff::to_bytes![leaf]?)
        .for_each(|(b, l_b)| *b = *l_b);
    H::evaluate(parameters, &buffer[..(H::INPUT_SIZE_BITS / 8)])
}

pub(crate) fn hash_empty<H: FixedLengthCRH>(
    parameters: &H::Parameters,
) -> Result<H::Output, crate::Error> {
    let empty_buffer = vec![0u8; H::INPUT_SIZE_BITS / 8];
    H::evaluate(parameters, &empty_buffer)
}

#[cfg(test)]
mod test {
    use crate::{
        crh::{pedersen, *},
        merkle_tree::*,
    };
    use ark_ed_on_bls12_381::EdwardsProjective as JubJub;
    use ark_ff::Zero;

    #[derive(Clone)]
    pub(super) struct Window4x256;
    impl pedersen::Window for Window4x256 {
        const WINDOW_SIZE: usize = 4;
        const NUM_WINDOWS: usize = 256;
    }

    type H = pedersen::CRH<JubJub, Window4x256>;

    struct JubJubMerkleTreeParams;

    impl Config for JubJubMerkleTreeParams {
        const HEIGHT: usize = 8;
        type H = H;
    }
    type JubJubMerkleTree = MerkleTree<JubJubMerkleTreeParams>;

    fn generate_merkle_tree<L: ToBytes + Clone + Eq>(leaves: &[L]) -> () {
        let mut rng = ark_std::test_rng();

        let crh_parameters = H::setup(&mut rng).unwrap();
        let tree = JubJubMerkleTree::new(crh_parameters.clone(), &leaves).unwrap();
        let root = tree.root();
        for (i, leaf) in leaves.iter().enumerate() {
            let proof = tree.generate_proof(i, &leaf).unwrap();
            assert!(proof.verify(&crh_parameters, &root, &leaf).unwrap());
        }
    }

    #[test]
    fn good_root_test() {
        let mut leaves = Vec::new();
        for i in 0..4u8 {
            leaves.push([i, i, i, i, i, i, i, i]);
        }
        generate_merkle_tree(&leaves);
        let mut leaves = Vec::new();
        for i in 0..100u8 {
            leaves.push([i, i, i, i, i, i, i, i]);
        }
        generate_merkle_tree(&leaves);
    }

    #[test]
    fn no_dummy_nodes_test() {
        let mut leaves = Vec::new();
        for i in 0..(1u8 << JubJubMerkleTree::HEIGHT - 1) {
            leaves.push([i, i, i, i, i, i, i, i]);
        }
        generate_merkle_tree(&leaves);
    }

    #[test]
    fn single_leaf_test() {
        generate_merkle_tree(&[[1u8; 8]]);
    }

    fn bad_merkle_tree_verify<L: ToBytes + Clone + Eq>(leaves: &[L]) -> () {
        let mut rng = ark_std::test_rng();

        let crh_parameters = H::setup(&mut rng).unwrap();
        let tree = JubJubMerkleTree::new(crh_parameters.clone(), &leaves).unwrap();
        let root = JubJub::zero().into();
        for (i, leaf) in leaves.iter().enumerate() {
            let proof = tree.generate_proof(i, &leaf).unwrap();
            assert!(proof.verify(&crh_parameters, &root, &leaf).unwrap());
        }
    }

    #[should_panic]
    #[test]
    fn bad_root_test() {
        let mut leaves = Vec::new();
        for i in 0..4u8 {
            leaves.push([i, i, i, i, i, i, i, i]);
        }
        generate_merkle_tree(&leaves);
        let mut leaves = Vec::new();
        for i in 0..100u8 {
            leaves.push([i, i, i, i, i, i, i, i]);
        }
        bad_merkle_tree_verify(&leaves);
    }
}
