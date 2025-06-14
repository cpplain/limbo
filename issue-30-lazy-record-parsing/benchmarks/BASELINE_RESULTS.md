# Baseline Performance Results

Generated: December 6, 2025

## Executive Summary

This document presents the baseline performance measurements of Limbo's current eager record parsing implementation compared to SQLite (via rusqlite). The results confirm significant optimization opportunities through lazy record parsing, particularly for selective queries on wide tables.

### Key Findings

1. **Limbo is 2-3x slower than SQLite for selective queries** due to parsing all columns eagerly
2. **Performance gap increases with table width** - 100-column tables show up to 3.3x overhead
3. **Aggregation queries show massive overhead** - COUNT(*) is 640x slower in Limbo
4. **SELECT * queries are competitive** - Limbo sometimes outperforms SQLite on full-row scans

## Detailed Performance Measurements

### Column Selectivity Performance

Time per query in microseconds (µs) for queries returning ~1000 rows:

#### 10-Column Tables
| Selectivity | Columns Selected | Limbo | SQLite | Overhead | Analysis |
|-------------|------------------|-------|--------|----------|----------|
| 10% | 1 of 10 | 3.5µs | 1.7µs | 2.1x | Parsing 9 unnecessary columns |
| 25% | 2 of 10 | 6.9µs | 2.1µs | 3.3x | Parsing 8 unnecessary columns |
| 50% | 5 of 10 | 7.8µs | 3.0µs | 2.6x | Parsing 5 unnecessary columns |
| 100% | All 10 | 8.1µs | 3.7µs | 2.2x | All columns needed |

#### 25-Column Tables
| Selectivity | Columns Selected | Limbo | SQLite | Overhead | Analysis |
|-------------|------------------|-------|--------|----------|----------|
| 10% | 2 of 25 | 7.3µs | 2.2µs | 3.3x | Parsing 23 unnecessary columns |
| 25% | 6 of 25 | 9.9µs | 3.6µs | 2.8x | Parsing 19 unnecessary columns |
| 50% | 12 of 25 | 12.1µs | 5.9µs | 2.1x | Parsing 13 unnecessary columns |
| 100% | All 25 | 11.9µs | 7.9µs | 1.5x | All columns needed |

#### 50-Column Tables
| Selectivity | Columns Selected | Limbo | SQLite | Overhead | Analysis |
|-------------|------------------|-------|--------|----------|----------|
| 10% | 5 of 50 | 8.5µs | 2.5µs | 3.4x | Parsing 45 unnecessary columns |
| 25% | 12 of 50 | 12.3µs | 4.6µs | 2.7x | Parsing 38 unnecessary columns |
| 50% | 25 of 50 | 17.0µs | 8.8µs | 1.9x | Parsing 25 unnecessary columns |
| 100% | All 50 | 16.3µs | 18.4µs | 0.9x | Limbo faster! |

#### 100-Column Tables
| Selectivity | Columns Selected | Limbo | SQLite | Overhead | Analysis |
|-------------|------------------|-------|--------|----------|----------|
| 10% | 10 of 100 | 14.0µs | 4.3µs | 3.3x | Parsing 90 unnecessary columns |
| 25% | 25 of 100 | 19.5µs | 8.7µs | 2.2x | Parsing 75 unnecessary columns |
| 50% | 50 of 100 | 30.5µs | 20.2µs | 1.5x | Parsing 50 unnecessary columns |
| 100% | All 100 | 27.0µs | 40.8µs | 0.7x | Limbo 30% faster! |

### Aggregation Performance

Time per query in milliseconds (ms) for queries processing 100,000 rows:

