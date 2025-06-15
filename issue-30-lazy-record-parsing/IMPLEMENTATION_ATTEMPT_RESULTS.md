# Lazy Record Parsing - Implementation Attempt Results

_Date: June 15, 2025_

## Changes Implemented

### 1. ~~Removed Sorter Pre-Parsing~~ (Reverted)
**File**: `core/vdbe/sorter.rs`  
**Issue**: Cannot remove pre-parsing due to Rust's sort_by limitations
- `sort_by` provides immutable references only
- Lazy parsing requires mutation to parse on-demand
- Attempted RefCell approach adds too much overhead
- **Conclusion**: Pre-parsing sort keys is unavoidable with current design

### 2. Fixed Redundant Clone
**File**: `core/vdbe/execute.rs:1464`  
**Change**: Removed unnecessary `record.clone()` 
- Was cloning an already-owned record
- Minor improvement but doesn't address main issue

### 3. Adjusted Thresholds (Then Reverted)
**File**: `core/storage/sqlite3_ondisk.rs`
- Tried 16 columns / 512 bytes (no tables qualified)
- Reverted to 8 columns / 256 bytes for testing
- Thresholds alone don't solve the performance issue

## Fundamental Problems Discovered

### 1. Rust's Sort Limitations
The core issue is that Rust's `sort_by` closure receives `&T` (immutable references), but lazy parsing requires `&mut T` to parse columns on demand. Options:
- **RefCell**: Adds runtime overhead, defeats performance gains
- **Pre-parse keys**: What we're forced to do, defeats lazy parsing for ORDER BY
- **Custom sort**: Would require reimplementing sort algorithm

### 2. Inherent Overhead
Even without sorting issues, lazy parsing adds:
- `Option<RefValue>` wrapper: 33% memory overhead per column
- `Arc<[u8]>` for payload: Atomic reference counting overhead
- `LazyParseState`: ~40-80 bytes per record
- Parse state checking on every access

### 3. Clone-Heavy Design
The current architecture clones records extensively:
- Sorter clones on insert
- Column instruction clones from sorter
- Each clone duplicates the entire structure

## Benchmark Results After Changes

```
selectivity_10pct_50cols: +12.86% regression (was +12.2%)
selectivity_10pct_100cols: +5.2% regression  
order_by_selective_50_cols: +13.8% regression (was +14%)
```

**No meaningful improvement despite changes.**

## Why Lazy Parsing Is Failing

The implementation adds overhead without removing work:

1. **For ORDER BY**: Must pre-parse sort keys anyway
2. **For Selective Queries**: Overhead exceeds savings for moderate column counts
3. **Architecture Mismatch**: Clone-heavy design amplifies overhead

## Potential Solutions (Not Implemented)

### 1. Redesign Sorter Completely
- Store record indices instead of cloning records
- Sort indices, then reorder records once
- Avoids cloning and allows lazy access

### 2. Alternative to Option Wrapper
- Use separate bit vector for parse state
- Or sentinel values in RefValue
- Reduces per-column overhead

### 3. Unsafe Interior Mutability
- Use unsafe code to mutate during sorting
- Trade safety for performance
- Requires careful implementation

### 4. Query-Aware Optimization
- Detect ORDER BY queries and use eager parsing
- Only use lazy parsing for simple SELECT queries
- Requires plumbing query context through system

## Conclusion

The current lazy parsing implementation cannot achieve its performance goals due to fundamental architectural mismatches:

1. Rust's sort_by doesn't support the mutation needed for true lazy parsing
2. The overhead of Option wrappers and Arc exceeds the savings for many queries  
3. The clone-heavy architecture amplifies all overhead

Without a significant architectural redesign, lazy record parsing will remain slower than eager parsing. The "fixes" suggested in the previous analysis addressed symptoms, not the root causes.