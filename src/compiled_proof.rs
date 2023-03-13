use crate::{
    boxed::Box,
    helper::{parent_offset, pos_height_in_tree, sibling_offset, PeakIterator},
    mmr::calculate_peaks_hashes,
    vec::Vec,
    Error, Merge, Result,
};
use core::fmt::Debug;
use core::marker::PhantomData;
use core::ops::RangeInclusive;

pub trait Packable: Sized {
    fn pack(&self) -> Result<Vec<u8>>;
    fn unpack(data: &[u8]) -> Result<(Self, usize)>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value<T> {
    Proof(T),
    LeafIndex(usize),
    Merged(Box<Value<T>>, Box<Value<T>>, RangeInclusive<usize>),
}

impl<T> Value<T> {
    fn leaf_range(&self) -> RangeInclusive<usize> {
        #[allow(clippy::reversed_empty_ranges)]
        match self {
            Value::LeafIndex(i) => (*i)..=(*i),
            Value::Merged(_, _, r) => r.clone(),
            // Proof has empty range
            Value::Proof(_) => 1..=0,
        }
    }
}

pub struct ValueMerge<T> {
    value: PhantomData<T>,
}

impl<T> ValueMerge<T> {
    fn merged_range(left: &Value<T>, right: &Value<T>) -> Result<RangeInclusive<usize>> {
        let lr = left.leaf_range();
        let rr = right.leaf_range();

        let r = if lr.is_empty() {
            rr
        } else if rr.is_empty() {
            lr
        } else if lr.end() + 1 == *rr.start() {
            (*lr.start())..=(*rr.end())
        } else if rr.end() + 1 == *lr.start() {
            (*rr.start())..=(*lr.end())
        } else {
            return Err(Error::InvalidRange);
        };

        Ok(r)
    }
}

impl<T: Clone> Merge for ValueMerge<T> {
    type Item = Value<T>;

