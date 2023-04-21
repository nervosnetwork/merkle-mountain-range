use super::{MergeNumberHash, NumberHash};
use crate::{
    compiled_proof::{
        pack_compiled_merkle_proof, pack_leaves, verify, CompiledMerkleProof, PackedLeaves,
        PackedMerkleProof, ValueMerge,
    },
    util::MemStore,
    MMR,
};
use proptest::prelude::*;

fn build_compiled_proof(
    count: u32,
    proof_elem: Vec<u32>,
) -> (
    CompiledMerkleProof<NumberHash>,
    NumberHash,
    u64,
    Vec<(u64, NumberHash)>,
) {
    let store = MemStore::default();
    let mut mmr = MMR::<_, MergeNumberHash, _>::new(0, &store);
    let positions: Vec<u64> = (0u32..count)
        .map(|i| mmr.push(NumberHash::from(i)).unwrap())
        .collect();
    let mmr_size = mmr.mmr_size();
    let root = mmr.get_root().expect("get root");
    let test_positions: Vec<u64> = proof_elem
        .iter()
        .map(|elem| positions[*elem as usize])
        .collect();

    let proof = mmr.gen_proof(test_positions.clone()).expect("gen proof");
    mmr.commit().expect("commit changes");

    let compiled_proof = proof
        .compile::<ValueMerge<_>>(test_positions)
        .expect("compile proof");

    (
        compiled_proof,
        root,
        mmr_size,
        proof_elem
            .iter()
            .map(|elem| (positions[*elem as usize], NumberHash::from(*elem)))
            .collect(),
    )
}

fn leaves(min_leaves: u32, max_leaves: u32) -> impl Strategy<Value = (u32, Vec<u32>)> {
    prop::sample::select((min_leaves..max_leaves).collect::<Vec<u32>>()).prop_flat_map(
        |count: u32| {
            (
                Just(count),
                prop::sample::subsequence((0..count).collect::<Vec<_>>(), 1..=count as usize),
            )
        },
    )
}

proptest! {
    #[test]
    fn test_random_compiled_proof((count, test_leaves) in leaves(10, 1000)) {
        let (compiled_proof, root, mmr_size, leaves) = build_compiled_proof(count, test_leaves);

        let result = compiled_proof
            .verify::<MergeNumberHash>(
                root,
                mmr_size,
                leaves,
            ).unwrap();
        assert!(result);
    }

    #[test]
    fn test_packed_compiled_proof((count, test_leaves) in leaves(10, 1000)) {
        let (compiled_proof, root, mmr_size, leaves) = build_compiled_proof(count, test_leaves);

        let proof_data: Vec<u8> = pack_compiled_merkle_proof(&compiled_proof).expect("serialize");
        let mut packed_proof: PackedMerkleProof<NumberHash> =
            PackedMerkleProof::new(&proof_data);

        let result = verify::<_, MergeNumberHash, _, _>(
            &mut packed_proof,
            root,
            mmr_size,
            &mut leaves.into_iter().map(Ok),
        ).unwrap();
        assert!(result);
    }

    #[test]
    fn test_packed_leaves((count, test_leaves) in leaves(10, 1000)) {
        let (compiled_proof, root, mmr_size, leaves) = build_compiled_proof(count, test_leaves);

        let proof_data: Vec<u8> = pack_compiled_merkle_proof(&compiled_proof).expect("serialize");
        let mut packed_proof: PackedMerkleProof<NumberHash> =
            PackedMerkleProof::new(&proof_data);

        let leaves_data = pack_leaves(&leaves).expect("pack leaves");
        let mut packed_leaves = PackedLeaves::new(&leaves_data);

        let result = verify::<_, MergeNumberHash, _, _>(
            &mut packed_proof,
            root,
            mmr_size,
            &mut packed_leaves,
        ).unwrap();
        assert!(result);
    }
}
