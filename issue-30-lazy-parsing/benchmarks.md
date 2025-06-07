# Record Parsing Benchmarks

This benchmark infrastructure tests different record parsing strategies on wide tables.

## What It Tests

Creates tables with 10, 50, 100, 200 columns and measures:
- **SELECT *** - Full table scan performance
- **SELECT first 3** - Sequential partial access
- **SELECT sparse** - Non-sequential column access  
- **SELECT last** - Single column at end

## Running Benchmarks

```bash
cd issue-30-lazy-parsing/benchmarks
cargo bench

# With profiling
cargo bench --bench lazy_parsing -- --profile-time=5
```

## Key Findings from Lazy Parsing Experiment

- SELECT * had 62% regression on 10-column tables
- No improvement on selective queries
- Limbo's eager parsing baseline is already 19% faster than SQLite

## Future Use

This benchmark framework can be reused for:
- Testing projection-based parsing
- SIMD optimization experiments
- Memory usage profiling
- Comparing different parsing strategies