    fn merge(left: &Self::Item, right: &Self::Item) -> Result<Self::Item> {
        Ok(Value::Merged(
            Box::new(left.clone()),
            Box::new(right.clone()),
            Self::merged_range(left, right)?,
        ))
    }
}

#[derive(Clone, Debug)]
pub enum Command<T> {
    // Push the next leaf to stack
    NextLeaf,
    // Push proof data to stack
    Proof(T),
    // Hash 2 leafs from the top of stack, the actual hashing order is deduced
    // via node positions
    Hash,
    // Hash bottom with top treating both as peaks, the hashing order is always
    // H(top | bottom)
    HashPeak,
    // Convert a hash to a peak hash. When a leaf value needs proving
    // in a peak, the peak position value will be available. The command
    // runner also checks peak position against the designated position
    // calculated via mmr_size.
    // When no leaf values in a peak needs proving, the peak will be
    // represented via a proof directly, the peak position will be missing
    // here, we will skip the checking then, which is the same as current
    // behavior here:
    // https://github.com/nervosnetwork/merkle-mountain-range/blob/7f47ea585f6ecda81be986b619a7a8fd6ddcafc2/src/mmr.rs#L378-L380
    ToPeak,
}

#[derive(Clone, Debug)]
pub struct CompiledMerkleProof<T>(Vec<Command<T>>);

impl<T: PartialEq + Clone> CompiledMerkleProof<T> {
    pub fn verify<M: Merge<Item = T>>(
        self,
        root: T,
        mmr_size: u64,
        leaves: Vec<(u64, T)>,
    ) -> Result<bool> {
        verify::<_, M, _, _>(
            &mut self.0.into_iter().map(Ok),
            root,
            mmr_size,
            &mut leaves.into_iter().map(Ok),
        )
    }
}

pub fn compile_merkle_proof<T: PartialEq + Clone, M: Merge<Item = Value<T>>>(
    mmr_size: u64,
    proof: Vec<T>,
    pos_list: Vec<u64>,
) -> Result<CompiledMerkleProof<T>> {
    // ensure positions are sorted and unique
    if pos_list.windows(2).any(|slice| slice[0] >= slice[1]) {
        return Err(Error::LeavesUnsorted);
    }

    let proof: Vec<Value<T>> = proof.into_iter().map(|proof| Value::Proof(proof)).collect();
    let leaves = pos_list
        .into_iter()
        .enumerate()
        .map(|(i, pos)| (pos, Value::LeafIndex(i)))
        .collect();

    let peaks = calculate_peaks_hashes::<_, M, _>(leaves, mmr_size, proof.iter())?;
    if peaks.windows(2).any(|s| {
        let lhs_range = s[0].leaf_range();
        let rhs_range = s[1].leaf_range();
        (!lhs_range.is_empty())
            && (!rhs_range.is_empty())
            && lhs_range.end() + 1 != *rhs_range.start()
    }) {
        return Err(Error::InvalidRange);
    }

    let mut commands = Vec::new();

    for peak in &peaks {
        emit_value_command(&mut commands, peak)?;
        commands.push(Command::ToPeak);
    }

    // bagging_peaks_hashes scans peaks from right to left, here the stack
    // keeps rightmost item at top of stack, so all we need to do is a couple
    // of hashes
    for _ in 1..peaks.len() {
        commands.push(Command::HashPeak);
    }

    Ok(CompiledMerkleProof(commands))
}

fn emit_value_command<T: Clone>(commands: &mut Vec<Command<T>>, value: &Value<T>) -> Result<()> {
    match value {
        Value::Merged(lhs, rhs, _) => {
            if lhs.leaf_range().start() < rhs.leaf_range().start() {
                emit_value_command(commands, &**lhs)?;
                emit_value_command(commands, &**rhs)?;
                commands.push(Command::Hash);
            } else {
                emit_value_command(commands, &**rhs)?;
                emit_value_command(commands, &**lhs)?;
                commands.push(Command::Hash);
            }
        }
        Value::LeafIndex(_) => commands.push(Command::NextLeaf),
        Value::Proof(proof) => commands.push(Command::Proof(proof.clone())),
    }
    Ok(())
}

pub fn pack_compiled_merkle_proof<T: Packable>(proof: &CompiledMerkleProof<T>) -> Result<Vec<u8>> {
    let mut ret = Vec::new();
    for command in &proof.0 {
        match command {
            Command::NextLeaf => {
                ret.push(1u8);
            }
            Command::Proof(proof) => {
                let proof_bytes: Vec<u8> = proof.pack()?;
                ret.push(2u8);
                ret.extend(&proof_bytes);
            }
            Command::Hash => {
                ret.push(3u8);
            }
            Command::HashPeak => {
                ret.push(4u8);
            }
            Command::ToPeak => {
                ret.push(5u8);
            }
        }
    }

    Ok(ret)
}

pub fn pack_leaves<T: Packable>(leaves: &[(u64, T)]) -> Result<Vec<u8>> {
    let mut ret = Vec::new();
    for (pos, item) in leaves {
        ret.extend(&pos.to_le_bytes()[..]);
        ret.extend(item.pack()?);
    }
    Ok(ret)
}

pub struct PackedMerkleProof<'a, T> {
    index: usize,
    data: &'a [u8],
    merge: PhantomData<T>,
}

impl<'a, T> PackedMerkleProof<'a, T> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            index: 0,
            data,
            merge: PhantomData,
        }
    }
}

impl<'a, T: Packable> Iterator for PackedMerkleProof<'a, T> {
    type Item = Result<Command<T>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.data.len() {
            return None;
        }

        let command = self.data[self.index];
        self.index += 1;

        match command {
            1u8 => Some(Ok(Command::NextLeaf)),
            2u8 => match T::unpack(&self.data[self.index..]) {
                Ok((proof, size)) => {
                    self.index += size;
                    Some(Ok(Command::Proof(proof)))
                }
                Err(e) => Some(Err(e)),
            },
            3u8 => Some(Ok(Command::Hash)),
            4u8 => Some(Ok(Command::HashPeak)),
            5u8 => Some(Ok(Command::ToPeak)),
            _ => Some(Err(Error::InvalidCommand(command))),
        }
    }
}

pub struct PackedLeaves<'a, T> {
    index: usize,
    data: &'a [u8],
    t: PhantomData<T>,
}

impl<'a, T> PackedLeaves<'a, T> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            index: 0,
            data,
            t: PhantomData,
        }
    }
}

impl<'a, T: Packable> Iterator for PackedLeaves<'a, T> {
    type Item = Result<(u64, T)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.data.len() {
            return None;
        }

        if self.data.len() - self.index < 8 {
            return Some(Err(Error::UnpackEof));
        }