| Query Type | Description | 10 cols | 25 cols | 50 cols | 100 cols |
|------------|-------------|---------|---------|---------|----------|
| **COUNT(*)** | | | | | |
| Limbo | No parsing needed | 5.8ms | 9.8ms | 16.5ms | 29.4ms |
| SQLite | Optimized path | 0.009ms | 0.009ms | 0.009ms | 0.009ms |
| **Overhead** | | **644x** | **1089x** | **1833x** | **3267x** |
| | | | | | |
| **COUNT(column)** | | | | | |
| Limbo | Single column | 7.0ms | 6.9ms | 11.1ms | 18.7ms |
| SQLite | | 1.3ms | 1.3ms | 1.3ms | 1.3ms |
| **Overhead** | | **5.4x** | **5.3x** | **8.5x** | **14.4x** |

### Performance Characteristics

1. **Linear Scaling with Unused Columns**: Limbo's performance degrades linearly with table width even when accessing few columns, confirming eager parsing overhead.

2. **Massive COUNT(*) Overhead**: The 600-3000x overhead for COUNT(*) queries indicates Limbo is unnecessarily parsing all record data when only row counting is needed.

3. **Competitive SELECT ***: Limbo actually outperforms SQLite on SELECT * queries for wider tables (30% faster on 100-column tables), suggesting the core parsing logic is efficient once all columns are needed.

4. **Overhead Formula**: For selective queries, overhead ≈ (total_columns / selected_columns) × base_overhead

## Root Cause Analysis

The performance gap is caused by Limbo's eager record parsing approach:

1. **Current Behavior**: When accessing any column, Limbo parses the entire record into a Vec<Value>
2. **SQLite Behavior**: Only parses the specific columns requested
3. **Impact**: Time wasted = (unused_columns / total_columns) × total_parse_time

Example: For a 100-column table where only 10 columns are needed:
- SQLite: Parses 10 columns → 4.3µs
- Limbo: Parses 100 columns → 14.0µs
- Wasted effort: 90% of parsing time

## Optimization Opportunities

Based on these measurements, lazy record parsing should provide:

### Projected Improvements

1. **10% Column Selectivity**
   - Current: 3.3x overhead
   - Target: ~1.1x overhead
   - **Expected improvement: 80-90%**

2. **COUNT(*) Queries**
   - Current: 1000x+ overhead
   - Target: Near parity with SQLite
   - **Expected improvement: 99%+**

3. **Aggregation Queries**
   - Current: 5-14x overhead
   - Target: ~1.5x overhead
   - **Expected improvement: 70-85%**

### Critical Optimizations

1. **COUNT(*) Fast Path**: Implement row counting without parsing record data
2. **Header-Only Parsing**: Separate header parsing from value parsing
3. **On-Demand Value Parsing**: Parse columns only when accessed
4. **Smart Thresholds**: Switch to eager parsing when >75% columns accessed

## Benchmark Methodology

- **Dataset**: 100,000 rows per table
- **Table Widths**: 10, 25, 50, and 100 columns
- **Data Types**: Mixed (INTEGER 25%, REAL 25%, TEXT 25%, BLOB 25%)
- **Query Filter**: WHERE col_0 < 1000 (returns ~1000 rows)
- **Environment**: Same machine, release builds, warm cache
- **Statistical Rigor**: Criterion with 10+ samples, outlier detection

## Recommendations

1. **Prioritize COUNT(*) Optimization**: The 1000x+ overhead is the most critical issue
2. **Focus on Wide Tables**: Tables with 50+ columns show the most dramatic benefits
3. **Implement Progressive Parsing**: Start with header, parse values on demand
4. **Benchmark Each Optimization**: Measure impact of each change

## Next Steps

1. Implement lazy record parsing following the design documentation
2. Re-run benchmarks: `./run_benchmarks.sh --compare-baseline pre-lazy-parsing`
3. Verify improvements meet targets
4. Profile any remaining bottlenecks with flamegraphs

## Note on ORDER BY Benchmarks

ORDER BY benchmarks were added to the suite on June 14, 2025, after the initial baseline measurements. These benchmarks test:
- Selective column retrieval with ORDER BY
- Full SELECT * with ORDER BY
- Multi-key ORDER BY scenarios

These were added specifically to measure the impact of the sorter optimization that was implemented as part of Fix #3.