# Benchmark Comparison Report

## Summary
Current benchmarks show that lazy parsing has NOT been enabled yet - the performance characteristics match eager parsing expectations.

## Results Comparison

### 10 Column Table (users)
| Query | Baseline | Current | Change | Status |
|-------|----------|---------|--------|--------|
| SELECT * | 17.9 µs | 17.3 µs | -3.4% | ✅ Slightly improved |
| SELECT first 3 | 10.7 µs | 10.4 µs | -2.8% | ✅ Slightly improved |
| SELECT sparse (id, city, age) | - | 9.8 µs | - | ✅ Good baseline |

### 50 Column Table
| Query | Baseline | Current | Change | Status |
|-------|----------|---------|--------|--------|
| SELECT * | 192.9 µs | 194.5 µs | +0.8% | ✅ Within variance |
| SELECT first 3 | 128.6 µs | 129.7 µs | +0.9% | ✅ Within variance |

### 100 Column Table
| Query | Baseline | Current | Change | Status |
|-------|----------|---------|--------|--------|
| SELECT * | 457 µs | 458.2 µs | +0.3% | ✅ Well within 5% limit |
| SELECT first 3 | 286 µs | 282.5 µs | -1.2% | ✅ Slightly improved |

## Key Observations

1. **No Lazy Parsing Active**: The results confirm lazy parsing is not yet enabled:
   - SELECT 3 columns from 100-column table: 282.5 µs (62% of SELECT * time)
   - This matches the expected eager parsing behavior where all columns are parsed

2. **Stable Performance**: All benchmarks are within normal variance (±3%)
   - No regressions detected
   - SELECT * performance is stable

3. **Ready for Lazy Parsing**: With these baselines confirmed:
   - 100-column SELECT *: Must stay under 480 µs after lazy parsing
   - 100-column SELECT 3: Should improve to ~100-140 µs (currently 282.5 µs)
   - This represents a 50-70% improvement opportunity

## Next Steps

1. Enable lazy parsing feature flag
2. Re-run benchmarks to measure improvements
3. Verify SELECT * stays within 5% regression limit
4. Confirm selective queries show expected 50-70% improvements

## Technical Notes

- Benchmarks run with SQLite comparisons disabled due to database locking
- Using read-only connections should resolve this for future runs
- Results are consistent with previous baseline measurements