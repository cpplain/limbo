# Implementation Plan for Lazy Record Parsing

## Overview
Implement SQLite-style lazy record parsing to improve performance for queries that don't access all columns.

## Design Decisions

### 1. Follow SQLite's Proven Approach
- **Always lazy parse** - no adaptive/hybrid approach
- Parse record header incrementally as columns are accessed
- **Critical**: Optimize for sequential access pattern (common in SELECT *)
- **Two-level caching**:
  - Metadata (serial types, offsets) - always cached once parsed
  - Values - cached selectively based on size and access patterns
- **Persistent parsing state**: Maintain state across multiple Column operations on same cursor position

### 2. Modified Data Structures

```rust
// core/types.rs
pub struct ImmutableRecord {
    payload: Vec<u8>,
    
    // Lazy parsing state
    header_size: usize,              // Total header size in bytes
    parsed_up_to: usize,             // Number of columns parsed so far
    header_offset: usize,            // Current position in header
    serial_types: Vec<SerialType>,   // Serial types (parsed incrementally)
    column_offsets: Vec<usize>,      // Byte offsets to each column
    
    // Cached values (None = not yet parsed)
    values: Vec<Option<RefValue>>,
    
    // Performance optimization
    last_accessed_column: Option<usize>, // For sequential access detection
    is_sequential_access: bool,          // Optimize for SELECT *
    
    recreating: bool,
}

// core/storage/btree.rs - Add to BTreeCursor
pub struct BTreeCursor {
    // ... existing fields ...
    
    // Parsing state validity
    parsing_state_valid: bool,  // Reset on cursor movement
}
```

### 3. Implementation Strategy

#### Phase 1: Core Infrastructure
1. Modify `ImmutableRecord` to support lazy parsing
2. Split `read_record()` into:
   - `read_record_header()` - parse header size only
   - `parse_column_metadata()` - parse serial types/offsets up to column N
   - `parse_column_value()` - deserialize specific column value

#### Phase 2: Column Opcode
1. Update Column opcode to call parsing functions on-demand
2. Implement caching for parsed values
3. Add sequential access optimization

#### Phase 3: Index Handling
1. Identify which columns are needed for index comparisons
2. Parse only those columns when needed
3. Refactor comparison functions to work with lazy records

## Implementation Steps

### Step 1: Create New Parsing Functions
```rust
// core/storage/sqlite3_ondisk.rs

pub fn read_record_lazy(payload: &[u8], record: &mut ImmutableRecord) -> Result<()> {
    record.invalidate();
    record.start_serialization(payload);
    
    // Parse only header size
    let (header_size, nr) = read_varint(payload)?;
    record.header_size = header_size as usize;
    record.header_offset = nr;
    record.parsed_up_to = 0;
    
    Ok(())
}

pub fn parse_up_to_column(record: &mut ImmutableRecord, column: usize) -> Result<()> {
    if record.parsed_up_to > column {
        return Ok(()); // Already parsed
    }
    
    // Continue parsing from where we left off
    let payload = record.get_payload();
    let mut pos = record.header_offset;
    
    // Parse serial types and calculate offsets
    while record.parsed_up_to <= column && pos < record.header_size {
        let (serial_type, nr) = read_varint(&payload[pos..])?;
        record.serial_types.push(serial_type.into());
        
        if record.parsed_up_to > 0 {
            let prev_offset = record.column_offsets[record.parsed_up_to - 1];
            let prev_type = record.serial_types[record.parsed_up_to - 1];
            record.column_offsets.push(prev_offset + prev_type.size());
        } else {
            record.column_offsets.push(record.header_size);
        }
        
        pos += nr;
        record.parsed_up_to += 1;
    }
    
    record.header_offset = pos;
    Ok(())
}

pub fn get_column_value(record: &mut ImmutableRecord, column: usize) -> Result<RefValue> {
    // Check cache first
    if let Some(Some(value)) = record.values.get(column) {
        return Ok(value.clone());
    }
    
    // Ensure metadata is parsed
    parse_up_to_column(record, column)?;
    
    // Parse the actual value
    let offset = record.column_offsets[column];
    let serial_type = record.serial_types[column];
    let payload = record.get_payload();
    
    let (value, _) = read_value(&payload[offset..], serial_type)?;
    
    // Cache the value
    record.values[column] = Some(value.clone());
    
    Ok(value)
}
```

