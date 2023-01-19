use super::{MergeNumberHash, NumberHash};
use crate::mmr::{take_while_vec};
use crate::{helper::pos_height_in_tree, leaf_index_to_mmr_size, util::MemStore, Error, MMRStore, Merge, MMR, mmr_position_to_k_index};
use faster_hex::hex_string;
use num::{Integer, Zero};
use num::integer::div_floor;
use proptest::prelude::*;
use rand::{seq::SliceRandom, thread_rng};
use tiny_keccak::keccak256;
use crate::helper::get_peaks;

fn test_mmr(count: u32, proof_elem: Vec<u32>) {
    let store = MemStore::default();
    let mut mmr = MMR::<_, MergeNumberHash, _>::new(0, &store);
    let positions: Vec<u64> = (0u32..count)
        .map(|i| mmr.push(NumberHash::from(i)).unwrap())
        .collect();
    let root = mmr.get_root().expect("get root");
    let proof = mmr
        .gen_proof(
            proof_elem
                .iter()
                .map(|elem| positions[*elem as usize])
                .collect(),
        )
        .expect("gen proof");
    mmr.commit().expect("commit changes");
    let result = proof
        .verify(
            root,
            proof_elem
                .iter()
                .map(|elem| (positions[*elem as usize], NumberHash::from(*elem)))
                .collect(),
        )
        .unwrap();
    assert!(result);
}

fn test_gen_new_root_from_proof(count: u32) {
    let store = MemStore::default();
    let mut mmr = MMR::<_, MergeNumberHash, _>::new(0, &store);
    let positions: Vec<u64> = (0u32..count)
        .map(|i| mmr.push(NumberHash::from(i)).unwrap())
        .collect();
    let elem = count - 1;
    let pos = positions[elem as usize];
    let proof = mmr.gen_proof(vec![pos]).expect("gen proof");
    let new_elem = count;
    let new_pos = mmr.push(NumberHash::from(new_elem)).unwrap();
    let root = mmr.get_root().expect("get root");
    mmr.commit().expect("commit changes");
    let calculated_root = proof
        .calculate_root_with_new_leaf(
            vec![(pos, NumberHash::from(elem))],
            new_pos,
            NumberHash::from(new_elem),
            leaf_index_to_mmr_size(new_elem.into()),
        )
        .unwrap();
    assert_eq!(calculated_root, root);
}

#[test]
fn test_mmr_root() {
    let store = MemStore::default();
    let mut mmr = MMR::<_, MergeNumberHash, _>::new(0, &store);
    (0u32..11).for_each(|i| {
        mmr.push(NumberHash::from(i)).unwrap();
    });
    let root = mmr.get_root().expect("get root");
    let hex_root = hex_string(&root.0);
    assert_eq!(
        "f6794677f37a57df6a5ec36ce61036e43a36c1a009d05c81c9aa685dde1fd6e3",
        hex_root
    );
}

#[test]
fn test_empty_mmr_root() {
    let store = MemStore::<NumberHash>::default();
    let mmr = MMR::<_, MergeNumberHash, _>::new(0, &store);
    assert_eq!(Err(Error::GetRootOnEmpty), mmr.get_root());
}

#[test]
fn test_mmr_3_peaks() {
    test_mmr(11, vec![5]);
}

#[test]
fn test_mmr_2_peaks() {
    test_mmr(10, vec![5]);
}

#[test]
fn test_mmr_1_peak() {
    test_mmr(8, vec![5]);
}

#[test]
fn test_mmr_first_elem_proof() {
    test_mmr(11, vec![0]);
}

#[test]
fn test_mmr_last_elem_proof() {
    test_mmr(11, vec![10]);
}

#[test]
fn test_mmr_1_elem() {
    test_mmr(1, vec![0]);
}

#[test]
fn test_mmr_2_elems() {
    test_mmr(2, vec![0]);
    test_mmr(2, vec![1]);
}

#[test]
fn test_mmr_2_leaves_merkle_proof() {
    test_mmr(11, vec![3, 7]);
    test_mmr(11, vec![3, 4]);
}

#[test]
fn test_mmr_2_sibling_leaves_merkle_proof() {
    test_mmr(11, vec![4, 5]);
    test_mmr(11, vec![5, 6]);
    test_mmr(11, vec![6, 7]);
}