        let pos = {
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&self.data[self.index..self.index + 8]);
            u64::from_le_bytes(buf)
        };
        self.index += 8;

        match T::unpack(&self.data[self.index..]) {
            Ok((item, size)) => {
                self.index += size;
                Some(Ok((pos, item)))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum StackItemType {
    Node(u64, u8),
    Peak,
    Proof,
}

pub fn calculate_root<
    T,
    M: Merge<Item = T>,
    I: Iterator<Item = Result<Command<T>>>,
    IT: Iterator<Item = Result<(u64, T)>>,
>(
    commands: &mut I,
    mmr_size: u64,
    leaves: &mut IT,
) -> Result<T> {
    let mut last_leaf_pos = None;

    let mut next_peak_info = PeakIterator::new(mmr_size);
    // let mut leaf_index = 0;
    let mut stack: Vec<(T, StackItemType)> = Vec::with_capacity(257);
    for command in commands {
        match command? {
            Command::NextLeaf => {
                let (pos, leaf_item) = leaves.next().ok_or(Error::CorruptedStack)??;
                if pos_height_in_tree(pos) > 0 {
                    return Err(Error::NodeProofsNotSupported);
                }
                if let Some(last_leaf_pos) = last_leaf_pos {
                    // ensure leaves are sorted and unique
                    if last_leaf_pos >= pos {
                        return Err(Error::CorruptedProof);
                    }
                }
                last_leaf_pos = Some(pos);
                stack.push((leaf_item, StackItemType::Node(pos, 0)));
            }
            Command::Proof(proof) => {
                stack.push((proof, StackItemType::Proof));
            }
            Command::Hash => {
                if stack.len() < 2 {
                    return Err(Error::CorruptedStack);
                }
                let (rhs, rhs_t) = stack.pop().unwrap();
                let (lhs, lhs_t) = stack.pop().unwrap();
                let (pos, height, next_pos, next_t, item, sibling_item) = match (lhs_t, &rhs_t) {
                    (StackItemType::Proof, StackItemType::Node(rhs_pos, rhs_height)) => (
                        *rhs_pos,
                        *rhs_height,
                        u64::MAX,
                        StackItemType::Proof,
                        rhs,
                        lhs,
                    ),
                    (StackItemType::Node(lhs_pos, lhs_height), StackItemType::Node(rhs_pos, _)) => {
                        (lhs_pos, lhs_height, *rhs_pos, rhs_t, lhs, rhs)
                    }
                    (StackItemType::Node(lhs_pos, lhs_height), StackItemType::Proof) => {
                        (lhs_pos, lhs_height, u64::MAX, rhs_t, lhs, rhs)
                    }
                    _ => return Err(Error::CorruptedProof),
                };
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
                if sib_pos != next_pos && next_t != StackItemType::Proof {
                    return Err(Error::CorruptedProof);
                }
                let parent_item = if next_height > height {
                    M::merge(&sibling_item, &item)
                } else {
                    M::merge(&item, &sibling_item)
                }?;
                stack.push((parent_item, StackItemType::Node(parent_pos, height + 1)));
            }
            Command::ToPeak => {
                if stack.is_empty() {
                    return Err(Error::CorruptedStack);
                }
                let (item, t) = stack.pop().unwrap();
                if let StackItemType::Node(pos, _height) = t {
                    // For peak that contains at least 1 leaf node, we will
                    // look for a matching peak deduced from mmr_size, and
                    // abort if such a peak does not exist.
                    if !next_peak_info.any(|peak_pos| peak_pos == pos) {
                        return Err(Error::CorruptedProof);
                    }
                }
                stack.push((item, StackItemType::Peak));
            }
            Command::HashPeak => {
                if stack.len() < 2 {
                    return Err(Error::CorruptedStack);
                }
                let top = stack.pop().unwrap();
                let bottom = stack.pop().unwrap();
                if top.1 != StackItemType::Peak || bottom.1 != StackItemType::Peak {
                    return Err(Error::CorruptedProof);
                }
                stack.push((M::merge_peaks(&top.0, &bottom.0)?, StackItemType::Peak));
            }
        }
    }
    if stack.len() != 1 {
        return Err(Error::CorruptedProof);
    }
    if leaves.next().is_some() {
        return Err(Error::CorruptedProof);
    }
    let (root, _) = stack.pop().unwrap();
    Ok(root)
}

pub fn verify<
    T: PartialEq,
    M: Merge<Item = T>,
    I: Iterator<Item = Result<Command<T>>>,
    IT: Iterator<Item = Result<(u64, T)>>,
>(
    commands: &mut I,
    root: T,
    mmr_size: u64,
    leaves: &mut IT,
) -> Result<bool> {
    calculate_root::<_, M, _, _>(commands, mmr_size, leaves)
        .map(|calculated_root| calculated_root == root)
}
