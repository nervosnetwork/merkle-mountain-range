#ifndef _CKB_MERKLE_MOUNTAIN_RANGE_H_
#define _CKB_MERKLE_MOUNTAIN_RANGE_H_

#include <blake2b.h>
#include <stdint.h>

#ifndef MMR_STACK_SIZE
#define MMR_STACK_SIZE 257
#endif

#ifndef MMR_NODE_BUFFER_MAX_BYTES
#define MMR_NODE_BUFFER_MAX_BYTES 32
#endif

#ifndef MMR_MEMCMP
#define MMR_MEMCMP memcmp
#endif

#ifndef MMR_MERGE
#define MMR_MERGE mmr_blake2b_merge
#endif

#ifndef MMR_MERGE_PEAKS
#define MMR_MERGE_PEAKS mmr_blake2b_merge
#endif

// Note that we actually have tried the following structure:
//
// ```
// typedef struct {
//   void *context;
//   mmr_leaf_reader_f reader;
// } mmr_leaf_iter_t;
//
// typedef struct {
//   void *context;
//   mmr_command_reader_f command_reader;
//   mmr_proof_reader_f proof_reader;
// } mmr_proof_iter_t;
//
// int mmr_verify(const uint8_t *root, uint32_t root_length, uint64_t mmr_size,
//                mmr_proof_iter_t *proof_iter, mmr_leaf_iter_t *leaf_iter);
// ```
//
// But C compiler won't inline function pointers, which will incur more
// cycles. Using macros here can help reduce cycle consumptions at a noticeable
// scale.
//
// We can think of those macros as poor man's generics in C.
#ifndef MMR_LEAF_READER
#define MMR_LEAF_READER _mmr_default_leaf_iter_read
#endif

#ifndef MMR_COMMAND_READER
#define MMR_COMMAND_READER _mmr_default_command_read
#endif

#ifndef MMR_PROOF_READER
#define MMR_PROOF_READER _mmr_default_buffer_read_node
#endif

enum MMRErrorCode {
  // MMR
  ERROR_INVALID_STACK = 80,
  ERROR_INVALID_COMMAND,
  ERROR_INVALID_PROOF,
  ERROR_PROOF_EOF,
  ERROR_LEAF_EOF,
  ERROR_NO_MORE_LEAFS,
  ERROR_NO_MORE_COMMANDS,
  ERROR_NODE_EOF,
};

#define MMR_NODE_TYPE_BUFFER 1
#define MMR_NODE_TYPE_POINTER 2

typedef struct {
  uint8_t t;
  union {
    uint8_t buffer[MMR_NODE_BUFFER_MAX_BYTES];
    const uint8_t *pointer;
  } value;
  uint32_t length;
} mmr_node_t;

typedef int (*mmr_leaf_reader_f)(void *, mmr_node_t *, uint64_t *);
typedef int (*mmr_command_reader_f)(void *, uint8_t *);
typedef int (*mmr_proof_reader_f)(void *, mmr_node_t *);

// In the default case, one should pass the pointer to
// mmr_default_buffer_reader_t data structure as reader contexts. A rough sample
// code look as follows:
//
// ```
// // First, load root_buffer, mmr_size, proof_buffer and leaf_buffer
// // using syscalls.
//
// mmr_default_buffer_reader_t proof_buffer_reader;
// mmr_default_buffer_reader_init(&proof_buffer_reader, proof_buffer,
//                                proof_length);
//
// mmr_default_buffer_reader_t leaf_buffer_reader;
// mmr_default_buffer_reader_init(&leaf_buffer_reader, leaf_buffer,
//                                leaves_length);
//
// return mmr_verify(root_buffer, 32, mmr_size, &proof_buffer_reader,
//                   &leaf_buffer_reader);
// ```
int mmr_verify(const uint8_t *root, uint32_t root_length, uint64_t mmr_size,
               void *proof_reader_context, void *leaf_reader_context);

const uint8_t *mmr_node_value(const mmr_node_t *node) {
  if (node->t == MMR_NODE_TYPE_POINTER) {
    return node->value.pointer;
  }
  return node->value.buffer;
}

