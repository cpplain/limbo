# Performance Bug Analysis

## Critical Issue: Payload Copy in get_column_lazy()

The lazy parsing implementation has a critical performance bug that defeats its purpose. In `get_column_lazy()`, the entire payload is being copied with `to_vec()` before parsing column headers:

```rust
let payload_slice = {
    let record = self.get_immutable_record();
    let payload = record.as_ref().unwrap().get_payload();
    // Create a temporary slice to avoid borrow conflicts
    payload.to_slice().to_vec()  // <-- THIS IS THE PROBLEM
};
```

### Why This Happens

1. `get_immutable_record()` returns a `RefMut<Option<ImmutableRecord>>` from a RefCell
2. We need to pass mutable references to `self.column_types` and `self.column_offsets` to `parse_columns_up_to()`
3. Rust's borrow checker prevents us from having both:
   - An immutable borrow of self (through the record)
   - Mutable borrows of self fields (column_types, column_offsets)

### Impact

- Every column access copies the entire record payload
- For a 100-column table, this means 100 copies for SELECT *
- This explains the 119% regression on 10-column tables
- The overhead completely dominates any savings from lazy parsing

### Potential Solutions

1. **Restructure the data**: Move parsing state out of the cursor into a separate struct
2. **Use unsafe code**: Carefully manage the borrows with unsafe blocks
3. **Cache the payload**: Store a copy once per record movement, not per column
4. **Redesign the API**: Change parse_columns_up_to to return the arrays instead of mutating

### Recommended Fix

The most practical short-term fix is option 3: cache the payload once when moving to a new record, rather than copying it on every column access. This would reduce the overhead from O(n) copies to O(1) copy per record.

However, the long-term solution should be option 1: restructure the data to avoid the borrow checker conflict entirely.