#[test]
fn test_mmr_3_leaves_merkle_proof() {
    test_mmr(11, vec![4, 5, 6]);
    test_mmr(11, vec![3, 5, 7]);
    test_mmr(11, vec![3, 4, 5]);
    test_mmr(100, vec![3, 5, 13]);
}

#[test]
fn test_gen_root_from_proof() {
    test_gen_new_root_from_proof(11);
}

#[test]
fn test_gen_proof_with_duplicate_leaves() {
    test_mmr(10, vec![5, 5]);
}

fn test_invalid_proof_verification(
    leaf_count: u32,
    positions_to_verify: Vec<u64>,
    // positions of entries that should be tampered
    tampered_positions: Vec<usize>,
    // optionally handroll proof from these positions
    handrolled_proof_positions: Option<Vec<u64>>,
) {
    use crate::{util::MemMMR, MerkleProof};
    use std::fmt::{Debug, Formatter};

    // Simple item struct to allow debugging the contents of MMR nodes/peaks
    #[derive(Clone, PartialEq)]
    enum MyItem {
        Number(u32),
        Merged(Box<MyItem>, Box<MyItem>),
    }

    impl Debug for MyItem {
        fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            match self {
                MyItem::Number(x) => f.write_fmt(format_args!("{}", x)),
                MyItem::Merged(a, b) => f.write_fmt(format_args!("Merged({:#?}, {:#?})", a, b)),
            }
        }
    }

    #[derive(Debug)]
    struct MyMerge;

    impl Merge for MyMerge {
        type Item = MyItem;
        fn merge(lhs: &Self::Item, rhs: &Self::Item) -> Result<Self::Item, crate::Error> {
            Ok(MyItem::Merged(Box::new(lhs.clone()), Box::new(rhs.clone())))
        }
    }

    let mut mmr: MemMMR<MyItem, MyMerge> = MemMMR::default();
    let mut positions: Vec<u64> = Vec::new();
    for i in 0u32..leaf_count {
        let pos = mmr.push(MyItem::Number(i)).unwrap();
        positions.push(pos);
    }
    let root = mmr.get_root().unwrap();

    let entries_to_verify: Vec<(u64, MyItem)> = positions_to_verify
        .iter()
        .map(|pos| (*pos, mmr.store().get_elem(*pos).unwrap().unwrap()))
        .collect();

    let mut tampered_entries_to_verify = entries_to_verify.clone();
    tampered_positions.iter().for_each(|proof_pos| {
        tampered_entries_to_verify[*proof_pos] = (
            tampered_entries_to_verify[*proof_pos].0,
            MyItem::Number(31337),
        )
    });

    let handrolled_proof: Option<MerkleProof<MyItem, MyMerge>> =
        handrolled_proof_positions.map(|handrolled_proof_positions| {
            MerkleProof::new(
                mmr.mmr_size(),
                handrolled_proof_positions
                    .iter()
                    .map(|pos| mmr.store().get_elem(*pos).unwrap().unwrap())
                    .collect(),
            )
        });

    // verification should fail whenever trying to prove membership of a non-member
    if let Some(handrolled_proof) = handrolled_proof {
        let handrolled_proof_result =
            handrolled_proof.verify(root.clone(), tampered_entries_to_verify.clone());
        assert!(handrolled_proof_result.is_err() || !handrolled_proof_result.unwrap());
    }

    match mmr.gen_proof(positions_to_verify.clone()) {
        Ok(proof) => {
            assert!(proof.verify(root.clone(), entries_to_verify).unwrap());
            assert!(!proof.verify(root, tampered_entries_to_verify).unwrap());
        }
        Err(Error::NodeProofsNotSupported) => {
            // if couldn't generate proof, then it contained a non-leaf
            assert!(positions_to_verify
                .iter()
                .any(|pos| pos_height_in_tree(*pos) > 0));
        }
        Err(e) => panic!("Unexpected error: {}", e),
    }
}

