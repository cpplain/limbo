# Lazy Parsing Benchmark Integration Strategy

## Overview

This document outlines strategies for integrating the lazy parsing benchmarks into the main Limbo benchmark suite once the implementation is ready for merging.

## Current Situation

### Main Benchmarks
- Located in `core/benches/benchmark.rs`
- All use the same `testing.db` file
- Use `EXCLUSIVE` locking mode for SQLite comparisons
- No conflicts because they share the same database

### Lazy Parsing Benchmarks
- Currently in `issue-30-lazy-parsing/benchmarks/`
- Create temporary databases for wide table testing
- Experience locking conflicts when SQLite tries to access Limbo-locked databases

## Integration Options

### Option 1: Pre-built Test Databases (Recommended)
Create wide table databases during build/setup phase:

```bash
# In testing/ directory
testing/
├── testing.db          # Existing
├── wide_table_50.db    # New
├── wide_table_100.db   # New
└── wide_table_200.db   # New
```

**Pros:**
- Follows existing pattern of using pre-built test data
- No runtime database creation overhead
- No locking conflicts
- Consistent test data across runs

**Cons:**
- Requires setup script or build step
- Increases repository size slightly

**Implementation:**
1. Create script to generate wide table databases
2. Add to Makefile or build process
3. Update benchmarks to use pre-built databases
4. No need for database copying

### Option 2: Separate Benchmark Binary
Keep lazy parsing benchmarks as a separate binary:

```toml
[[bench]]
name = "benchmark"
harness = false

[[bench]]
name = "lazy_parsing_benchmark"
harness = false
```

**Pros:**
- Complete isolation
- Can use different approaches without conflicts
- Easy to run separately

**Cons:**
- Some code duplication
- Two places to maintain benchmarks

### Option 3: Runtime Database Isolation
Continue using temporary databases with improved isolation:

```rust
// Close Limbo connection before SQLite benchmarks
drop(limbo_conn);
let sqlite_conn = rusqlite::Connection::open(db_path).unwrap();
```

**Pros:**
- No pre-built databases needed
- Flexible for different configurations

**Cons:**
- Can't interleave Limbo/SQLite benchmarks
- More complex benchmark structure

### Option 4: Modify rusqlite_open (Not Recommended)
Change the existing function to use read-only mode:

**Pros:**
- Solves all locking issues

**Cons:**
- Changes existing benchmark behavior
- May affect benchmark results

## Recommended Approach

**Phase 1 (Pre-merge):**
- Keep benchmarks in `issue-30-lazy-parsing/benchmarks/`
- Use for development and testing
- Share results with maintainers for feedback

**Phase 2 (Integration):**
1. Implement Option 1 (pre-built databases)
2. Add wide table generation to build process
3. Integrate benchmarks into main `benchmark.rs`
4. Follow existing naming patterns
5. Ensure CI compatibility

**Phase 3 (Post-merge):**
- Monitor for regressions
- Add to CI benchmark suite
- Document expected performance characteristics

## Implementation Checklist

- [ ] Create wide table generation script
- [ ] Add to Makefile: `make generate-test-databases`
- [ ] Update `.gitignore` for generated databases (if not committed)
- [ ] Modify benchmarks to use pre-built databases
- [ ] Test with both `cargo bench` and CI environment
- [ ] Document in CONTRIBUTING.md

## Example Integration Code

```rust
// In core/benches/benchmark.rs

fn bench_lazy_parsing_impact(criterion: &mut Criterion) {
    // Use pre-built wide tables from testing/
    let tables = vec![
        ("testing/testing.db", 10),          // Existing
        ("testing/wide_table_50.db", 50),    // Pre-built
        ("testing/wide_table_100.db", 100),  // Pre-built
    ];
    
    for (db_path, num_columns) in tables {
        // Benchmark code here
        // No database creation needed
        // No locking conflicts
    }
}
```

## Questions for Maintainers

1. Is adding pre-built test databases acceptable?
2. Should wide table benchmarks be in main suite or separate?
3. Any preference on database sizes to test?
4. Should benchmarks be gated by a feature flag initially?

## Next Steps

1. Get maintainer feedback on preferred approach
2. Implement chosen solution
3. Submit PR with:
   - Lazy parsing implementation
   - Integrated benchmarks
   - Documentation updates