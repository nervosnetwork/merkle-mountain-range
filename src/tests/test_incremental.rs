use proptest::proptest;

use super::{MergeNumberHash, NumberHash};
use crate::util::{MemMMR, MemStore};

proptest! {
    #[test]
    fn test_incremental(start in 1u32..500, steps in 1usize..50, turns in 10usize..20) {
        test_incremental_with_params(start, steps, turns);
    }
}

fn test_incremental_with_params(start: u32, steps: usize, turns: usize) {
    let store = MemStore::default();
    let mut mmr = MemMMR::<_, MergeNumberHash>::new(0, &store);

    let mut curr = 0;

    let _positions: Vec<u64> = (0u32..start)
        .map(|_| {
            let pos = mmr.push(NumberHash::from(curr)).unwrap();
            curr += 1;
            pos
        })
        .collect();
    mmr.commit().expect("commit changes");

    for turn in 0..turns {
        let prev_root = mmr.get_root().expect("get root");
        let (positions, leaves) = (0..steps).fold(
            (Vec::new(), Vec::new()),
            |(mut positions, mut leaves), _| {
                let leaf = NumberHash::from(curr);
                let pos = mmr.push(leaf.clone()).unwrap();
                curr += 1;
                positions.push(pos);
                leaves.push(leaf);
                (positions, leaves)
            },
        );
        mmr.commit().expect("commit changes");
        let proof = mmr.gen_proof(positions).expect("gen proof");
        let root = mmr.get_root().expect("get root");
        let result = proof.verify_incremental(root, prev_root, leaves).unwrap();
        assert!(
            result,
            "start: {}, steps: {}, turn: {}, curr: {}",
            start, steps, turn, curr
        );
    }
}
