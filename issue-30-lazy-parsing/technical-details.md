# Lazy Parsing: Technical Implementation Details

**NOTE: This document describes the lazy parsing implementation that was developed and subsequently reverted. The code described here no longer exists in the codebase. This is preserved for historical reference.**

## Architecture Overview

### Limbo's Implementation

```rust
// In BTreeCursor (core/storage/btree.rs)
pub struct BTreeCursor {
    // Lazy parsing state
    pub(crate) column_types: Vec<Option<SerialType>>,
    pub(crate) column_offsets: Vec<Option<usize>>,
    pub(crate) cached_values: Vec<Option<RefValue>>,  // ⚠️ Problem: Caches actual values
    pub(crate) cached_payload: Option<Vec<u8>>,       // ⚠️ Problem: Copies entire payload
    pub(crate) last_accessed_column: Option<usize>,
    pub(crate) is_sequential_access: bool,
    pub(crate) columns_parsed: usize,
}
```

### SQLite's Implementation

```c
// In VdbeCursor (SQLite's vdbeInt.h)
struct VdbeCursor {
    u32 nHdrParsed;      // Number of header fields parsed
    u32 iHdrOffset;      // Offset to next unparsed header byte
    u32 *aOffset;        // Offset to each column value
    u32 aType[];         // Type of each column (MUST BE LAST)
    // Note: NO value caching, NO payload copy
}
```

## Performance Bottlenecks

### 1. Value Caching (Biggest Issue)
```rust
// Limbo caches parsed values
if should_cache {
    self.cached_values[column_idx] = Some(value.clone());
}

// Problem: Adds overhead without benefit
// - Clone operations
// - Memory allocations
// - Cache management complexity
```

### 2. Payload Copying
```rust
// Limbo copies entire payload
self.cached_payload = Some(payload.to_vec());

// Problem: Defeats zero-copy design
// - Allocates memory for every record
// - Double memory usage
// - Cache pollution
```

### 3. Early Value Materialization
```rust
// In Column opcode (execute.rs)
let value = cursor.get_column_lazy(column)?;
*reg = Register::Value(value.to_owned()); // ⚠️ Allocates!

// Problem: Forces allocation even for pass-through values
```

## Benchmark Analysis

### Test Setup
- Tables with 10, 50, 100, 200 columns
- Sequential access (SELECT *)
- Selective access (SELECT 3 columns)
- Sparse access (columns 1, 50, 99)

### Results Breakdown

#### 10-Column Table (Most Common Case)
```
Baseline eager: 17.9µs (beats SQLite's 22.1µs)
With lazy:      29.0µs (+62% regression)

Overhead breakdown:
- Payload copy:      ~5µs (28%)
- Value caching:     ~3µs (17%)
- State management:  ~2µs (11%)
- Access checks:     ~1µs (6%)
```

#### Why No Benefit on Selective Queries
```
Selective query: SELECT col1, col2, col3 FROM table_50

Expected: Parse only 3 columns
Actual:   Parse 3 columns + overhead of:
          - Payload copy (all 50 columns worth)
          - Cache management
          - State tracking
          
Result: Overhead > Savings
```

## Optimization Opportunities

### 1. Eager Parsing Improvements

```rust
// Current eager parsing
pub fn read_record(payload: &[u8], reuse: &mut ImmutableRecord) {
    reuse.start_serialization(payload); // ⚠️ Copies payload
    // Parse all columns...
}

// Optimized eager parsing
pub fn read_record_zero_copy<'a>(
    payload: &'a [u8], 
    reuse: &mut ImmutableRecord<'a>
) {
    reuse.set_payload_ref(payload); // No copy!
    // Parse with SIMD...
}
```

### 2. Projection-Based Parsing

```rust
// Parse only needed columns
pub fn parse_projected(
    payload: &[u8],
    needed_columns: &BitVec,
    record: &mut ImmutableRecord
) {
    let header = parse_header_simd(payload);
    
    for (idx, needed) in needed_columns.iter().enumerate() {
        if needed {
            record.values[idx] = parse_value_at(payload, header[idx]);
        }
    }
}
```

### 3. SIMD Varint Parsing

```rust
// Parse multiple varints in parallel
#[target_feature(enable = "avx2")]
unsafe fn parse_varints_avx2(data: &[u8]) -> Vec<u64> {
    // Load 32 bytes
    let chunk = _mm256_loadu_si256(data.as_ptr() as *const __m256i);
    
    // Find continuation bits
    let cont_mask = _mm256_cmpgt_epi8(chunk, _mm256_set1_epi8(0x7F));
    
    // Extract varint boundaries
    // ... (complex but 3-4x faster)
}
```

## Memory Layout Considerations

### Current Layout (Inefficient)
```
BTreeCursor:
├── Metadata (inline)
├── Vec<Option<SerialType>>    → Heap allocation
├── Vec<Option<usize>>         → Heap allocation  
├── Vec<Option<RefValue>>      → Heap allocation
└── Option<Vec<u8>>            → Heap allocation (payload copy)
```

### Optimized Layout
```
BTreeCursor:
├── Metadata (inline)
├── SmallVec<[SerialType; 32]> → Stack for common case
├── SmallVec<[usize; 32]>      → Stack for common case
└── &'a [u8]                   → Reference to page buffer
```

## Why SQLite's Approach Works

1. **Minimal State**: Just `nHdrParsed` and arrays
2. **No Allocations**: Works directly with page buffer
3. **Simple Logic**: Parse up to needed column, stop
4. **Low Overhead**: State check is single comparison

## Why Limbo's Approach Fails

1. **Too Much State**: Multiple vectors, flags, counters
2. **Allocations**: Payload copy + value caching
3. **Complex Logic**: Sequential detection, cache decisions
4. **High Overhead**: Multiple checks per column access

## Conclusion

The lazy parsing implementation is architecturally sound but practically flawed. The overhead of managing lazy state exceeds the cost of parsing, especially given Limbo's efficient baseline. The path forward is either:

1. **Remove it**: Focus on eager parsing optimization
2. **Simplify it**: Match SQLite's minimal approach
3. **Replace it**: With projection-based parsing

Given Limbo's performance characteristics, option 1 or 3 are most promising.