// Note dst might collide with either lhs or rhs to save memcpy operation
int mmr_blake2b_merge(mmr_node_t *dst, const mmr_node_t *lhs,
                      const mmr_node_t *rhs) {
  blake2b_state ctx;
  ckb_blake2b_init(&ctx, 32);
  blake2b_update(&ctx, mmr_node_value(lhs), lhs->length);
  blake2b_update(&ctx, mmr_node_value(rhs), rhs->length);
  blake2b_final(&ctx, dst->value.buffer, 32);
  dst->t = MMR_NODE_TYPE_BUFFER;
  dst->length = 32;
  return 0;
}

typedef struct {
  const uint8_t *buffer;
  uint32_t buffer_length;
  uint32_t index;
} mmr_default_buffer_reader_t;

int mmr_default_buffer_reader_init(mmr_default_buffer_reader_t *context,
                                   const uint8_t *buffer,
                                   uint32_t buffer_length) {
  context->buffer = buffer;
  context->buffer_length = buffer_length;
  context->index = 0;
  return 0;
}

int _mmr_default_buffer_read_node(void *context, mmr_node_t *out) {
  mmr_default_buffer_reader_t *c = (mmr_default_buffer_reader_t *)context;

  if (c->buffer_length - c->index < 2) {
    return ERROR_NODE_EOF;
  }
  uint16_t len = *((uint16_t *)(&c->buffer[c->index]));
  if (c->buffer_length - c->index - ((uint32_t)2) < len) {
    return ERROR_NODE_EOF;
  }
  out->value.pointer = &c->buffer[c->index + 2];
  out->t = MMR_NODE_TYPE_POINTER;
  out->length = len;
  c->index += 2 + len;
  return 0;
}

inline int _mmr_default_command_read(void *context, uint8_t *out_command) {
  mmr_default_buffer_reader_t *c = (mmr_default_buffer_reader_t *)context;

  if (c->index >= c->buffer_length) {
    return ERROR_NO_MORE_COMMANDS;
  }
  *out_command = c->buffer[c->index++];
  return 0;
}

int _mmr_default_leaf_iter_read(void *context, mmr_node_t *out_node,
                                uint64_t *out_position) {
  mmr_default_buffer_reader_t *c = (mmr_default_buffer_reader_t *)context;

  if (c->index >= c->buffer_length) {
    return ERROR_NO_MORE_LEAFS;
  }
  if (c->buffer_length - c->index < 8) {
    return ERROR_LEAF_EOF;
  }
  uint64_t position = *((uint64_t *)(&c->buffer[c->index]));
  c->index += 8;

  int ret = _mmr_default_buffer_read_node(c, out_node);
  if (ret != 0) {
    return ret;
  }
  *out_position = position;
  return 0;
}

#ifdef __CKB_FORCE_RISCV_B__
uint64_t _mmr_trailing_zeros_u64(uint64_t value) {
  uint64_t ret;
  asm("ctz %0,%1" : "=r"(ret) : "r"(value));
  return ret;
}

uint64_t _mmr_leading_zeros_u64(uint64_t value) {
  uint64_t ret;
  asm("clz %0,%1" : "=r"(ret) : "r"(value));
  return ret;
}

uint64_t _mmr_count_zeros_u64(uint64_t value) {
  uint64_t ret;
  asm("cpop %0,%1" : "=r"(ret) : "r"(value));
  return 64 - ret;
}
#elif defined __has_builtin
uint64_t _mmr_trailing_zeros_u64(uint64_t value) {
  return __builtin_ctzl(value);
}

uint64_t _mmr_leading_zeros_u64(uint64_t value) {
  return __builtin_clzl(value);
}

uint64_t _mmr_count_zeros_u64(uint64_t value) {
  return 64 - __builtin_popcountl(value);
}
#else
#error "No implementation available for ctz, clz and cpop!"
#endif

uint64_t _mmr_parent_offset(uint32_t height) { return 2 << height; }

uint64_t _mmr_sibling_offset(uint32_t height) { return (2 << height) - 1; }

int _mmr_all_ones(uint64_t num) {
  return num != 0 && _mmr_count_zeros_u64(num) == _mmr_leading_zeros_u64(num);
}

