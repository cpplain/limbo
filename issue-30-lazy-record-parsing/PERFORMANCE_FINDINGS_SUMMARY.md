# Lazy Record Parsing - Performance Findings Summary

## Bottom Line

**The lazy record parsing implementation is causing performance regression because it's effectively still doing eager parsing, plus adding overhead.**

## Key Problems Found

1. **Memory Copy Defeats Purpose**: Every record copies the entire payload (`payload.to_vec()`)
2. **No Smart Activation**: Lazy parsing used for ALL records, even tiny ones
3. **Sorter Kills Performance**: Pre-parses all columns before sorting
4. **Excessive Allocations**: Creates new Vecs on every comparison during sort
5. **Too Eager Threshold**: Parses everything once 50% of columns are accessed
6. **Benchmarks Not Testing It**: Feature flag may not be enabled during testing

## Why Performance is Worse

```
Current State:
- Overhead: Option<RefValue> wrappers + LazyParseState = ~58 bytes/record extra
- Memory: Doubles payload memory (copy instead of reference)  
- CPU: Parse header + parse columns = more work than just parsing once
- Cache: Larger records = worse cache performance
```

## Quick Fixes That Would Help Most

1. **Stop copying payload** → Use Arc or lifetime reference
2. **Only use for wide tables** → if columns > 8 && payload > 256 bytes
3. **Remove sorter pre-parsing** → Let it parse on-demand during compare
4. **Test with feature enabled** → `cargo bench --features lazy_parsing`

## Expected Results After Fixes

- **Selective queries (10% columns)**: 80-90% faster
- **COUNT(*) on wide tables**: 95%+ faster  
- **ORDER BY**: 20-30% faster
- **SELECT * (all columns)**: Within 5% of current performance

The implementation is functionally correct but needs these performance fixes to deliver the promised benefits.