#[test]
fn test_generic_proofs() {
    test_invalid_proof_verification(7, vec![5], vec![0], Some(vec![2, 9, 10]));
    test_invalid_proof_verification(7, vec![1, 2], vec![0], Some(vec![5, 9, 10]));
    test_invalid_proof_verification(7, vec![1, 5], vec![0], Some(vec![0, 9, 10]));
    test_invalid_proof_verification(7, vec![1, 6], vec![0], Some(vec![0, 5, 9, 10]));
    test_invalid_proof_verification(7, vec![5, 6], vec![0], Some(vec![2, 9, 10]));
    test_invalid_proof_verification(7, vec![1, 5, 6], vec![0], Some(vec![0, 9, 10]));
    test_invalid_proof_verification(7, vec![1, 5, 7], vec![0], Some(vec![0, 8, 10]));
    test_invalid_proof_verification(7, vec![5, 6, 7], vec![0], Some(vec![2, 8, 10]));
    test_invalid_proof_verification(7, vec![5, 6, 7, 8, 9, 10], vec![0], Some(vec![2]));
    test_invalid_proof_verification(7, vec![1, 5, 7, 8, 9, 10], vec![0], Some(vec![0]));
    test_invalid_proof_verification(7, vec![0, 1, 5, 7, 8, 9, 10], vec![0], Some(vec![]));
    test_invalid_proof_verification(7, vec![0, 1, 5, 6, 7, 8, 9, 10], vec![0], Some(vec![]));
    test_invalid_proof_verification(7, vec![0, 1, 2, 5, 6, 7, 8, 9, 10], vec![0], Some(vec![]));
    test_invalid_proof_verification(7, vec![0, 1, 2, 3, 7, 8, 9, 10], vec![0], Some(vec![4]));
    test_invalid_proof_verification(7, vec![0, 2, 3, 7, 8, 9, 10], vec![0], Some(vec![1, 4]));
    test_invalid_proof_verification(7, vec![0, 3, 7, 8, 9, 10], vec![0], Some(vec![1, 4]));
    test_invalid_proof_verification(7, vec![0, 2, 3, 7, 8, 9, 10], vec![0], Some(vec![1, 4]));
}

prop_compose! {
    fn count_elem(count: u32)
                (elem in 0..count)
                -> (u32, u32) {
                    (count, elem)
    }
}

proptest! {
    #[test]
    fn test_random_mmr(count in 10u32..500u32) {
        let mut leaves: Vec<u32> = (0..count).collect();
        let mut rng = thread_rng();
        leaves.shuffle(&mut rng);
        let leaves_count = rng.gen_range(1..count - 1);
        leaves.truncate(leaves_count as usize);
        test_mmr(count, leaves);
    }

    #[test]
    fn test_random_gen_root_with_new_leaf(count in 1u32..500u32) {
        test_gen_new_root_from_proof(count);
    }
}

struct MergeKeccak;

impl Merge for MergeKeccak {
    type Item = NumberHash;
    fn merge(lhs: &Self::Item, rhs: &Self::Item) -> Result<Self::Item, Error> {
        let mut concat = vec![];
        concat.extend(&lhs.0);
        concat.extend(&rhs.0);
        let hash = keccak256(&concat);
        Ok(NumberHash(hash.to_vec().into()))
    }
}


type Hash = [u8; 32];

pub fn calculate_merkle_multi_root(proof: Vec<Vec<(usize, [u8; 32])>>) -> [u8; 32] {
    let mut previous_layer = vec![];
    for layer in proof {
        let mut current_layer = vec![];
        if previous_layer.len() == 0 {
            current_layer = layer;
        } else {
            current_layer.extend(previous_layer.drain(..));
            current_layer.extend(&layer);
            current_layer.sort_by(|(a_i, _), (b_i, _)| a_i.cmp(b_i));
        }

        for index in (0..current_layer.len()).step_by(2) {
            if index + 1 >= current_layer.len() {
                let node = current_layer[index].clone();
                previous_layer.push((div_floor(node.0, 2), node.1));
            } else {
                let mut concat = vec![];
                let left = current_layer[index].clone();
                let right = current_layer[index + 1].clone();
                concat.extend(&left.1);
                concat.extend(&right.1);
                let hash = keccak256(&concat);

                previous_layer.push((div_floor(left.0, 2), hash));
            }
        }
    }

    debug_assert_eq!(previous_layer.len(), 1);

    previous_layer[0].1
}

fn sibling_indices(indices: Vec<usize>) -> Vec<usize> {
    let mut siblings = Vec::new();

    for index in indices {
        let index = if index.is_zero() {
            index + 1
        } else if index.is_even() {
            index + 1
        } else {
            index - 1
        };
        siblings.push(index);
    }



    siblings
}

fn parent_indices(indices: Vec<usize>) -> Vec<usize> {
    let mut parents = Vec::new();


    for index in indices {
        let index = div_floor(index, 2);
        parents.push(index);
    }


    parents
}