uint64_t _mmr_jump_left(uint64_t pos) {
  uint64_t bit_length = 64 - _mmr_leading_zeros_u64(pos);
  uint64_t most_significant_bits = ((uint64_t)1) << (bit_length - 1);
  return pos - (most_significant_bits - 1);
}

uint32_t _mmr_pos_height_in_tree(uint64_t pos) {
  pos += 1;

  while (!_mmr_all_ones(pos)) {
    pos = _mmr_jump_left(pos);
  }

  return 64 - _mmr_leading_zeros_u64(pos) - 1;
}

typedef struct {
  uint64_t pos;
  uint32_t height;
  int present;
} _mmr_peak_t;

uint64_t _mmr_get_peak_pos_by_height(uint32_t height) {
  return (((uint64_t)1) << (height + 1)) - 2;
}

_mmr_peak_t _mmr_left_peak_height_pos(uint64_t mmr_size) {
  uint32_t height = 1;
  uint64_t prev_pos = 0;
  uint64_t pos = _mmr_get_peak_pos_by_height(height);
  while (pos < mmr_size) {
    height += 1;
    prev_pos = pos;
    pos = _mmr_get_peak_pos_by_height(height);
  }

  _mmr_peak_t ret;
  ret.height = height - 1;
  ret.pos = prev_pos;
  ret.present = 1;
  return ret;
}

int _mmr_get_right_peak(_mmr_peak_t *peak, uint64_t mmr_size) {
  uint32_t height = peak->height;
  uint64_t pos = peak->pos;

  pos += _mmr_sibling_offset(height);
  while (pos > mmr_size - 1) {
    if (height == 0) {
      peak->present = 0;
      return 0;
    }
    pos -= _mmr_parent_offset(height - 1);
    height -= 1;
  }
  peak->height = height;
  peak->pos = pos;
  peak->present = 1;
  return 1;
}

typedef struct {
  uint8_t t;
  mmr_node_t node;
  uint64_t position;
  uint32_t height;
} _mmr_stack_value_t;

#define _MMR_STACK_NODE 1
#define _MMR_STACK_PROOF 2
#define _MMR_STACK_PEAK 3

