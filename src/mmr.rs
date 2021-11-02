//! Merkle Mountain Range
//!
//! references:
//! https://github.com/mimblewimble/grin/blob/master/doc/mmr.md#structure
//! https://github.com/mimblewimble/grin/blob/0ff6763ee64e5a14e70ddd4642b99789a1648a32/core/src/core/pmmr.rs#L606

use crate::borrow::Cow;
use crate::collections::VecDeque;
use crate::helper::{get_peaks, parent_offset, pos_height_in_tree, sibling_offset};
use crate::leaf_index_to_pos;
use crate::mmr_store::{MMRBatch, MMRStore};
use crate::vec;
use crate::vec::Vec;
use crate::{Error, Merge, Result};
use core::fmt::Debug;
use core::marker::PhantomData;
use sp_core::H256;
use sp_runtime::traits::{BlakeTwo256, Hash};
use wasm_bindgen::prelude::*;

pub struct MMR<T, M, S: MMRStore<T>> {
    mmr_size: u64,
    batch: MMRBatch<T, S>,
    merge: PhantomData<M>,
}

impl<'a, T: Clone + PartialEq + Debug, M: Merge<Item = T>, S: MMRStore<T>> MMR<T, M, S> {
    pub fn new(mmr_size: u64, store: S) -> Self {
        MMR {
            mmr_size,
            batch: MMRBatch::new(store),
            merge: PhantomData,
        }
    }

    // find internal MMR elem, the pos must exists, otherwise a error will return
    fn find_elem<'b>(&self, pos: u64, hashes: &'b [T]) -> Result<Cow<'b, T>> {
        let pos_offset = pos.checked_sub(self.mmr_size);
        if let Some(elem) = pos_offset.and_then(|i| hashes.get(i as usize)) {
            return Ok(Cow::Borrowed(elem));
        }
        let elem = self.batch.get_elem(pos)?.ok_or(Error::InconsistentStore)?;
        Ok(Cow::Owned(elem))
    }

    pub fn mmr_size(&self) -> u64 {
        self.mmr_size
    }

    pub fn is_empty(&self) -> bool {
        self.mmr_size == 0
    }

    // push a element and return position
    pub fn push(&mut self, elem: T) -> Result<u64> {
        let mut elems: Vec<T> = Vec::new();
        // position of new elem
        let elem_pos = self.mmr_size;
        elems.push(elem);
        let mut height = 0u32;
        let mut pos = elem_pos;
        // continue to merge tree node if next pos heigher than current
        while pos_height_in_tree(pos + 1) > height {
            pos += 1;
            let left_pos = pos - parent_offset(height);
            let right_pos = left_pos + sibling_offset(height);
            let left_elem = self.find_elem(left_pos, &elems)?;
            let right_elem = self.find_elem(right_pos, &elems)?;
            let parent_elem = M::merge(&left_elem, &right_elem);
            elems.push(parent_elem);
            height += 1
        }
        // store hashes
        self.batch.append(elem_pos, elems);
        // update mmr_size
        self.mmr_size = pos + 1;
        Ok(elem_pos)
    }

    /// get_root
    pub fn get_root(&self) -> Result<T> {
        if self.mmr_size == 0 {
            return Err(Error::GetRootOnEmpty);
        } else if self.mmr_size == 1 {
            return self.batch.get_elem(0)?.ok_or(Error::InconsistentStore);
        }
        let peaks: Vec<T> = get_peaks(self.mmr_size)
            .into_iter()
            .map(|peak_pos| {
                self.batch
                    .get_elem(peak_pos)
                    .and_then(|elem| elem.ok_or(Error::InconsistentStore))
            })
            .collect::<Result<Vec<T>>>()?;
        self.bag_rhs_peaks(peaks)?.ok_or(Error::InconsistentStore)
    }

    fn bag_rhs_peaks(&self, mut rhs_peaks: Vec<T>) -> Result<Option<T>> {
        while rhs_peaks.len() > 1 {
            let right_peak = rhs_peaks.pop().expect("pop");
            let left_peak = rhs_peaks.pop().expect("pop");
            rhs_peaks.push(M::merge(&right_peak, &left_peak));
        }
        Ok(rhs_peaks.pop())
    }

    /// generate merkle proof for a peak
    /// the pos_list must be sorted, otherwise the behaviour is undefined
    ///
    /// 1. find a lower tree in peak that can generate a complete merkle proof for position
    /// 2. find that tree by compare positions
    /// 3. generate proof for each positions
    fn gen_proof_for_peak(
        &self,
        proof: &mut Vec<T>,
        pos_list: Vec<u64>,
        peak_pos: u64,
    ) -> Result<()> {
        // do nothing if position itself is the peak
        if pos_list.len() == 1 && pos_list == [peak_pos] {
            return Ok(());
        }
        // take peak root from store if no positions need to be proof
        if pos_list.is_empty() {
            proof.push(
                self.batch
                    .get_elem(peak_pos)?
                    .ok_or(Error::InconsistentStore)?,
            );
            return Ok(());
        }

        let mut queue: VecDeque<_> = pos_list.into_iter().map(|pos| (pos, 0u32)).collect();
        // Generate sub-tree merkle proof for positions
        while let Some((pos, height)) = queue.pop_front() {
            debug_assert!(pos <= peak_pos);
            if pos == peak_pos {
                break;
            }

            // calculate sibling
            let (sib_pos, parent_pos) = {
                let next_height = pos_height_in_tree(pos + 1);
                let sibling_offset = sibling_offset(height);
                if next_height > height {
                    // implies pos is right sibling
                    (pos - sibling_offset, pos + 1)
                } else {
                    // pos is left sibling
                    (pos + sibling_offset, pos + parent_offset(height))
                }
            };

            if Some(&sib_pos) == queue.front().map(|(pos, _)| pos) {
                // drop sibling
                queue.pop_front();
            } else {
                proof.push(
                    self.batch
                        .get_elem(sib_pos)?
                        .ok_or(Error::InconsistentStore)?,
                );
            }
            if parent_pos < peak_pos {
                // save pos to tree buf
                queue.push_back((parent_pos, height + 1));
            }
        }
        Ok(())
    }

    /// Generate merkle proof for positions
    /// 1. sort positions
    /// 2. push merkle proof to proof by peak from left to right
    /// 3. push bagged right hand side root
    pub fn gen_proof(&self, mut pos_list: Vec<u64>) -> Result<MerkleProof<T, M>> {
        if pos_list.is_empty() {
            return Err(Error::GenProofForInvalidLeaves);
        }
        if self.mmr_size == 1 && pos_list == [0] {
            return Ok(MerkleProof::new(self.mmr_size, Vec::new()));
        }
        // ensure positions is sorted
        pos_list.sort_unstable();
        let peaks = get_peaks(self.mmr_size);
        let mut proof: Vec<T> = Vec::new();
        // generate merkle proof for each peaks
        let mut bagging_track = 0;
        for peak_pos in peaks {
            let pos_list: Vec<_> = take_while_vec(&mut pos_list, |&pos| pos <= peak_pos);
            if pos_list.is_empty() {
                bagging_track += 1;
            } else {
                bagging_track = 0;
            }
            self.gen_proof_for_peak(&mut proof, pos_list, peak_pos)?;
        }

        // ensure no remain positions
        if !pos_list.is_empty() {
            return Err(Error::GenProofForInvalidLeaves);
        }

        if bagging_track > 1 {
            let rhs_peaks = proof.split_off(proof.len() - bagging_track);
            proof.push(self.bag_rhs_peaks(rhs_peaks)?.expect("bagging rhs peaks"));
        }

        Ok(MerkleProof::new(self.mmr_size, proof))
    }

    pub fn commit(self) -> Result<()> {
        self.batch.commit()
    }
}

