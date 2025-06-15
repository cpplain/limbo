# Why Lazy Record Parsing Is Still Failing

_Date: June 15, 2025_

## The Bottom Line

**Lazy record parsing shows 12-14% performance regression because it adds overhead without actually being lazy.**

## The Fundamental Problem

The implementation adds all the costs of lazy parsing:
- ✗ Option wrapper overhead (8 bytes per column)
- ✗ Arc reference counting overhead
- ✗ Cloning overhead in hot paths
- ✗ Larger memory footprint
- ✗ Extra indirection on every access

But fails to deliver the benefits:
- ✗ Still parses columns eagerly in sorter
- ✗ Clones entire records unnecessarily
- ✗ No actual "laziness" in critical paths

## The Smoking Gun

In `/core/vdbe/sorter.rs:47-51`, despite claims of being fixed:

```rust
// This code STILL EXISTS and defeats lazy parsing:
for record in &mut self.records {
    for i in 0..self.key_len {
        let _ = record.parse_column(i);  // PARSES EAGERLY!
    }
}
```

This means:
1. Every record inserted into sorter gets cloned
2. Every sort key column gets parsed immediately
3. All the lazy parsing machinery runs for nothing
4. We pay the overhead cost without getting the benefit

## Why Each "Fix" Failed

| Claimed Fix | What Was Done | Why It Failed |
|------------|---------------|---------------|
| Stop copying payload | Used Arc<[u8]> | Added atomic overhead without removing parsing |
| Smart activation | Added thresholds | Overhead still present for "qualified" records |
| Remove sorter pre-parsing | **NOT ACTUALLY DONE** | Code still pre-parses all key columns |
| Eliminate allocations | Removed Vec collection | Overshadowed by pre-parsing issue |
| Increase threshold to 75% | Changed from 50% | Still parses everything for high-selectivity |
| Fix VDBE integration | Added lazy support | Includes unnecessary cloning |

## The Performance Math

For a 50-column record accessed 10%:

**Eager Parsing**:
- Parse 50 columns once: 50 units of work
- Access 5 columns: ~0 units (already parsed)
- Total: 50 units

**Lazy Parsing (Theory)**:
- Parse 5 columns on-demand: 5 units of work
- Total: 5 units (90% improvement!)

**Lazy Parsing (Current Reality)**:
- Clone record: 5 units
- Option checks: 5 units  
- Arc overhead: 3 units
- Parse 5 columns: 5 units
- **Sorter pre-parses anyway**: 50 units
- Total: 68 units (36% WORSE!)

## Three Critical Actions to Fix

1. **DELETE the sorter pre-parsing loop** (lines 47-51 in sorter.rs)
2. **STOP cloning records** in sorter and Column instruction
3. **INCREASE thresholds** to 16 columns and 512 bytes

Without these fixes, lazy parsing will always be slower than eager parsing.

## One-Line Summary

**The implementation is "lazy" in name only - it does all the work of eager parsing plus adds overhead.**