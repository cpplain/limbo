# Benchmark Status - June 15, 2025

## Current Results

### With Lazy Parsing Enabled
```
Selective Queries (10% columns, 50-column table): 10.534 µs (+12.2% regression)
ORDER BY Selective (50-column table): 60.896 ms (+14% regression)
```

### Without Lazy Parsing (Baseline)
```
Selective Queries (10% columns, 50-column table): 9.372 µs
ORDER BY Selective (50-column table): 53.393 ms
```

## How to Run Benchmarks

### Test Current Implementation
```bash
# With lazy parsing (shows regression)
cargo bench --bench record_parsing_benchmark --features lazy_parsing -- "selectivity_10pct_50cols"

# Without lazy parsing (baseline)
cargo bench --bench record_parsing_benchmark -- "selectivity_10pct_50cols"
```

### After Implementing Fixes
```bash
# 1. First, implement fixes from CRITICAL_FIXES_REQUIRED.md
# 2. Then run benchmarks to verify improvement:
./run_benchmarks.sh

# Or run specific scenarios:
cargo bench --bench record_parsing_benchmark --features lazy_parsing
```

## Expected Results After Fixes

Once the sorter pre-parsing is removed and cloning is eliminated:

- Selective queries (10% columns): Should be 70-80% FASTER than baseline
- ORDER BY queries: Should be 15-25% FASTER than baseline
- Memory usage: Should show 20-30% reduction

## Critical Fix Verification

Before running benchmarks, verify the fix:
```bash
# This should return NOTHING (no output):
grep -A5 "feature.*lazy_parsing" ../../core/vdbe/sorter.rs | grep "parse_column"
```

If it returns output, the sorter pre-parsing has NOT been removed and benchmarks will continue to show regression.