#[derive(Debug)]
pub struct MerkleProof<T, M> {
    mmr_size: u64,
    proof: Vec<T>,
    merge: PhantomData<M>,
}

impl<T: PartialEq + Debug + Clone, M: Merge<Item = T>> MerkleProof<T, M> {
    pub fn new(mmr_size: u64, proof: Vec<T>) -> Self {
        MerkleProof {
            mmr_size,
            proof,
            merge: PhantomData,
        }
    }

    pub fn mmr_size(&self) -> u64 {
        self.mmr_size
    }

    pub fn proof_items(&self) -> &[T] {
        &self.proof
    }

    pub fn calculate_root(&self, leaves: Vec<(u64, T)>) -> Result<T> {
        calculate_root::<_, M, _>(leaves, self.mmr_size, self.proof.iter())
    }

    /// from merkle proof of leaf n to calculate merkle root of n + 1 leaves.
    /// by observe the MMR construction graph we know it is possible.
    /// https://github.com/jjyr/merkle-mountain-range#construct
    /// this is kinda tricky, but it works, and useful
    pub fn calculate_root_with_new_leaf(
        &self,
        mut leaves: Vec<(u64, T)>,
        new_pos: u64,
        new_elem: T,
        new_mmr_size: u64,
    ) -> Result<T> {
        let pos_height = pos_height_in_tree(new_pos);
        let next_height = pos_height_in_tree(new_pos + 1);
        if next_height > pos_height {
            let mut peaks_hashes =
                calculate_peaks_hashes::<_, M, _>(leaves, self.mmr_size, self.proof.iter())?;
            let peaks_pos = get_peaks(new_mmr_size);
            // reverse touched peaks
            let mut i = 0;
            while peaks_pos[i] < new_pos {
                i += 1
            }
            peaks_hashes[i..].reverse();
            calculate_root::<_, M, _>(vec![(new_pos, new_elem)], new_mmr_size, peaks_hashes.iter())
        } else {
            leaves.push((new_pos, new_elem));
            calculate_root::<_, M, _>(leaves, new_mmr_size, self.proof.iter())
        }
    }

