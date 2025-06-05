# Baseline Performance Results

These are the baseline performance measurements for Limbo before implementing lazy record parsing. These will be used to detect regressions and measure improvements.

## 10 Column Table Results

- **SELECT * (all columns)**: 17.9 µs
- **SELECT first 3 columns**: 10.7 µs

## 100 Column Table Results

- **SELECT * (all columns)**: 457 µs ⚠️ **Critical benchmark - must not regress > 5%**
- **SELECT first 3 columns**: 286 µs
- **SELECT sparse columns**: 294 µs
- **SELECT last column**: 293 µs

## Key Observations

1. **Current Eager Parsing Overhead**: 
   - With 100 columns, SELECT * takes 457 µs
   - SELECT 3 columns takes 286 µs (62% of full time)
   - SELECT 1 column takes 293 µs (64% of full time)
   - This shows most time is spent parsing ALL columns even when only a few are needed

2. **Opportunity for Improvement**:
   - Selecting 3 columns currently takes 62% of the time of SELECT *
   - With lazy parsing, this should drop to ~10-20%
   - Single column selection should be even faster

3. **Regression Limits**:
   - SELECT * on 100 columns: Must stay under 480 µs (5% regression limit)
   - Any regression beyond this indicates the lazy parsing implementation has fundamental issues

## Expected Improvements After Lazy Parsing

Based on the current measurements and SQLite's performance:

- **SELECT first 3 columns**: ~50-70% improvement expected (286 µs → ~100-140 µs)
- **SELECT sparse columns**: ~60-80% improvement expected (294 µs → ~60-120 µs)  
- **SELECT last column**: ~70-85% improvement expected (293 µs → ~45-90 µs)
- **SELECT * (all columns)**: <5% regression acceptable (457 µs → <480 µs)

## Next Steps

1. Save these baseline measurements
2. Implement lazy parsing following the plan
3. Re-run benchmarks and compare
4. Ensure SELECT * regression is under 5%
5. Verify expected improvements on selective queries