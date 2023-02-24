#include "ckb_mmr.h"

int run_mmr_verify(const uint8_t *root, uint32_t root_length, uint64_t mmr_size,
                   const uint8_t *proof, uint32_t proof_length,
                   const uint8_t *leaves, uint32_t leaves_length) {
  mmr_default_buffer_reader_t proof_buffer_reader;
  mmr_default_buffer_reader_init(&proof_buffer_reader, proof, proof_length);

  mmr_default_buffer_reader_t leaf_buffer_reader;
  mmr_default_buffer_reader_init(&leaf_buffer_reader, leaves, leaves_length);

  return mmr_verify(root, root_length, mmr_size, &proof_buffer_reader,
                    &leaf_buffer_reader);
}