    pub fn verify(&self, root: T, leaves: Vec<(u64, T)>) -> Result<bool> {
        self.calculate_root(leaves)
            .map(|calculated_root| calculated_root == root)
    }
}

fn calculate_peak_root<
    'a,
    T: 'a + PartialEq + Debug + Clone,
    M: Merge<Item = T>,
    I: Iterator<Item = &'a T>,
>(
    leaves: Vec<(u64, T)>,
    peak_pos: u64,
    proof_iter: &mut I,
) -> Result<T> {
    debug_assert!(!leaves.is_empty(), "can't be empty");
    // (position, hash, height)
    let mut queue: VecDeque<_> = leaves
        .into_iter()
        .map(|(pos, item)| (pos, item, 0u32))
        .collect();

    // calculate tree root from each items
    while let Some((pos, item, height)) = queue.pop_front() {
        if pos == peak_pos {
            // return root
            return Ok(item);
        }
        // calculate sibling
        let next_height = pos_height_in_tree(pos + 1);
        let (sib_pos, parent_pos) = {
            let sibling_offset = sibling_offset(height);
            if next_height > height {
                // implies pos is right sibling
                (pos - sibling_offset, pos + 1)
            } else {
                // pos is left sibling
                (pos + sibling_offset, pos + parent_offset(height))
            }
        };
        let sibling_item = if Some(&sib_pos) == queue.front().map(|(pos, _, _)| pos) {
            queue.pop_front().map(|(_, item, _)| item).unwrap()
        } else {
            proof_iter.next().ok_or(Error::CorruptedProof)?.clone()
        };

        let parent_item = if next_height > height {
            M::merge(&sibling_item, &item)
        } else {
            M::merge(&item, &sibling_item)
        };

        if parent_pos < peak_pos {
            queue.push_back((parent_pos, parent_item, height + 1));
        } else {
            return Ok(parent_item);
        }
    }
    Err(Error::CorruptedProof)
}

fn calculate_peaks_hashes<
    'a,
    T: 'a + PartialEq + Debug + Clone,
    M: Merge<Item = T>,
    I: Iterator<Item = &'a T>,
>(
    mut leaves: Vec<(u64, T)>,
    mmr_size: u64,
    mut proof_iter: I,
) -> Result<Vec<T>> {
    // special handle the only 1 leaf MMR
    if mmr_size == 1 && leaves.len() == 1 && leaves[0].0 == 0 {
        return Ok(leaves.into_iter().map(|(_pos, item)| item).collect());
    }
    // sort items by position
    leaves.sort_by_key(|(pos, _)| *pos);
    let peaks = get_peaks(mmr_size);

    let mut peaks_hashes: Vec<T> = Vec::with_capacity(peaks.len() + 1);
    for peak_pos in peaks {
        let mut leaves: Vec<_> = take_while_vec(&mut leaves, |(pos, _)| *pos <= peak_pos);
        let peak_root = if leaves.len() == 1 && leaves[0].0 == peak_pos {
            // leaf is the peak
            leaves.remove(0).1
        } else if leaves.is_empty() {
            // if empty, means the next proof is a peak root or rhs bagged root
            if let Some(peak_root) = proof_iter.next() {
                peak_root.clone()
            } else {
                // means that either all right peaks are bagged, or proof is corrupted
                // so we break loop and check no items left
                break;
            }
        } else {
            calculate_peak_root::<_, M, _>(leaves, peak_pos, &mut proof_iter)?
        };
        peaks_hashes.push(peak_root.clone());
    }

    // ensure nothing left in leaves
    if !leaves.is_empty() {
        return Err(Error::CorruptedProof);
    }

    // check rhs peaks
    if let Some(rhs_peaks_hashes) = proof_iter.next() {
        peaks_hashes.push(rhs_peaks_hashes.clone());
    }
    // ensure nothing left in proof_iter
    if proof_iter.next().is_some() {
        return Err(Error::CorruptedProof);
    }
    Ok(peaks_hashes)
}

