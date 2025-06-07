# New Implementation Plan - Performance Optimizations for Limbo

## Executive Summary

Based on the lessons learned from the lazy parsing experiment, this plan outlines a pragmatic approach to optimizing Limbo's query performance. Instead of implementing complex lazy parsing that doesn't fit Limbo's architecture, we'll focus on targeted optimizations that leverage Limbo's strengths.

## Goals

1. **Maintain Limbo's simplicity** - No over-engineered solutions
2. **Improve selective query performance** - Close the 96% gap with SQLite
3. **Preserve eager parsing speed** - Keep the 19% advantage on small tables
4. **Add zero-copy optimizations** - Reduce unnecessary allocations

## Phase 1: Projection-Based Parsing (Highest Priority)

### Objective
Parse only the columns needed by the query, achieving lazy parsing benefits without complexity.

### Implementation Steps

#### 1.1 Query Analysis Enhancement
```rust
// In translate/select.rs
struct ProjectionInfo {
    needed_columns: BitVec,
    access_pattern: AccessPattern,
    has_aggregates: bool,
}

impl SelectTranslator {
    fn analyze_projection(&self, select: &Select) -> ProjectionInfo {
        // Identify which columns are actually used
        // Track if we need all columns (SELECT *)
        // Detect aggregate functions that need full scan
    }
}
```

#### 1.2 Column Set Propagation
- Add column requirements to VM instructions
- Pass projection info through cursor operations
- Update `OpenRead` instruction to include column mask

#### 1.3 Selective Record Parsing
```rust
// In storage/btree.rs
impl BTreeCursor {
    fn read_record_projected(
        &mut self, 
        columns_needed: &BitVec
    ) -> Result<Record> {
        let header = self.parse_record_header()?;
        let mut values = vec![Value::Null; header.len()];
        
        // Only parse columns we need
        for (idx, needed) in columns_needed.iter().enumerate() {
            if needed {
                values[idx] = self.parse_column_value(idx, &header)?;
            }
        }
        
        Ok(Record { values })
    }
}
```

### Success Metrics
- 50-70% performance improvement on selective queries
- <5% overhead on full table scans
- No regression on existing workloads

### Timeline: 2-3 weeks

## Phase 2: SIMD Optimizations for Header Parsing

### Objective
Accelerate varint decoding and header parsing using SIMD instructions.

### Implementation Steps

#### 2.1 SIMD Varint Decoder
```rust
// In storage/sqlite3_ondisk.rs
#[cfg(target_arch = "x86_64")]
mod simd {
    use std::arch::x86_64::*;
    
    pub unsafe fn decode_varints_simd(
        data: &[u8], 
        output: &mut [u64]
    ) -> usize {
        // Batch decode 4-8 varints at once
        // Use PSHUFB for byte manipulation
        // Fall back to scalar for edge cases
    }
}
```

#### 2.2 Parallel Header Processing
- Process multiple column headers simultaneously
- Vectorize type determination
- Batch offset calculations

### Success Metrics
- 3-4x speedup on header parsing
- 15-25% overall improvement on wide tables
- Works on all x86_64 platforms

### Timeline: 3-4 weeks

## Phase 3: Zero-Copy Architecture

### Objective
Eliminate unnecessary allocations by referencing page buffers directly.

### Implementation Steps

#### 3.1 Extend VM Register Types
```rust
// In vdbe/mod.rs
pub enum Register {
    Value(Value),
    RefValue(RefValue<'static>), // New variant
    // ... existing variants
}

impl Register {
    fn materialize(&mut self) -> Result<&Value> {
        match self {
            Register::RefValue(ref_val) => {
                *self = Register::Value(ref_val.to_owned());
                self.as_value()
            }
            Register::Value(val) => Ok(val),
            // ...
        }
    }
}
```

#### 3.2 Deferred Materialization
- Only convert RefValue → Value when necessary
- Keep references for comparisons and projections
- Materialize for aggregations and outputs

#### 3.3 Buffer Lifetime Management
- Pin page buffers during query execution
- Use reference counting for safety
- Clear references after query completion

### Success Metrics
- 20-30% reduction in allocations
- 10-15% performance improvement
- No safety issues or lifetime errors

### Timeline: 4-6 weeks

## Phase 4: Large BLOB/TEXT Lazy Loading

### Objective
Defer loading of large variable-length data until accessed.

### Implementation Steps

#### 4.1 Size-Based Deferral
```rust
// In storage/sqlite3_ondisk.rs
const LAZY_BLOB_THRESHOLD: usize = 1024;

fn parse_value(serial_type: u64, data: &[u8]) -> RefValue {
    let size = get_value_size(serial_type);
    
    if serial_type.is_blob() && size > LAZY_BLOB_THRESHOLD {
        return RefValue::DeferredBlob {
            page_id: current_page,
            offset: current_offset,
            size,
        };
    }
    
    // Parse normally for small values
    parse_value_immediate(serial_type, data)
}
```

#### 4.2 On-Demand Loading
- Load deferred BLOBs when accessed
- Cache loaded values for repeated access
- Clear cache after query completion

### Success Metrics
- 50-80% memory reduction for BLOB-heavy queries
- <2% overhead for normal queries
- Improved performance on large datasets

### Timeline: 2 weeks

## Phase 5: Testing and Integration

### Objective
Integrate new optimizations and ensure robust testing.

### Implementation Steps

#### 5.1 Integration
- Integrate new optimizations incrementally
- Add feature flags for experimental features
- Ensure backward compatibility

#### 5.2 Performance Validation
- Run comprehensive benchmarks
- Compare against SQLite and baseline
- Ensure no regressions

#### 5.3 Documentation
- Update architecture docs
- Add performance tuning guide
- Document new APIs

### Timeline: 1 week

## Risk Mitigation

### Technical Risks
1. **SIMD portability** - Provide scalar fallbacks
2. **Lifetime complexity** - Extensive testing, use unsafe sparingly
3. **Integration issues** - Incremental rollout with feature flags

### Performance Risks
1. **Overhead on small queries** - Heuristics to disable optimizations
2. **Memory pressure** - Configurable thresholds
3. **Cache misses** - Profile and tune buffer sizes

## Alternative Approach: Focus on Eager Parsing

If the above proves too complex, fall back to optimizing eager parsing:

1. **Memory pooling** - Reuse allocations
2. **Batch processing** - Parse similar types together
3. **Inline parsing** - Reduce function call overhead
4. **Profile-guided optimization** - Focus on hot paths

## Success Criteria

### Performance Goals
- Close 50% of the performance gap with SQLite on selective queries
- Maintain current performance on full table scans
- Reduce memory usage by 30% on BLOB-heavy workloads

### Code Quality Goals
- No increase in complexity score
- All optimizations behind clean APIs
- Comprehensive test coverage

## Conclusion

This plan focuses on pragmatic optimizations that fit Limbo's architecture. By implementing projection-based parsing and targeted optimizations, we can achieve the benefits of lazy parsing without its complexity. The phased approach allows for incremental progress and early validation of each optimization's effectiveness.