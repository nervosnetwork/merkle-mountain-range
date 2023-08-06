use crate::mmr::take_while_vec;
use crate::vec;
use crate::vec::Vec;

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

pub fn pos_height_in_tree(mut pos: u64) -> u8 {
    if pos == 0 {
        return 0;
    }

    let mut peak_size = u64::MAX >> pos.leading_zeros();
    while peak_size > 0 {
        if pos >= peak_size {
            pos -= peak_size;
        }
        peak_size >>= 1;
    }
    pos as u8
}

pub fn parent_offset(height: u8) -> u64 {
    2 << height
}

pub fn sibling_offset(height: u8) -> u64 {
    (2 << height) - 1
}

/// Returns the height of the peaks in the mmr, presented by a bitmap.
/// for example, for a mmr with 11 leaves, the mmr_size is 19, it will return 0b1011.
/// 0b1011 indicates that the left peaks are at height 0, 1 and 3.
///           14
///        /       \
///      6          13
///    /   \       /   \
///   2     5     9     12     17
///  / \   /  \  / \   /  \   /  \
/// 0   1 3   4 7   8 10  11 15  16 18
///
/// please note that when the mmr_size is invalid, it will return the bitmap of the last valid mmr.
/// in the below example, the mmr_size is 6, but it's not a valid mmr, it will return 0b11.
///   2     5
///  / \   /  \
/// 0   1 3   4
pub fn get_peak_map(mmr_size: u64) -> u64 {
    if mmr_size == 0 {
        return 0;
    }

    let mut pos = mmr_size;
    let mut peak_size = u64::MAX >> pos.leading_zeros();
    let mut peak_map = 0;
    while peak_size > 0 {
        peak_map <<= 1;
        if pos >= peak_size {
            pos -= peak_size;
            peak_map |= 1;
        }
        peak_size >>= 1;
    }

    peak_map
}

/// Returns the pos of the peaks in the mmr.
/// for example, for a mmr with 11 leaves, the mmr_size is 19, it will return [14, 17, 18].
///           14
///        /       \
///      6          13
///    /   \       /   \
///   2     5     9     12     17
///  / \   /  \  / \   /  \   /  \
/// 0   1 3   4 7   8 10  11 15  16 18
///
/// please note that when the mmr_size is invalid, it will return the peaks of the last valid mmr.
/// in the below example, the mmr_size is 6, but it's not a valid mmr, it will return [2, 3].
///   2     5
///  / \   /  \
/// 0   1 3   4
pub fn get_peaks(mmr_size: u64) -> Vec<u64> {
    if mmr_size == 0 {
        return vec![];
    }

    let leading_zeros = mmr_size.leading_zeros();
    let mut pos = mmr_size;
    let mut peak_size = u64::MAX >> leading_zeros;
    let mut peaks = Vec::with_capacity(64 - leading_zeros as usize);
    let mut peaks_sum = 0;
    while peak_size > 0 {
        if pos >= peak_size {
            pos -= peak_size;
            peaks.push(peaks_sum + peak_size - 1);
            peaks_sum += peak_size;
        }
        peak_size >>= 1;
    }
    peaks
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
