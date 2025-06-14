# Lazy Record Parsing - Developer TODO List

## 🚨 Critical Performance Fixes (Do First!)

### 1. Fix Memory Copy Issue
**File**: `core/types.rs` 
**Function**: `init_lazy()`
**Problem**: `self.payload = payload.to_vec()` creates unnecessary copy
**Fix**: Use `Arc<[u8]>` or lifetime-based reference
```rust
// Change from:
self.payload = payload.to_vec();

// To:
self.payload = Arc::from(payload);  // or use lifetime parameter
```

### 2. Add Lazy Parsing Heuristics  
**File**: `core/storage/sqlite3_ondisk.rs`
**Function**: `read_record()`
**Problem**: Lazy parsing applied to ALL records
**Fix**: Only use for records with >8 columns and >256 byte payloads
```rust
let should_use_lazy = column_count > 8 && payload_size > 256;
if should_use_lazy {
    // lazy path
} else {
    // eager path
}
```

### 3. Remove Sorter Pre-Parsing
**File**: `core/vdbe/sorter.rs`
**Function**: `sort_main()`
**Problem**: Pre-parses all columns before sorting
**Fix**: DELETE the entire pre-parsing loop:
```rust
// DELETE THIS:
for record in &mut self.records {
    for i in 0..self.key_len {
        let _ = record.parse_column(i);
    }
}
```

### 4. Fix Benchmark Setup
**Commands**:
```bash
# Copy benchmark to core
cp issue-30-lazy-record-parsing/benchmarks/record_parsing_benchmark.rs core/benches/

# Add to core/Cargo.toml:
[[bench]]
name = "record_parsing_benchmark"
harness = false

# Run with lazy parsing enabled:
cargo bench --bench record_parsing_benchmark --features lazy_parsing
```

## 🔧 Optimization Fixes

### 5. Increase Parse-Remaining Threshold
**File**: `core/types.rs`
**Function**: `should_parse_remaining()`
**Change**: 50% → 75%
```rust
parsed > (total_columns as usize * 3 / 4)  // was: / 2
```

### 6. Eliminate Comparison Allocations
**File**: `core/vdbe/sorter.rs`
**Problem**: Creates new Vecs for every comparison
**Fix**: Compare columns directly without intermediate collections
```rust
// Instead of collecting into Vec, compare directly:
for i in 0..self.key_len {
    let a_val = a.get_column_lazy(i);
    let b_val = b.get_column_lazy(i);
    match compare_values(a_val, b_val, ...) {
        Ordering::Equal => continue,
        other => return other,
    }
}
```

### 7. Complete VDBE Integration
**File**: `core/vdbe/execute.rs`
**Problem**: Some paths return Null instead of lazy parsing
**Fix**: Implement proper lazy column access for all cursor types

## 📊 Testing Checklist

- [ ] Run benchmarks WITHOUT lazy parsing (baseline)
- [ ] Run benchmarks WITH lazy parsing 
- [ ] Test scenarios:
  - [ ] 10% column selectivity (should be 90% faster)
  - [ ] COUNT(*) on 50+ column tables (should be 95%+ faster)
  - [ ] ORDER BY with partial columns (should be 20%+ faster)
  - [ ] SELECT * (should be within 5% of baseline)
- [ ] Memory profiling with heaptrack
- [ ] Run full test suite with lazy parsing enabled

## 🎯 Expected Outcomes

After these fixes:
- Selective queries: 80-90% performance improvement
- COUNT(*): 95%+ improvement 
- Memory usage: 30-50% reduction
- No regression for SELECT *

## 🔍 Quick Validation Commands

```bash
# Check if lazy parsing is working:
RUST_LOG=trace cargo test --features lazy_parsing 2>&1 | grep "lazy"

# Memory usage comparison:
/usr/bin/time -v cargo bench --bench record_parsing_benchmark

# Profile with perf:
echo -1 | sudo tee /proc/sys/kernel/perf_event_paranoid
cargo bench --bench record_parsing_benchmark -- --profile-time=5
```