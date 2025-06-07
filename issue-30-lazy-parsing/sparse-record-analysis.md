# Sparse Record Implementation Analysis

## What We Implemented

We implemented a sparse record representation that:
1. Added column mapping to ImmutableRecord (Option<Vec<Option<usize>>>)
2. Modified column access methods to use the mapping
3. Eliminated NULL placeholder creation in read_record_projected

## Results

**Performance REGRESSED**:
- 10 columns: +1% regression
- 50 columns: -1.7% (slight improvement)
- 100 columns: +2% regression

## Why It Failed

The sparse record approach failed because:

1. **RefValue::Null is essentially free** - It's just an enum discriminant, no heap allocation
2. **Column mapping adds real overhead**:
   - Vec<Option<usize>> allocation (100+ elements for wide tables)
   - Extra indirection on every column access
   - Option checking and unwrapping overhead

3. **Access pattern overhead**: Every column access now requires:
   ```rust
   mapping.get(idx).and_then(|pos| pos.and_then(|p| self.values.get(p)))
   ```
   Instead of simple:
   ```rust
   self.values.get(idx)
   ```

## Key Insight

We optimized the wrong thing. The cost isn't in storing NULL values (they're lightweight), but in:
1. **Parsing all serial types from the header**
2. **Varint decoding for every column**
3. **Reading through the entire header sequentially**

## Better Approach: Lazy Header Parsing

Instead of eliminating NULL storage, we should:
1. Skip varint decoding for columns we don't need
2. Jump directly to the serial types we care about
3. Reduce header parsing overhead

This targets the actual expensive operations (varint decoding) rather than the cheap ones (NULL storage).