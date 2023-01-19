use crate::mmr::take_while_vec;
use crate::vec::Vec;
use crate::vec;

pub fn leaf_index_to_pos(index: u64) -> u64 {
    // mmr_size - H - 1, H is the height(intervals) of last peak
    leaf_index_to_mmr_size(index) - (index + 1).trailing_zeros() as u64 - 1
}

pub fn leaf_index_to_mmr_size(index: u64) -> u64 {
    // leaf index start with 0
    let leaves_count = index + 1;

    // the peak count(k) is actually the count of 1 in leaves count's binary representation
    let peak_count = leaves_count.count_ones() as u64;

    2 * leaves_count - peak_count
}

pub fn pos_height_in_tree(mut pos: u64) -> u32 {
    pos += 1;
    fn all_ones(num: u64) -> bool {
        num != 0 && num.count_zeros() == num.leading_zeros()
    }
    fn jump_left(pos: u64) -> u64 {
        let bit_length = 64 - pos.leading_zeros();
        let most_significant_bits = 1 << (bit_length - 1);
        pos - (most_significant_bits - 1)
    }

    while !all_ones(pos) {
        pos = jump_left(pos)
    }

    64 - pos.leading_zeros() - 1
}

pub fn parent_offset(height: u32) -> u64 {
    2 << height
}

pub fn sibling_offset(height: u32) -> u64 {
    (2 << height) - 1
}

pub fn get_peaks(mmr_size: u64) -> Vec<u64> {
    let mut pos_s = Vec::new();
    let (mut height, mut pos) = left_peak_height_pos(mmr_size);
    pos_s.push(pos);
    while height > 0 {
        let peak = match get_right_peak(height, pos, mmr_size) {
            Some(peak) => peak,
            None => break,
        };
        height = peak.0;
        pos = peak.1;
        pos_s.push(pos);
    }
    pos_s
}

fn get_right_peak(mut height: u32, mut pos: u64, mmr_size: u64) -> Option<(u32, u64)> {
    // move to right sibling pos
    pos += sibling_offset(height);
    // loop until we find a pos in mmr
    while pos > mmr_size - 1 {
        if height == 0 {
            return None;
        }
        // move to left child
        pos -= parent_offset(height - 1);
        height -= 1;
    }
    Some((height, pos))
}

fn get_peak_pos_by_height(height: u32) -> u64 {
    (1 << (height + 1)) - 2
}

fn left_peak_height_pos(mmr_size: u64) -> (u32, u64) {
    let mut height = 1;
    let mut prev_pos = 0;
    let mut pos = get_peak_pos_by_height(height);
    while pos < mmr_size {
        height += 1;
        prev_pos = pos;
        pos = get_peak_pos_by_height(height);
    }
    (height - 1, prev_pos)
}

pub fn mmr_position_to_k_index(mut leaves: Vec<u64>, mmr_size: u64) -> Vec<(u64, usize)> {
    let peaks = get_peaks(mmr_size);
    let mut leaves_with_k_indices = vec![];

    for peak in peaks {
        let leaves: Vec<_> = take_while_vec(&mut leaves, |pos| *pos <= peak);

        if leaves.len() > 0 {
            for pos in leaves {
                let height = pos_height_in_tree(peak);
                let mut index = 0;
                let mut parent_pos = peak;
                for height in (1..=height).rev() {
                    let left_child = parent_pos - parent_offset(height - 1);
                    let right_child = left_child + sibling_offset(height - 1);
                    index *= 2;
                    if left_child >= pos {
                        parent_pos = left_child;
                    } else {
                        parent_pos = right_child;
                        index += 1;
                    }
                }

                leaves_with_k_indices.push((pos, index));
            }
        }
    }

    leaves_with_k_indices
}
