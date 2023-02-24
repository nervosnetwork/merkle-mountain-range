use super::{run_mmr_verify, Blake2bHash, VariableBytes};
use ckb_merkle_mountain_range::{
    compiled_proof::{
        pack_compiled_merkle_proof, pack_leaves, verify, PackedLeaves, PackedMerkleProof,
        ValueMerge,
    },
    util::MemStore,
    MMR,
};
use proptest::prelude::*;
use rand::{rngs::StdRng, Rng, RngCore, SeedableRng};

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
    fn test_random_proof_in_c(((count, test_leaf_indices), seed) in (leaves(10, 1000), 0..=u64::MAX)) {
        let mut rng = StdRng::seed_from_u64(seed);

        let store = MemStore::default();
        let mut mmr = MMR::<_, Blake2bHash, _>::new(0, &store);

        let all_leaves: Vec<_> = (0..count).map(|_| {
            let size = rng.gen_range(30..50);
            let mut data = vec![0u8; size];
            rng.fill_bytes(&mut data[..]);
            let value = VariableBytes(data);

            let position = mmr.push(value.clone()).expect("push");

            (position, value)
        }).collect();
        let mmr_size = mmr.mmr_size();
        let root = mmr.get_root().expect("get root");

        let tested_leaves: Vec<_> = test_leaf_indices
            .iter()
            .map(|elem| all_leaves[*elem as usize].clone())
            .collect();
        let tested_positions: Vec<_> = tested_leaves
            .iter()
            .map(|(pos, _)| *pos)
            .collect();
        let proof = mmr.gen_proof(tested_positions.clone()).expect("gen proof");
        let compiled_proof = proof
            .compile::<ValueMerge<_>>(tested_positions.clone())
            .expect("compile proof");
        mmr.commit().expect("commit changes");

        let packed_proof = pack_compiled_merkle_proof(&compiled_proof).expect("pack proof");
        let packed_leaves = pack_leaves(&tested_leaves).expect("pack leaves");

        // Verifying in Rust
        {
            let mut packed_proof: PackedMerkleProof<VariableBytes> =
                PackedMerkleProof::new(&packed_proof);
            let mut packed_leaves = PackedLeaves::new(&packed_leaves);

            let result = verify::<_, Blake2bHash, _, _>(
                &mut packed_proof,
                root.clone(),
                mmr_size,
                &mut packed_leaves,
            ).unwrap();
            assert!(result);
        }

        // Verifying in C
        unsafe {
            let result = run_mmr_verify(
                root.0.as_ptr(),
                root.0.len().try_into().unwrap(),
                mmr_size,
                packed_proof.as_ptr(),
                packed_proof.len().try_into().unwrap(),
                packed_leaves.as_ptr(),
                packed_leaves.len().try_into().unwrap(),
            );
            assert_eq!(result, 0);
        }
    }
}
