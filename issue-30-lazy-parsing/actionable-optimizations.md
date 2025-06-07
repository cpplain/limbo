# Actionable Optimizations for Limbo Wide Table Performance

Based on SQLite source code analysis, here are concrete optimizations Limbo can implement:

## 1. Add Column Header Caching to BTreeCursor

### Current State
Limbo re-parses the entire record header for each column access.

### Proposed Change
Add header caching similar to SQLite's approach:

```rust
// In BTreeCursor
pub struct BTreeCursor {
    // ... existing fields ...
    
    // New fields for header caching
    header_cache: Option<HeaderCache>,
}

pub struct HeaderCache {
    serial_types: Vec<u8>,      // Cached serial type for each column
    offsets: Vec<u32>,          // Cached offset to each column's data
    parsed_up_to: usize,        // Number of columns parsed so far
    cache_generation: u64,      // To invalidate on cursor movement
}
```

### Implementation Sketch

```rust
impl BTreeCursor {
    pub fn get_column_with_cache(&mut self, column: usize) -> Result<RefValue> {
        // Check if we need to parse more headers
        if self.header_cache.is_none() || 
           self.header_cache.as_ref().unwrap().parsed_up_to <= column {
            self.parse_headers_up_to(column)?;
        }
        
        let cache = self.header_cache.as_ref().unwrap();
        let offset = cache.offsets[column];
        let serial_type = cache.serial_types[column];
        
        // Jump directly to column data
        let data = &self.payload[offset..];
        deserialize_value(serial_type, data)
    }
}
```

## 2. Optimize the Column Instruction Path

### Current Bottleneck
Each `Column` instruction goes through multiple abstraction layers.

### Proposed Optimization
Create a fast path for common cases:

```rust
pub fn op_column_fast(
    state: &mut ProgramState,
    cursor_id: usize,
    column: usize,
    dest: usize,
) -> Result<()> {
    // Fast path: avoid match statements and indirection
    let cursor = unsafe { 
        // SAFETY: bounds checked during compilation
        state.cursors.get_unchecked_mut(cursor_id)
    };
    
    if let Some(btree) = cursor.as_btree_mut_fast() {
        // Direct access without going through trait objects
        let value = btree.get_column_cached(column)?;
        
        // Reuse existing register allocation if possible
        match (&value, &mut state.registers[dest]) {
            (RefValue::Text(ref_text), Register::Value(Value::Text(reg_text))) => {
                // Reuse allocation
                reg_text.value.clear();
                reg_text.value.extend_from_slice(ref_text.value);
            }
            _ => {
                state.registers[dest] = Register::Value(value.to_owned());
            }
        }
    }
    Ok(())
}
```

## 3. Batch Column Access Pattern Recognition

### Observation
Queries often access columns sequentially (e.g., SELECT a, b, c).

### Optimization
Detect and optimize sequential column access:

```rust
// During code generation
fn optimize_column_sequence(instructions: &mut Vec<Insn>) {
    // Look for sequences like:
    // Column(cursor: 0, column: 0, dest: 1)
    // Column(cursor: 0, column: 1, dest: 2)
    // Column(cursor: 0, column: 2, dest: 3)
    
    // Replace with:
    // ColumnRange(cursor: 0, start_col: 0, count: 3, start_dest: 1)
}

// New instruction that parses headers once and extracts multiple columns
pub fn op_column_range(
    state: &mut ProgramState,
    cursor_id: usize,
    start_col: usize,
    count: usize,
    start_dest: usize,
) -> Result<()> {
    // Parse headers once
    // Extract all columns in one pass
    // Minimal overhead per column
}
```

## 4. SIMD Varint Decoding

### Current State
Serial type parsing uses scalar varint decoding.

### Optimization
Use SIMD to parse multiple varints in parallel:

```rust
#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

unsafe fn parse_serial_types_simd(data: &[u8], types: &mut Vec<u8>) -> usize {
    // Load 16 bytes at once
    let chunk = _mm_loadu_si128(data.as_ptr() as *const __m128i);
    
    // Check for continuation bits (MSB set)
    let cont_mask = _mm_movemask_epi8(_mm_cmpgt_epi8(chunk, _mm_set1_epi8(0x7F)));
    
    // Fast path: all single-byte varints
    if cont_mask == 0 {
        // All 16 values are single-byte varints
        _mm_storeu_si128(types.as_mut_ptr() as *mut __m128i, chunk);
        return 16;
    }
    
    // Fall back to scalar for multi-byte varints
    // ...
}
```

## 5. Zero-Copy RefValue for Common Cases

### Problem
Converting `RefValue::Text` to `Value::Text` always allocates.

### Solution
Add a zero-copy path for immutable access:

```rust
pub enum Register {
    Value(Value),
    // New variant for zero-copy references
    Ref(RefValue<'static>),  // Lifetime tied to page cache
}

impl Register {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Register::Value(Value::Text(t)) => Some(&t.value),
            Register::Ref(RefValue::Text(t)) => Some(t.value.as_ref()),
            _ => None,
        }
    }
}
```

## 6. Query-Level Optimization

### Add Column Usage Analysis
Track which columns are actually used and propagate this information:

```rust
// During query planning
pub struct ColumnUsageTracker {
    tables: HashMap<TableId, BitVec>,
}

impl QueryPlanner {
    fn analyze_column_usage(&mut self, select: &Select) -> ColumnUsageTracker {
        // Track which columns appear in:
        // - SELECT clause
        // - WHERE clause  
        // - ORDER BY
        // - GROUP BY
        
        // Propagate to OpenRead instructions
    }
}
```

## Prioritized Implementation Plan

1. **Quick Win**: Header caching in BTreeCursor (1-2 days)
   - Biggest bang for buck
   - Relatively isolated change

2. **Medium**: Batch column access optimization (3-5 days)
   - Requires codegen changes
   - High impact for common patterns

3. **Advanced**: SIMD varint decoding (1 week)
   - Platform-specific code
   - Needs careful benchmarking

4. **Long-term**: Zero-copy architecture (2-3 weeks)
   - Requires broader architectural changes
   - Highest potential impact

## Expected Performance Gains

Based on SQLite's performance and Limbo's architecture:

- **Header Caching**: 15-25% improvement on selective queries
- **Batch Column Access**: 10-15% improvement on multi-column SELECT
- **SIMD Decoding**: 20-30% improvement on header parsing
- **Zero-Copy**: 30-50% reduction in allocation overhead

Combined, these optimizations could close most of the 96% performance gap on selective queries while maintaining Limbo's advantage on full table scans.