### Step 2: Update Column Opcode
```rust
// core/vdbe/execute.rs

insn::Column { column } => {
    let cursor = &mut cursors[&self.reg(REG_CURSOR)];
    let record = cursor
        .get_immutable_record_mut()
        .ok_or(LimboError::InternalError("no record".into()))?;
    
    // Detect sequential access pattern
    if let Some(last) = record.last_accessed_column {
        if *column == last + 1 {
            record.is_sequential_access = true;
        } else if *column < last {
            record.is_sequential_access = false;
        }
    }
    record.last_accessed_column = Some(*column);
    
    // If sequential and next column, parse ahead
    if record.is_sequential_access && record.parsed_up_to == *column {
        // Parse ahead multiple columns to amortize overhead
        let parse_ahead = std::cmp::min(*column + 8, record.estimate_column_count());
        parse_up_to_column(record, parse_ahead)?;
    }
    
    // Get the column value
    let value = get_column_value(record, *column)?;
    trace_cursor(self.trace.as_ref(), &value);
    self.columns_fetched_count += 1;
    Ok(Response::Column(value))
}
```

### Step 3: Handle Index Comparisons
```rust
// core/storage/btree.rs

impl BTreeCursor {
    // In get_next_record(), for index cells:
    fn prepare_record_for_comparison(&mut self, key_info: &KeyInfo) -> Result<()> {
        if !self.parsing_state_valid {
            // Reset parsing state on cursor movement
            self.reusable_immutable_record.invalidate_parsing_state();
            self.parsing_state_valid = true;
        }
        
        // For index comparisons, eagerly parse key columns
        if let Some(record) = &mut self.reusable_immutable_record {
            let key_columns = key_info.columns.len();
            // Parse all key columns at once for efficiency
            parse_up_to_column(record, key_columns - 1)?;
            
            // Pre-parse values for small key columns (e.g., integers)
            for i in 0..key_columns {
                if record.serial_types[i].is_small_value() {
                    get_column_value(record, i)?;
                }
            }
        }
        Ok(())
    }
    
    // Reset parsing state on cursor movement
    fn invalidate_parsing_state(&mut self) {
        self.parsing_state_valid = false;
    }
}
```

## Testing Strategy

1. **Unit Tests**
   - Test incremental header parsing
   - Test column value caching
   - Test sequential access detection and optimization
   - Test cursor movement and state invalidation
   - Test edge cases (empty records, single column, corrupt headers)
   - Test overflow page handling with lazy parsing

2. **Performance Tests**
   - **Critical**: Benchmark SELECT * to ensure <5% regression
   - Benchmark SELECT with 1-3 columns from 100+ column table
   - Benchmark mixed access patterns (random vs sequential)
   - Benchmark index scans with lazy parsing
   - Memory usage comparison for large TEXT/BLOB columns

3. **Compatibility Tests**
   - Ensure all existing tests pass
   - Add tests for lazy parsing edge cases
   - Fuzz testing with corrupted record headers
   - Thread safety tests if applicable

## Risk Mitigation

1. **Performance Regression**
   - **Sequential Access**: Parse-ahead strategy for SELECT * patterns
   - **Small Records**: Consider eager parsing for records < 100 bytes
   - **Hot Path**: Inline critical parsing functions
   - **Benchmarking**: Set up benchmarks BEFORE implementation

2. **Correctness Issues**
   - Extensive testing with fuzzing
   - Compare results with current implementation
   - Add debug assertions for parsing invariants
   - Careful handling of varint edge cases

3. **Memory Safety**
   - Ensure payload lifetime management with RefValue
   - Test with valgrind/sanitizers
   - Careful bounds checking in parsing functions
   - No unsafe code in initial implementation

4. **Complexity Management**
   - Feature flag for gradual rollout
   - Clear separation between lazy and eager code paths
   - Extensive documentation of parsing state machine

## Success Metrics

1. **Performance Improvements**
   - 20-50% faster for selective column queries on wide tables
   - <5% regression for SELECT * queries
   - Reduced memory usage for large TEXT/BLOB columns

2. **Code Quality**
   - All tests passing
   - No memory leaks or safety issues
   - Clean, maintainable implementation

## Timeline Estimate

- **Week 0**: Set up comprehensive benchmarking suite
- **Week 1-2**: Core infrastructure and parsing functions
- **Week 3**: Column opcode with sequential optimization
- **Week 4**: Index handling and cursor state management
- **Week 5**: Performance optimization and tuning
- **Week 6**: Testing, fuzzing, and edge case handling

## Implementation Order (Critical Path)

1. **Benchmarking Infrastructure** (Must be first!)
   - Create test database with 100+ column table
   - Benchmark current implementation baseline
   - Set up automated regression detection

2. **Basic Lazy Parsing**
   - Implement core parsing functions
   - Add to ImmutableRecord without breaking existing code

3. **Sequential Access Optimization**
   - This is make-or-break for the feature
   - Must show <5% regression on SELECT *

4. **Full Integration**
   - Update Column opcode
   - Handle cursor state management
   - Index comparison updates

5. **Polish and Optimization**
   - Profile and optimize hot paths
   - Handle edge cases discovered in testing