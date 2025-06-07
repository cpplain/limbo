# Historical Benchmark Data - Lazy Record Parsing Development

This document archives the historical benchmark data collected during the development of lazy record parsing for Limbo (Issue #30). These measurements were taken to establish baselines, track progress, and ensure performance goals were met.

**Note**: For current performance analysis and bug investigation results, see `performance-bug-analysis.md`.

## Baseline Performance (Before Implementation)

These measurements were taken before implementing lazy record parsing to establish performance baselines and set regression limits.

### 10 Column Table Results
- **SELECT * (all columns)**: 17.9 µs (Limbo) / 22.1 µs (SQLite)
- **SELECT first 3 columns**: 10.7 µs (Limbo) / 10.7 µs (SQLite)

### 50 Column Table Results
- **SELECT * (all columns)**: 192.9 µs (Limbo) / 184.4 µs (SQLite)
- **SELECT first 3 columns**: 128.6 µs (Limbo) / Not captured (SQLite)

### 100 Column Table Results
- **SELECT * (all columns)**: 457 µs ⚠️ **Critical benchmark - regression limit set at 480 µs (5%)**
- **SELECT first 3 columns**: 286 µs
- **SELECT sparse columns**: 294 µs
- **SELECT last column**: 293 µs

### Key Baseline Observations
1. **Eager Parsing Overhead Identified**:
   - With 100 columns, SELECT * takes 457 µs
   - SELECT 3 columns takes 286 µs (62% of full time)
   - SELECT 1 column takes 293 µs (64% of full time)
   - This confirmed that most time was spent parsing ALL columns even when only a few were needed

2. **Performance Improvement Targets**:
   - SELECT first 3 columns: ~50-70% improvement expected (286 µs → ~100-140 µs)
   - SELECT sparse columns: ~60-80% improvement expected (294 µs → ~60-120 µs)
   - SELECT last column: ~70-85% improvement expected (293 µs → ~45-90 µs)
   - SELECT * (all columns): <5% regression acceptable (457 µs → <480 µs)

## Mid-Development Benchmark Comparison

These measurements were taken during development to verify the implementation status and ensure no regressions.

### Performance Comparison Table

#### 10 Column Table (users)
| Query | Baseline | Mid-Dev | Change | Status |
|-------|----------|---------|--------|--------|
| SELECT * | 17.9 µs | 17.3 µs | -3.4% | ✅ Slightly improved |
| SELECT first 3 | 10.7 µs | 10.4 µs | -2.8% | ✅ Slightly improved |
| SELECT sparse (id, city, age) | - | 9.8 µs | - | ✅ Good baseline |

#### 50 Column Table
| Query | Baseline | Mid-Dev | Change | Status |
|-------|----------|---------|--------|--------|
| SELECT * | 192.9 µs | 194.5 µs | +0.8% | ✅ Within variance |
| SELECT first 3 | 128.6 µs | 129.7 µs | +0.9% | ✅ Within variance |

#### 100 Column Table
| Query | Baseline | Mid-Dev | Change | Status |
|-------|----------|---------|--------|--------|
| SELECT * | 457 µs | 458.2 µs | +0.3% | ✅ Well within 5% limit |
| SELECT first 3 | 286 µs | 282.5 µs | -1.2% | ✅ Slightly improved |

### Mid-Development Findings
1. **Lazy Parsing Not Yet Active**: Results confirmed lazy parsing was not enabled at this point
   - SELECT 3 columns from 100-column table: 282.5 µs (still 62% of SELECT * time)
   - This matched the expected eager parsing behavior

2. **Stable Performance**: All benchmarks within normal variance (±3%)
   - No regressions detected
   - SELECT * performance stable and ready for lazy parsing implementation

## Benchmark Infrastructure Improvements

During development, several improvements were made to the benchmarking infrastructure:

### Naming Conventions Aligned
- Renamed functions to match existing patterns:
  - `bench_lazy_parsing_column_access` → `bench_execute_lazy_parsing`
  - Updated benchmark group names for consistency

### Code Organization
- Split benchmarks into logical groups:
  - `bench_execute_lazy_parsing_users_table`: Tests with existing users table
  - `bench_execute_lazy_parsing_wide_tables`: Tests with generated wide tables

### Technical Improvements
- Replaced database copying with read-only connections to avoid file locking
- Reduced wide table tests from 4 sizes (10, 50, 100, 200) to 2 (50, 100) for efficiency
- Updated scripts to use new naming patterns

## Historical Context

This lazy parsing implementation was designed to address a fundamental performance issue in Limbo where all columns in a record were parsed eagerly, even when only a subset was needed. The benchmarks documented here guided the development process and helped ensure the implementation met its performance goals while avoiding regressions.

The development process included:
1. Establishing clear baselines and regression limits
2. Building robust benchmarking infrastructure
3. Iterative implementation with continuous performance monitoring
4. Bug fixes and optimizations based on benchmark findings

For the final implementation results and performance bug analysis, see `performance-bug-analysis.md`.