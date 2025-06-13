# Lazy Record Parsing Benchmarks

This directory contains benchmarks for measuring the performance impact of lazy record parsing in Limbo.

## Overview

Limbo currently uses eager record parsing, which parses all columns in a record even when only a few are needed. This creates significant overhead for analytical queries on wide tables. These benchmarks measure the current performance and will track improvements from implementing lazy parsing.

## Key Finding

**Limbo is currently 2-3x slower than SQLite for selective queries on wide tables**. When accessing only 10% of columns in a 100-column table, Limbo parses all 100 columns while SQLite parses only the needed 10, creating a 3.3x performance overhead.

## Running Benchmarks

The benchmark code (`record_parsing_benchmark.rs`) is stored in this directory for reference and analysis. To run the benchmarks:

### Setup Instructions

1. **Copy the benchmark to Limbo's benchmark directory:**
   ```bash
   cp record_parsing_benchmark.rs ../../core/benches/
   ```

2. **Add to `core/Cargo.toml`:**
   ```toml
   [[bench]]
   name = "record_parsing_benchmark"
   harness = false
   ```

3. **Run from project root:**
   ```bash
   # Full benchmark run
   cargo bench --bench record_parsing_benchmark
   
   # Save baseline before implementing lazy parsing
   cargo bench --bench record_parsing_benchmark -- --save-baseline pre-lazy-parsing
   
   # Compare after implementing lazy parsing
   cargo bench --bench record_parsing_benchmark -- --baseline pre-lazy-parsing
   
   # Quick test run
   cargo bench --bench record_parsing_benchmark -- --sample-size 10
   ```

For detailed instructions, run: `./run_benchmarks.sh`

## Benchmark Structure

### Test Categories

1. **Column Selectivity Tests** - Measure performance with different percentages of columns accessed:
   - 10% selectivity (e.g., 10 of 100 columns)
   - 25% selectivity
   - 50% selectivity
   - 100% selectivity (SELECT *)

2. **Aggregation Tests** - Measure aggregation query performance:
   - COUNT(*) - Should not need column parsing
   - COUNT(column) - Single column access
   - SUM(column) - Single numeric column
   - Multi-column aggregates

3. **Real-World Patterns** - Test realistic query patterns:
   - Filter and project (WHERE + SELECT few columns)
   - GROUP BY with aggregations

### Test Configuration

- **Dataset Size**: 100,000 rows per table
- **Table Widths**: 10, 25, 50, and 100 columns
- **Data Types**: Mixed (INTEGER 25%, REAL 25%, TEXT 25%, BLOB 25%)
- **Query Filter**: WHERE clause returning ~1000 rows (1% selectivity)

## Current Performance

Based on baseline measurements (see [BASELINE_RESULTS.md](BASELINE_RESULTS.md) for details):

| Query Type | Limbo vs SQLite Overhead |
|------------|--------------------------|
| 10% column selectivity | 2.1-3.4x slower |
| 25% column selectivity | 2.2-3.3x slower |
| 50% column selectivity | 1.5-2.6x slower |
| SELECT * | 0.7-2.2x (competitive) |
| COUNT(*) | 600-1000x slower (!!) |

## Expected Improvements

After implementing lazy record parsing:
- **80-90% improvement** for 10% column selectivity (3.3x → ~1.1x overhead)
- **75% improvement** for 25% column selectivity
- **50% improvement** for 50% column selectivity
- **<5% regression** for SELECT * queries
- **99%+ improvement** for COUNT(*) queries

## Implementation Details

The benchmark implementation is self-contained in this directory:
- **Benchmark code**: `record_parsing_benchmark.rs` (in this directory)
- **Run script**: `run_benchmarks.sh`

The benchmarks:
- Use temporary databases created on-the-fly
- Compare Limbo directly against rusqlite in the same process
- Handle async I/O properly using Limbo's step() API
- Use Criterion for statistically rigorous measurements
- Support flamegraph profiling for performance analysis

**Note**: This is a standalone benchmark for analysis purposes. When implementing lazy parsing, the benchmark will need to be integrated into Limbo's test suite by moving to `core/benches/` and updating `core/Cargo.toml`.

## Files in This Directory

- `README.md` - This file
- `run_benchmarks.sh` - Convenience script for running benchmarks
- `BASELINE_RESULTS.md` - Detailed analysis of current performance
- `IMPLEMENTATION_NOTES.md` - Technical details about benchmark corrections

## Next Steps

1. Review baseline results in [BASELINE_RESULTS.md](BASELINE_RESULTS.md)
2. Implement lazy record parsing following the design documentation
3. Re-run benchmarks to measure improvement
4. Ensure performance targets are met