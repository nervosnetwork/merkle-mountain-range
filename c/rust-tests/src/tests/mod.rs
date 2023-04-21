use blake2b_rs::{Blake2b, Blake2bBuilder};
use ckb_merkle_mountain_range::{compiled_proof::Packable, Error, Merge, Result};

mod random;

#[link(name = "mmr_c")]
extern "C" {
    pub fn run_mmr_verify(
        root: *const u8,
        root_length: u32,
        mmr_size: u64,
        proof: *const u8,
        proof_length: u32,
        leaves: *const u8,
        leaves_length: u32,
    ) -> i32;
}

#[derive(Clone, Debug, PartialEq)]
pub struct VariableBytes(Vec<u8>);

impl Packable for VariableBytes {
    fn pack(&self) -> Result<Vec<u8>> {
        if self.0.len() > u16::MAX as usize {
            return Err(Error::UnpackEof);
        }
        let mut ret = Vec::new();
        ret.resize(self.0.len() + 2, 0);
        ret[0..2].copy_from_slice(&(self.0.len() as u16).to_le_bytes());
        ret[2..].copy_from_slice(&self.0);
        Ok(ret)
    }

    fn unpack(data: &[u8]) -> Result<(Self, usize)> {
        if data.len() < 2 {
            return Err(Error::UnpackEof);
        }
        let len = {
            let mut buf = [0u8; 2];
            buf.copy_from_slice(&data[0..2]);
            u16::from_le_bytes(buf)
        } as usize;
        if data.len() < 2 + len {
            return Err(Error::UnpackEof);
        }
        let mut r = Vec::new();
        r.resize(len, 0);
        r.copy_from_slice(&data[2..2 + len]);
        Ok((VariableBytes(r), 2 + len))
    }
}

fn new_blake2b() -> Blake2b {
    Blake2bBuilder::new(32)
        .personal(b"ckb-default-hash")
        .build()
}

#[derive(Debug)]
struct Blake2bHash;

impl Merge for Blake2bHash {
    type Item = VariableBytes;

    fn merge(lhs: &Self::Item, rhs: &Self::Item) -> Result<Self::Item> {
        let mut hasher = new_blake2b();
        hasher.update(&lhs.0[..]);
        hasher.update(&rhs.0[..]);
        let mut hash = Vec::new();
        hash.resize(32, 0);
        hasher.finalize(&mut hash);
        Ok(VariableBytes(hash))
    }
}
