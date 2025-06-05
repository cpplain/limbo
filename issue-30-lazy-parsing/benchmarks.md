# Lazy Parsing Benchmark Infrastructure

## Overview

This benchmark infrastructure was created to measure the performance impact of implementing lazy record parsing (Issue #30). It specifically targets scenarios where lazy parsing should provide benefits while ensuring no regression on common patterns.

## Benchmark Structure

### Test Database
- Creates temporary databases with wide tables (10, 50, 100, 200 columns)
- Mixed data types: INTEGER, TEXT, REAL, BLOB
- 1000 rows per table
- Larger BLOB values in the last 25% of columns to maximize lazy parsing benefits

### Access Patterns Tested

1. **SELECT * (select_all)**
   - Critical benchmark - MUST NOT regress more than 5%
   - Tests the common case where all columns are accessed
   - This is where PR #250 failed with 100% slowdown

2. **SELECT first 3 columns (select_first_3)**
   - Sequential partial access pattern
   - Should benefit from lazy parsing
   - Common pattern in real applications

3. **SELECT sparse columns (select_sparse)**
   - Selects col1, middle column, and last column
   - Non-sequential access pattern
   - Should show significant improvement with lazy parsing

4. **SELECT last column (select_last)**
   - Best case for lazy parsing
   - Only needs to parse one column at the end
   - Expected 20-50% improvement

## Running Benchmarks

### Full Benchmark Suite
```bash
cargo bench -p limbo_core
```

### Lazy Parsing Benchmarks Only
```bash
cargo bench -p limbo_core --bench benchmark -- lazy_parsing
```

### Capture Baseline (Before Implementation)
```bash
./core/benches/lazy_parsing_baseline.sh
```

### Generate Flamegraphs
```bash
cargo bench -p limbo_core --bench benchmark -- lazy_parsing --profile-time=5
```

## Interpreting Results

### Baseline Performance
Results are stored in `target/criterion/lazy_parsing_*/`

Key files:
- `base/estimates.json` - Statistical estimates of performance
- `report/index.html` - HTML report with graphs

### Regression Detection
After implementing lazy parsing, run benchmarks again and compare:

```bash
cargo bench -p limbo_core --bench benchmark -- lazy_parsing
```

Criterion will automatically compare with baseline and report:
- Performance improvements (green)
- Performance regressions (red)
- Statistical significance

### Success Criteria

1. **No Regression on SELECT ***
   - Must be within 5% of baseline performance
   - This is the most critical metric

2. **Improvements on Selective Access**
   - select_first_3: 10-20% improvement expected
   - select_sparse: 20-40% improvement expected
   - select_last: 30-50% improvement expected

3. **Memory Usage**
   - Should reduce memory footprint for wide tables
   - Especially when large TEXT/BLOB columns aren't accessed

## Implementation Notes

### Why These Benchmarks Matter

1. **PR #250 Failed Due to SELECT * Regression**
   - Without proper benchmarks, the implementation caused 100% slowdown
   - Our benchmarks specifically test this scenario

2. **Real-World Patterns**
   - Wide tables are common in enterprise applications
   - ORMs often select all columns even when only a few are needed
   - Large TEXT/BLOB values waste memory when parsed eagerly

3. **Sequential Access Optimization**
   - SQLite succeeds because it detects sequential patterns
   - Our benchmarks test both sequential and random access

### Future Enhancements

1. **Memory Usage Tracking**
   - Add custom metrics to track heap allocations
   - Measure peak memory usage during query execution

2. **Cache Hit Rates**
   - Track how often parsed values are reused
   - Measure effectiveness of caching strategy

3. **Overflow Page Handling**
   - Test with very large BLOB values spanning multiple pages
   - Measure impact of lazy parsing on overflow page access

## Troubleshooting

### Benchmark Variance
If results show high variance:
- Increase sample size: `group.sample_size(50)`
- Close other applications
- Disable CPU frequency scaling
- Run with `nice -n -20`

### Comparison Issues
If criterion can't find baseline:
- Ensure you ran baseline capture first
- Check `target/criterion/` directory
- Use `--save-baseline` flag explicitly

### Platform Differences
- Linux: Best performance, io_uring support
- macOS: May show different results due to I/O implementation
- Windows: Not yet fully optimized