int mmr_verify(const uint8_t *root, uint32_t root_length, uint64_t mmr_size,
               void *proof_reader_context, void *leaf_reader_context) {
  _mmr_stack_value_t stack[MMR_STACK_SIZE];

  uint32_t stack_top = 0;
  _mmr_peak_t next_peak_info = _mmr_left_peak_height_pos(mmr_size);
  uint64_t last_leaf_pos = 0;
  int has_last_leaf = 0;
  uint8_t command = 0xFF;

  // We won't bother doing anything when mmr_size is 0.
  if (mmr_size == 0) {
    return ERROR_INVALID_PROOF;
  }

  while (1) {
    int ret = MMR_COMMAND_READER(proof_reader_context, &command);
    if (ret == ERROR_NO_MORE_COMMANDS) {
      break;
    }
    if (ret != 0) {
      return ret;
    }
    switch (command) {
    case 1: {
      if (stack_top >= MMR_STACK_SIZE) {
        return ERROR_INVALID_STACK;
      }
      int leaf_success =
          MMR_LEAF_READER(leaf_reader_context, &stack[stack_top].node,
                          &stack[stack_top].position);
      if (leaf_success != 0) {
        return leaf_success;
      }
      if (has_last_leaf) {
        if (last_leaf_pos >= stack[stack_top].position) {
          return ERROR_INVALID_PROOF;
        }
      }
      if (stack[stack_top].position >= mmr_size) {
        return ERROR_INVALID_PROOF;
      }
      if (_mmr_pos_height_in_tree(stack[stack_top].position) > 0) {
        return ERROR_INVALID_PROOF;
      }
      stack[stack_top].t = _MMR_STACK_NODE;
      last_leaf_pos = stack[stack_top].position;
      has_last_leaf = 1;
      stack[stack_top].height = 0;
      stack_top++;
    } break;
    case 2: {
      if (stack_top >= MMR_STACK_SIZE) {
        return ERROR_INVALID_STACK;
      }
      int ret = MMR_PROOF_READER(proof_reader_context, &stack[stack_top].node);
      if (ret != 0) {
        return ret;
      }
      stack[stack_top].t = _MMR_STACK_PROOF;
      stack[stack_top].position = 0;
      stack[stack_top].height = 0;
      stack_top++;
    } break;
    case 3: {
      if (stack_top < 2) {
        return ERROR_INVALID_STACK;
      }
      uint32_t lhs_index = stack_top - 2;
      uint32_t rhs_index = stack_top - 1;
      uint64_t pos, next_pos;
      uint8_t next_t;
      uint32_t height;
      const mmr_node_t *item;
      const mmr_node_t *sibling_item;
      if (stack[lhs_index].t == _MMR_STACK_PROOF) {
        pos = stack[rhs_index].position;
        height = stack[rhs_index].height;
        next_pos = stack[lhs_index].position;
        next_t = stack[lhs_index].t;
        item = &stack[rhs_index].node;
        sibling_item = &stack[lhs_index].node;
      } else {
        pos = stack[lhs_index].position;
        height = stack[lhs_index].height;
        next_pos = stack[rhs_index].position;
        next_t = stack[rhs_index].t;
        item = &stack[lhs_index].node;
        sibling_item = &stack[rhs_index].node;
      }
      uint32_t next_height = _mmr_pos_height_in_tree(pos + 1);
      uint64_t sibling_offset = _mmr_sibling_offset(height);
      uint64_t sib_pos, parent_pos;
      if (next_height > height) {
        sib_pos = pos - sibling_offset;
        parent_pos = pos + 1;
      } else {
        sib_pos = pos + sibling_offset;
        parent_pos = pos + _mmr_parent_offset(height);
      }
      if ((sib_pos != next_pos) && (next_t != _MMR_STACK_PROOF)) {
        return ERROR_INVALID_PROOF;
      }
      if (next_height > height) {
        MMR_MERGE(&stack[stack_top - 2].node, sibling_item, item);
      } else {
        MMR_MERGE(&stack[stack_top - 2].node, item, sibling_item);
      }
      stack[stack_top - 2].position = parent_pos;
      stack[stack_top - 2].height = height + 1;
      stack[stack_top - 2].t = _MMR_STACK_NODE;
      stack_top--;
    } break;
    case 4: {
      if (stack_top < 2) {
        return ERROR_INVALID_STACK;
      }
      uint32_t top_index = stack_top - 1;
      uint32_t bottom_index = stack_top - 2;
      if ((stack[top_index].t != _MMR_STACK_PEAK) ||
          (stack[bottom_index].t != _MMR_STACK_PEAK)) {
        return ERROR_INVALID_PROOF;
      }
      MMR_MERGE_PEAKS(&stack[stack_top - 2].node, &stack[top_index].node,
                      &stack[bottom_index].node);
      stack[stack_top - 2].height = 0;
      stack_top--;
    } break;
    case 5: {
      if (stack_top < 1) {
        return ERROR_INVALID_STACK;
      }
      uint64_t pos = stack[stack_top - 1].position;
      if (stack[stack_top - 1].t != _MMR_STACK_PROOF) {
        while ((next_peak_info.present != 0) && (pos != next_peak_info.pos)) {
          _mmr_get_right_peak(&next_peak_info, mmr_size);
        }
        if (next_peak_info.present == 0) {
          return ERROR_INVALID_PROOF;
        }
        _mmr_get_right_peak(&next_peak_info, mmr_size);
      }
      stack[stack_top - 1].t = _MMR_STACK_PEAK;
    } break;
    default:
      return ERROR_INVALID_COMMAND;
    }
  }

  if (stack_top != 1) {
    return ERROR_INVALID_PROOF;
  }
  mmr_node_t dummy;
  uint64_t dummy_length;
  if (MMR_LEAF_READER(leaf_reader_context, &dummy, &dummy_length) == 0) {
    return ERROR_INVALID_PROOF;
  }
  if ((root_length != stack[0].node.length) ||
      (MMR_MEMCMP(root, mmr_node_value(&stack[0].node), root_length) != 0)) {
    return ERROR_INVALID_PROOF;
  }
  return 0;
}

#endif /* _CKB_MERKLE_MOUNTAIN_RANGE_H_ */
