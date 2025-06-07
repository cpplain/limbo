# SQLite Wide Table Optimization Analysis

## Executive Summary

After analyzing SQLite's source code, I've identified that SQLite's performance advantage on selective queries (96% faster when selecting 3 columns from a 100-column table) comes primarily from its **lazy column header parsing** implementation in the VDBE, not from deferring value deserialization. Limbo already has a similar mechanism via `column_mask`, but the performance gap suggests there may be additional optimizations needed at the query planning or VM execution level.

## Key SQLite Optimizations for Wide Tables

### 1. Lazy Column Header Parsing

SQLite's `VdbeCursor` structure implements a sophisticated caching mechanism:

```c
// In VdbeCursor structure:
u32 *aOffset;      // Array of offsets to each column's data
u32 *aType;        // Array storing each column's type  
u32 nHdrParsed;    // Number of headers parsed so far
u32 cacheStatus;   // Cache validity indicator
```

The `OP_Column` opcode only parses column headers up to the requested column:
- First access: Parse headers from 0 to requested column
- Subsequent accesses: Use cached offsets for already-parsed columns
- Never parse headers for columns that aren't accessed

### 2. Column Usage Tracking

SQLite tracks which columns are used via a 64-bit `colUsed` mask:
- Propagated through the query planner
- Used to optimize covering indexes
- Passed to virtual table implementations
- Helps avoid unnecessary work at multiple levels

### 3. Direct Offset Jumping

Once a column's offset is cached, SQLite can:
- Jump directly to the column's data using `aOffset[column]`
- Skip deserialization of intervening columns
- Only materialize the requested value

### 4. Key Implementation Details

```c
// Simplified OP_Column logic:
if (pC->nHdrParsed <= p2) {
    // Parse headers up to column p2
    while (i <= p2 && zHdr < zEndHdr) {
        // Parse serial type
        aType[i] = read_varint(zHdr);
        aOffset[i+1] = aOffset[i] + sqlite3VdbeSerialTypeLen(aType[i]);
        i++;
    }
    pC->nHdrParsed = i;
}
// Now use cached offset to jump to column data
zData = pC->aRow + aOffset[p2];
sqlite3VdbeSerialGet(zData, aType[p2], pDest);
```

## Comparison with Limbo's Current Implementation

### What Limbo Already Has

1. **Column Mask Support**: Limbo's `OpenRead` accepts a `column_mask` parameter
2. **Selective Parsing**: `read_record_projected()` only parses columns in the mask
3. **Proper Integration**: BTreeCursor stores and uses the column mask

### What's Different

1. **Architecture**: Limbo uses `RefValue` → `Value` conversion, forcing early materialization
2. **No Header Caching**: Limbo doesn't cache parsed header information between column accesses
3. **All-or-Nothing**: Limbo's projection parsing happens at record read time, not per-column access

## Why Previous Lazy Parsing Attempts Failed

The issue-30 attempts tried to implement SQLite-style lazy parsing but hit architectural mismatches:

1. **Over-Engineering**: Cached entire parsed VALUES instead of just metadata
2. **Wrong Layer**: Tried to defer at the record level instead of the column access level
3. **Allocation Overhead**: The `RefValue` → `Value` conversion negates lazy parsing benefits

## Recommendations for Limbo

### 1. Don't Pursue SQLite-Style Lazy Parsing

The previous attempts showed that Limbo's architecture (Rust ownership, `RefValue`/`Value` split) isn't well-suited for SQLite's approach. The 52% performance regression confirms this.

### 2. Optimize What Works: Eager Parsing

Limbo is already 40% faster than SQLite on 100-column tables with full scans. Focus on:
- **SIMD Varint Decoding**: Vectorize the header parsing loop
- **Better Memory Layout**: Reduce allocations in the hot path
- **Zero-Copy Architecture**: Eliminate the `RefValue` → `Value` conversion where possible

### 3. Attack the Real Bottleneck: Query Planning

The 96% performance gap on selective queries likely comes from:
- **Missing Query Optimizations**: SQLite may be eliminating unnecessary work at the query level
- **VM Overhead**: Instruction dispatch overhead for wide tables
- **Better Projection Pushdown**: Ensure column masks are propagated optimally

### 4. Specific Quick Wins

1. **Header Caching** (if feasible): Cache just the type/offset arrays, not parsed values
2. **Batch Column Access**: Optimize for accessing multiple columns from the same record
3. **VM Instruction Fusion**: Combine multiple Column operations into one
4. **Profile-Guided Optimization**: Focus on the exact bottlenecks in the 3-column SELECT case

## Conclusion

SQLite's wide table performance comes from careful lazy evaluation of column headers and sophisticated caching, not from deferring value parsing. Limbo's eager parsing approach is fundamentally sound and already performs well. Rather than trying to retrofit SQLite's lazy approach, Limbo should:

1. Make eager parsing even faster (SIMD, zero-copy)
2. Reduce VM overhead for column access
3. Ensure optimal query planning for selective queries

The architectural differences between SQLite (C, zero-copy pointers) and Limbo (Rust, owned values) mean that different optimization strategies are appropriate for each system.