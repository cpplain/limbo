# Wide Table Optimization Implementation Guide

## The Problem (30 seconds)

Limbo is 96% slower than SQLite when selecting 3 columns from a 100-column table. SQLite achieves this through **header caching**, not lazy value parsing. We need to add header caching and optimize the VM path.

## What NOT To Do ❌

- **Don't** implement lazy value parsing (52% performance regression - already tried)
- **Don't** change the RefValue/Value architecture 
- **Don't** add complex state management
- **Don't** read the 20+ historical docs in the old branch

## Implementation Tasks ✅

### Task 1: Add Header Caching to BTreeCursor [HIGH PRIORITY]

**Time estimate**: 1-2 days  
**Expected gain**: 15-25% improvement

**What to build**:
```rust
// In core/storage/btree.rs - add to BTreeCursor struct
pub struct HeaderCache {
    serial_types: Vec<u8>,      // Type of each column
    offsets: Vec<u32>,          // Byte offset to each column
    parsed_up_to: usize,        // Columns parsed so far
    generation: u64,            // Invalidate on cursor move
}

// In BTreeCursor implementation
pub fn get_column_cached(&mut self, column: usize) -> Result<RefValue> {
    if self.header_cache.is_none() || 
       self.header_cache.as_ref().unwrap().parsed_up_to <= column {
        self.parse_headers_up_to(column)?;
    }
    
    let cache = self.header_cache.as_ref().unwrap();
    let offset = cache.offsets[column];
    let serial_type = cache.serial_types[column];
    
    // Jump directly to column data
    deserialize_value(serial_type, &self.payload[offset..])
}
```

**Key files to modify**:
- `core/storage/btree.rs` - Add HeaderCache to BTreeCursor
- `core/storage/sqlite3_ondisk.rs` - Modify read_record to populate cache
- `core/vdbe/execute.rs` - Update op_column to use cached access

### Task 2: Batch Column Access [MEDIUM PRIORITY]

**Time estimate**: 3-5 days  
**Expected gain**: 10-15% improvement

**What to build**:
```rust
// New instruction in core/vdbe/insn.rs
pub enum Insn {
    // ... existing variants ...
    ColumnRange {
        cursor_id: usize,
        start_col: usize,
        count: usize,
        start_dest: usize,
    },
}

// Optimizer in core/translate/emitter.rs
fn optimize_column_sequence(insns: &mut Vec<Insn>) {
    // Find patterns like:
    // Column(cursor: 0, column: 0, dest: 1)
    // Column(cursor: 0, column: 1, dest: 2)
    // Column(cursor: 0, column: 2, dest: 3)
    
    // Replace with:
    // ColumnRange(cursor: 0, start_col: 0, count: 3, start_dest: 1)
}
```

### Task 3: VM Fast Path [LOW PRIORITY]

**Time estimate**: 2-3 days  
**Expected gain**: 5-10% improvement

**What to build**:
```rust
// In core/vdbe/execute.rs
#[inline(always)]
pub fn op_column_fast(
    state: &mut ProgramState,
    cursor_id: usize,
    column: usize,
    dest: usize,
) -> Result<()> {
    // Skip all the match statements and type checking
    let cursor = unsafe { state.cursors.get_unchecked_mut(cursor_id) };
    let value = cursor.btree.get_column_cached(column)?;
    
    // Reuse existing allocations
    match (&value, &mut state.registers[dest]) {
        (RefValue::Text(src), Register::Value(Value::Text(dst))) => {
            dst.value.clear();
            dst.value.extend_from_slice(src.value.as_ref());
            return Ok(());
        }
        _ => {}
    }
    
    state.registers[dest] = Register::Value(value.to_owned());
    Ok(())
}
```

## How to Test

### Benchmark Setup
```bash
# Create test database
cd testing
python gen-database.py

# Run the specific benchmark that shows the problem
cd ../issue-30-lazy-parsing/benchmarks
cargo bench -- "wide_table_selective"
```

### Expected Results
- Baseline: ~290µs (current Limbo)
- After Task 1: ~220µs (25% improvement)
- After Task 2: ~190µs (35% total improvement)
- After Task 3: ~170µs (40% total improvement)
- SQLite: ~110µs (our target)

### Verification
```sql
-- This is the query we're optimizing for
SELECT col1, col2, col3 FROM wide_table_100_cols;
```

## Common Pitfalls

1. **Don't invalidate cache too aggressively** - Only invalidate on cursor movement
2. **Watch for overflow pages** - Cache won't help if payload spans pages
3. **Test with NULL values** - They have different serialization
4. **Profile first** - Use `cargo flamegraph` to verify you're optimizing the right thing

## Questions?

If you need context on why we're NOT doing lazy parsing, see the previous branch's failed attempts. Otherwise, focus on the tasks above.

## Quick Start Checklist

- [ ] Read this guide (5 minutes)
- [ ] Run the benchmark to see current performance
- [ ] Start with Task 1 (header caching)
- [ ] Test with the benchmark after each change
- [ ] Submit PR when you hit 20%+ improvement