pub fn calculate_peak_roots(
    mut leaves: Vec<(u64, usize, Hash)>,
    mmr_size: u64,
    mut proof_iter: Vec<Hash>,
) -> Hash {
    let peaks = get_peaks(mmr_size);
    let mut peak_roots = vec![];

    for peak in peaks {
        let mut leaves: Vec<_> = take_while_vec(&mut leaves, |(pos, _, _)| *pos <= peak);

        match leaves.len() {
            1 if leaves[0].0 == peak => {
                // this is a peak root.
                peak_roots.push(leaves.pop().unwrap().2);
            }
            0 => {
                // the next proof item is a peak
                if let Some(peak) = proof_iter.pop() {
                    peak_roots.push(peak)
                } else {
                    break;
                }
            }
            _ => {

                let leaves = leaves
                    .into_iter()
                    .map(|(_pos, index, leaf)| {
                        (index, leaf)
                    })
                    .collect::<Vec<_>>();

                let height = pos_height_in_tree(peak);
                let mut current_layer: Vec<_> = leaves.iter().map(|(i, _)| *i).collect();
                let mut layers: Vec<Vec<_>> = vec![];

                for i in 0..height {
                    let siblings = sibling_indices(current_layer.clone().drain(..).collect());
                    let diff = difference(&siblings, &current_layer);
                    if diff.len() == 0 {
                        // fill the remaining layers
                        layers.extend((i..height).map(|_| vec![]));
                        break;
                    }

                    let len = diff.len();
                    let proof = diff.into_iter().zip(proof_iter.drain(..len)).collect();
                    layers.push(proof);
                    current_layer = parent_indices(siblings);

                    if i == 0 {
                        // insert the leaves
                        layers[0].extend(&leaves);
                        layers[0].sort_by(|a, b| a.0.cmp(&b.0));
                    }
                }

                let peak_root = calculate_merkle_multi_root(layers);
                peak_roots.push(peak_root);
            }
        };
    }

    // bagging peaks
    // bagging from right to left via hash(right, left).
    while peak_roots.len() > 1 {
        let right_peak = peak_roots.pop().expect("pop");
        let left_peak = peak_roots.pop().expect("pop");
        let mut buf = vec![];
        buf.extend(&right_peak);
        buf.extend(&left_peak);

        peak_roots.push(keccak256(&buf));
    }
    peak_roots.pop().unwrap()
}

fn difference(right: &Vec<usize>, left: &Vec<usize>) -> Vec<usize> {
    let mut out = vec![];

    for item in right {
        let mut found = false;
        for i in left {
            if item == i {
                found = true;
                break;
            }
        }

        if !found {
            out.push(*item);
        }

    }
    out
}

#[test]
fn test_simplified_mmr_verification_algorithm() {
    let store = MemStore::default();
    let mut mmr = MMR::<_, MergeKeccak, _>::new(0, &store);
    let positions: Vec<u64> = (0u32..=13)
        .map(|i| mmr.push(NumberHash::from(i)).unwrap())
        .collect();
    let root = mmr.get_root().expect("get root");
    let proof = mmr
        .gen_proof(vec![
            positions[2],
            positions[5],
            positions[8],
            positions[10],
            positions[12],
        ])
        .unwrap();

    let leaves = vec![
        (NumberHash::from(2), positions[2]),
        (NumberHash::from(5), positions[5]),
        (NumberHash::from(8), positions[8]),
        (NumberHash::from(10), positions[10]),
        (NumberHash::from(12), positions[12]),
    ]
        .into_iter()
        .map(|(a, b)| (b, a))
        .collect::<Vec<_>>();

    let positions = leaves.iter().map(|(pos, _)| *pos).collect();
    let pos_with_index = mmr_position_to_k_index(positions, proof.mmr_size());

    let mut custom_leaves = pos_with_index
        .into_iter()
        .zip(leaves.clone())
        .map(|((pos, index), (_, leaf))| {
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&leaf.0);
            (pos, index, hash)
        })
        .collect::<Vec<_>>();


    custom_leaves.sort_by(|(a_pos, _, _), (b_pos, _, _)| a_pos.cmp(b_pos));

    let nodes = proof
        .proof_items()
        .iter()
        .map(|n| {
            let mut buf = [0u8; 32];
            buf.copy_from_slice(&n.0[..]);
            buf
        })
        .collect();

    let calculated = calculate_peak_roots(custom_leaves, proof.mmr_size(), nodes);

    assert_eq!(calculated.to_vec(), root.0.to_vec());
}