fn bagging_peaks_hashes<'a, T: 'a + PartialEq + Debug + Clone, M: Merge<Item = T>>(
    mut peaks_hashes: Vec<T>,
) -> Result<T> {
    // bagging peaks
    // bagging from right to left via hash(right, left).
    while peaks_hashes.len() > 1 {
        let right_peak = peaks_hashes.pop().expect("pop");
        let left_peak = peaks_hashes.pop().expect("pop");
        peaks_hashes.push(M::merge(&right_peak, &left_peak));
    }
    peaks_hashes.pop().ok_or(Error::CorruptedProof)
}

/// merkle proof
/// 1. sort items by position
/// 2. calculate root of each peak
/// 3. bagging peaks
fn calculate_root<
    'a,
    T: 'a + PartialEq + Debug + Clone,
    M: Merge<Item = T>,
    I: Iterator<Item = &'a T>,
>(
    leaves: Vec<(u64, T)>,
    mmr_size: u64,
    proof_iter: I,
) -> Result<T> {
    let peaks_hashes = calculate_peaks_hashes::<_, M, _>(leaves, mmr_size, proof_iter)?;
    bagging_peaks_hashes::<_, M>(peaks_hashes)
}

fn take_while_vec<T, P: Fn(&T) -> bool>(v: &mut Vec<T>, p: P) -> Vec<T> {
    for i in 0..v.len() {
        if !p(&v[i]) {
            return v.drain(..i).collect();
        }
    }
    v.drain(..).collect()
}

#[wasm_bindgen]
pub fn convert(block_num: u64, mmr_size: u64, mmr_proof: &[u8], leaf: &[u8]) -> String {
    let mut proof = <Vec<H256>>::new();
    for i in (0..mmr_proof.len()).step_by(32) {
        let mut proof_ = [0; 32];
        proof_.copy_from_slice(&mmr_proof[i..i + 32]);
        proof.push(proof_.into());
	}
    let leaves = vec![(leaf_index_to_pos(block_num), {
        let mut leaf_ = [0; 32];
        leaf_.copy_from_slice(leaf);

        leaf_.into()
    })];
    let mut proof_hashes = proof.clone();
    let peaks_hashes =
        calculate_peaks_hashes::<H256, MMRMerge, _>(leaves.clone(), mmr_size, proof.iter())
            .unwrap();
    proof_hashes.retain(|h| !contains(&peaks_hashes, h));
    let peaks = peaks_hashes
        .iter()
        .map(|hash| format!("{:?}", hash))
        .collect::<Vec<_>>()
        .join(",");
    let siblings = proof_hashes
        .iter()
        .map(|hash| format!("{:?}", hash))
        .collect::<Vec<_>>()
        .join(",");

    return format!("{}|{}|{}", mmr_size, peaks, siblings);
}

pub struct MMRMerge;

impl Merge for MMRMerge {
    type Item = H256;
    fn merge(lhs: &Self::Item, rhs: &Self::Item) -> Self::Item {
        let encodable = (lhs, rhs);
        BlakeTwo256::hash_of(&encodable)
    }
}

fn contains(hashes: &Vec<H256>, target: &H256) -> bool {
    hashes.iter().find(|&x| x == target).is_some()
}

#[test]
fn test_convert() {
    let mut proof = vec![];
    for proof_hex_str in [
        "0x91bcaaf0182d2a68cb26d61883abf3f352a681a4f53fbfa8e782502aac8756d0",
        "0x5eb822a9c78ac1e0e3c4c7ca1a7c15e47df67627b6a1cc94a39cd1bcc3ed0ed6",
        "0xe8851435697be9e0bdf6b58569581d3331b6b8ae2d624fc702c74d1ba5044d25",
        "0x029ce80dc5ba1f5e10da74d831563311b6d77f564f3c9036a682e9ea63cccafe",
        "0xba89e9b3e3524df5a80257e78fff815b501ed694c58696190292d05d235d1cbd",
        "0x32e33d3743aa1c8fb2dd02e397ae882745e57917a165031bd57334d89cbb9216",
        "0x5fcc7f36411473041fed141924da53cf31d1c9eabfc41eae3deb2d3b5052417d",
        "0x2193c7b130358d5d04e3c0f2f54988d51cac61de459bd44062600763f40ebb99",
        "0xa471a6aa13f5f34b70447d9381b7786ee55561aadebdabcae30c36491fac1396",
        "0x802029e8de6f0b99f574080313ae749b0787c82f73d13a5d69eed028eaff6169",
    ]
    .iter()
    {
        proof.append(&mut array_bytes::bytes_unchecked(proof_hex_str));
    }
    let leaf = array_bytes::bytes_unchecked(
        "b8d165cc6a13de707a646acd52b1f8d3d45ef6877b005ea3ae576937fe2e5822",
    );
    let c = convert(271475, 542954, &proof, &leaf);
    print!("{}", c)
}

