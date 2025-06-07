# Issue #30: Lazy Record Parsing Analysis

**NOTE: This document contains the original analysis before the lazy parsing implementation was attempted. The implementation was subsequently developed, tested, and reverted due to performance regressions.**

## Issue Summary
- **Issue**: [#30 - Parse record values lazily in `Column` op?](https://github.com/tursodatabase/limbo/issues/30)
- **Goal**: Improve performance by parsing only the columns that are actually used, rather than parsing entire records upfront
- **Previous Attempt**: PR #250 showed performance improvements for some queries but 100% slowdown for `SELECT * FROM USERS`

## Current Limbo Implementation

### Record Parsing Flow
1. **Cursor Movement** (`core/storage/btree.rs:1365` - `get_next_record()`)
   - When cursor moves to a new record, immediately calls `read_record()`
   - Happens for both table cells and index cells

2. **Full Record Parsing** (`core/storage/sqlite3_ondisk.rs:1094` - `read_record()`)
   ```rust
   pub fn read_record(payload: &[u8], reuse_immutable: &mut ImmutableRecord) -> Result<()>
   ```
   - Parses entire record header (all serial types)
   - Deserializes ALL column values immediately
   - Stores parsed values in `ImmutableRecord.values`

3. **Column Access** (`core/vdbe/execute.rs:1332` - Column opcode)
   ```rust
   match record.get_value_opt(*column) {
       Some(val) => val.clone(),
       None => RefValue::Null,
   }
   ```
   - Simply retrieves pre-parsed value by index
   - No parsing happens here - just array access

### Data Structures
- **ImmutableRecord** (`core/types.rs:665`)
  ```rust
  pub struct ImmutableRecord {
      payload: Vec<u8>,           // Raw record data
      pub values: Vec<RefValue>,  // All parsed values
      recreating: bool,
  }
  ```

## SQLite's Implementation (Confirmed)

### Lazy Parsing Strategy
1. **Incremental Header Parsing**
   - Only parses record header up to the requested column
   - Tracks parsing progress with `nHdrParsed`
   - Caches serial types and offsets in `aType[]` and `aOffset[]`

2. **VdbeCursor State** (`src/vdbeInt.h`)
   ```c
   struct VdbeCursor {
       u32 iHdrOffset;      // Offset to next unparsed byte of header
       u16 nHdrParsed;      // Number of header fields parsed so far
       u32 *aOffset;        // Column byte offsets
       u32 aType[];         // Serial types for each column
   }
   ```

3. **OP_Column Implementation** (`src/vdbe.c:2940`)
   - Checks if requested column is already parsed: `if( pC->nHdrParsed<=p2 )`
   - If not, parses header incrementally up to column p2
   - Deserializes only the requested column value
   - Optimized for sequential column access (SELECT *)

### Key Optimizations
- **Sequential Access**: When accessing columns 0,1,2... in order, parsing continues from last position
- **Caching**: Parsed header information persists across Column operations
- **Minimal Overhead**: For TEXT/BLOB, only deserializes when actually needed
- **Special Flags**: `OPFLAG_TYPEOFARG` and `OPFLAG_LENGTHARG` skip value parsing entirely

## Why PR #250 Failed

The 100% slowdown on `SELECT * FROM USERS` suggests the implementation likely:
1. **No persistent parsing state**: Each Column operation may have started parsing from scratch
2. **No sequential access optimization**: Failed to detect and optimize the common SELECT * pattern
3. **Excessive overhead**: Without parse-ahead strategy, each column access paid full parsing cost
4. **State management**: Lacked SQLite's sophisticated cursor state tracking
5. **Cache invalidation**: May have invalidated parsing state too aggressively on cursor operations

## Performance Implications

### Current Limbo Approach
- ✅ Simple implementation
- ✅ Fast column access (just array lookup)
- ❌ Wastes CPU parsing unused columns
- ❌ Wastes memory storing all values
- ❌ Particularly inefficient for:
  - Wide tables with many columns
  - Large TEXT/BLOB values that aren't accessed
  - Queries that only need a few columns

### SQLite's Lazy Approach
- ✅ Only parses what's needed
- ✅ Optimized for both selective and full access
- ✅ Lower memory footprint
- ❌ More complex implementation
- ❌ Slight overhead for column access (but minimal)

## Implementation Challenges for Limbo

1. **Rust Ownership Model**
   - Need to maintain payload buffer lifetime while allowing partial parsing
   - RefValue already handles this, but need careful management

2. **Index Comparisons**
   - Current code expects all values pre-parsed for comparisons
   - Need to either parse indexed columns eagerly or refactor comparison logic

3. **State Management**
   - Need to track parsing progress across multiple Column operations
   - Must handle cursor movement and cache invalidation

4. **Performance Critical Path**
   - Column access is in the hot path of query execution
   - Any added overhead will impact all queries