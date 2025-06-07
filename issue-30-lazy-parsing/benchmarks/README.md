# Lazy Parsing Benchmarks

This directory contains standalone benchmarks for measuring the performance impact of lazy record parsing (Issue #30).

## Purpose

These benchmarks are separate from the main benchmark suite to:
- Avoid conflicts with existing benchmarks
- Allow experimentation without affecting CI
- Provide clear before/after comparisons
- Enable discussion with maintainers before integration

## Running Benchmarks

```bash
# From the limbo root directory
cd issue-30-lazy-parsing/benchmarks
cargo bench

# To run specific benchmarks
cargo bench -- "SELECT \*"
cargo bench -- "partial"
```

## Benchmark Structure

The benchmarks test various column access patterns:
1. **SELECT \*** - Critical test that must not regress >5%
2. **Partial column access** - Should show 50-70% improvement
3. **Sparse column access** - Tests non-sequential access
4. **Last column access** - Best case for lazy parsing

## Test Data

The benchmarks create temporary databases with:
- 10 columns (using existing users table schema)
- 50 columns (medium width)
- 100 columns (wide table)
- Mixed data types (INTEGER, TEXT, REAL, BLOB)
- 1000 rows per table

## Expected Results

### Without Lazy Parsing
- 100-column SELECT *: ~457 µs
- 100-column SELECT 3 cols: ~286 µs (62% of full time)
- Shows most time is spent parsing unused columns

### With Lazy Parsing (Expected)
- 100-column SELECT *: <480 µs (5% regression limit)
- 100-column SELECT 3 cols: ~100-140 µs (50-70% improvement)

## Integration Notes

When ready to integrate into main benchmarks:
1. Database locking issues need to be addressed
2. Consider using pre-built test databases
3. May need to coordinate with other benchmarks
4. Follow existing benchmark naming patterns

See `../integration-strategy.md` for detailed integration plans.