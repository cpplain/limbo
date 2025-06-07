# Preservation Plan - Lazy Parsing Experiment

## Purpose

This document outlines what should be preserved from the lazy parsing experiment for future optimization efforts.

## Assets to Preserve

### 1. Benchmarking Infrastructure ✅
**Location**: `issue-30-lazy-parsing/benchmarks/`

**What it provides**:
- Comprehensive record parsing performance tests
- Wide table benchmarks (10, 50, 100, 200 columns)
- Comparison framework against SQLite
- Memory usage profiling capabilities

**How to use**:
```bash
cd issue-30-lazy-parsing/benchmarks
cargo bench

# For specific optimizations:
cargo bench --bench lazy_parsing -- --profile-time=5
```

**Future applications**:
- Testing projection-based parsing
- SIMD optimization experiments
- Zero-copy architecture validation
- Any record parsing optimization

### 2. Performance Insights 📊

**Key findings to remember**:
- Limbo's eager parsing is 19% faster than SQLite on small tables
- Limbo is 40% faster than SQLite on 100-column tables
- The 96% performance gap on selective queries isn't from lazy parsing
- Most real-world tables have <50 columns

**Optimization targets**:
- Selective queries: 50-70% improvement potential
- SIMD parsing: 3-4x header parsing speedup
- Zero-copy: 20-30% allocation reduction

### 3. Implementation Patterns to Avoid ❌

**Don't do**:
- Cache parsed values when parsing is cheap
- Copy payloads instead of referencing
- Add complex state management without clear benefit
- Force SQLite patterns onto different architectures

**Do instead**:
- Optimize the fast path (eager parsing)
- Use projection information from query planning
- Defer only truly expensive operations (>1KB BLOBs)
- Measure before optimizing

## Recommended Usage

### For Future Parsing Optimizations

1. **Start with benchmarks**:
   ```bash
   # Establish baseline
   cd benchmarks && cargo bench > baseline.txt
   
   # After changes
   cargo bench > results.txt
   diff baseline.txt results.txt
   ```

2. **Set regression limits**:
   - <5% regression on full table scans
   - >20% improvement required on selective queries
   - Memory usage should decrease or stay flat

3. **Use the test scenarios**:
   - `select_all_*` - Full table scan performance
   - `select_first_three_*` - Sequential partial access
   - `select_sparse_*` - Non-sequential access
   - `select_last_*` - Worst-case single column

### For Architecture Decisions

Before implementing any parsing optimization:

1. **Review LESSONS_LEARNED.md** - Understand what failed and why
2. **Check current baselines** - Limbo may already be fast enough
3. **Consider simpler alternatives** - Can query planning help instead?
4. **Prototype and measure** - Use the benchmark framework

## Migration Guide

When this experiment directory is eventually archived or removed:

1. **Move benchmarks** to `perf/record-parsing/`
2. **Extract key insights** to main architecture docs
3. **Update issue #30** with link to this final analysis
4. **Tag the commit** for historical reference

## Contact

For questions about this experiment or the preserved assets:
- Original issue: #30
- Final PR: #250 (reverted)
- Key commits: Listed in README.md