#[test]
fn test_peakBagging() {
    let mut mmr_proof = vec![];
    for proof_hex_str in [
        "0xfc94dd28d893d7628d0c7769d2cc0a51354944305cb522570f2bb67fb5b0d37b",
        "0x3dea9908a10d8e9cc807f93f65d55b4c7bf84d41c4dc0b4e70215332aeda483e",
        "0x084631199357bd0e8a6ca232c3f77e08cba4989581ded276c7187ee30e800dc6",
        "0xbe3541b92633bec4c5f66173e84e08bcacb45f731bb8e0afc837310db93e01c2",
        "0x45b4492dc617494e67ea5de3883bcccb8a1fb4933ead9015943f2ef7ca56e0cc",
        "0xb93c5bb679241d7142fdb43c2be2e6dd8abd49a236b317cfca7a033b2a1435fe",
        "0x58fd8aef01e364a83edf9ffcd2f46668ce07e0881b0fab0e33f1fa696a9e4cbe",
        "0x13e46905134952dcad7ea16f53c8c6cbef8730483f77424fc8482895898c3dbc",
        "0x075fdc850bb7d1b4a47144725a7d2fe04135f3270b753d7bf5174a047cf4835b",
        "0xe5c278613ee8455a215660ae20d8922b5db76735d8714bc78a709a1f12b071c4",
        "0xb698b4fc3afc36de530c95e7f73ebef7929dc91b446fe401eb32af5684f92687",
        "0x6137c159cee8c345652cbeb0e69cbefb7c4d5cd0f932a153c9979fc5c6ce3a66",
        "0x8194ddd444c66b47862347d450c15f19ea969d468e692d1fd87a510dae666064",
        "0xc2764c84ed2883fab1fa93d5bba4f97b10f7336b57876515115e0bd8ee35a83e",
        "0x29af35fc65cf938f99326298e20c29f86c66fd9d5d7ab0578d00d9e69bd3ab02",
        "0x00643a6a7a7760975b36d6d5eea9f07a96a40b3c36423d5686369ed01a856e93",
        "0xcdd7e2712810c06fe356c163ceba267e8c7db40adc9d989527084831a739b277",
        "0x277812899431a221ad7e155ab9d6373710b2ba2ecc07113cb50f2db25094fd76",
        "0x256898cde8a0fd17496bb466a57bdd4acdf1cfb5fa4f3365093b0a6e819dda0c",
        "0x02f3cac9bbb81d99b2c9c78fb76ddd192af6d0212f3d172c125225872dd2ff4f",
        "0xd04985859b216d605ccb6f79ad4cbf7b28797205605d9b2e695f3140b2bc081d",
    ]
    .iter()
    {
        mmr_proof.append(&mut array_bytes::bytes_unchecked(proof_hex_str));
    }

    let leaf = array_bytes::bytes_unchecked(
        "0x133fdb7f9459fda9a2420b5717643af2588b4eb305792b9615f31b4a610b8d34",
    );

    let expected_root = array_bytes::bytes_unchecked(
        "0x9fb345285fbea92ce003fd982f60b52a65144953fac5d082ab45651d9525c80c",
    );

    let mut proof = <Vec<H256>>::new();
    for i in (0..mmr_proof.len()).step_by(32) {
        let mut proof_ = [0; 32];
        proof_.copy_from_slice(&mmr_proof[i..i + 32]);
        proof.push(proof_.into());
    }
    let leaves = vec![(leaf_index_to_pos(5689148), {
        let mut leaf_ = [0; 32];
        leaf_.copy_from_slice(&leaf);

        leaf_.into()
    })];
    let mut proof_hashes = proof.clone();
    let peaks_hashes =
        calculate_peaks_hashes::<H256, MMRMerge, _>(leaves.clone(), 11445390, proof.iter())
            .unwrap();

    let root = bagging_peaks_hashes::<_, MMRMerge>(peaks_hashes).unwrap();

    let mut expected_root_v = [0; 32];
    expected_root_v.copy_from_slice(&expected_root);
    // let c = convert(5689148, 11445390, &proof, &leaf);
    assert_eq!(H256(expected_root_v), root);

    let a_root = calculate_root::<_, MMRMerge, _>(leaves.clone(), 11445390, proof.iter()).unwrap();

    assert_eq!(a_root, root);

}
