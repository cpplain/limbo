# Executive Summary: SQLite's Wide Table Optimizations

## Key Finding

SQLite's 96% performance advantage on selective queries (3 columns from 100) doesn't come from lazy record parsing as initially suspected, but from **lazy column header parsing** combined with intelligent caching.

## How SQLite Actually Works

1. **SQLite does NOT defer value deserialization** - it parses values immediately when requested
2. **SQLite DOES defer header parsing** - it only parses column headers up to the requested column
3. **SQLite caches parsing results** - offsets and types are cached in the cursor for reuse

## Why Limbo's Lazy Parsing Attempts Failed

The team tried to implement the wrong optimization:
- Attempted to defer value parsing (expensive in Limbo due to `RefValue` → `Value` conversion)
- Over-engineered with complex state management
- Missed that SQLite's advantage comes from header caching, not value deferral

## The Real Bottleneck

Limbo already has column projection (`column_mask`) but lacks:
1. **Header caching between column accesses**
2. **Optimized VM instruction paths for column access**
3. **Query-level optimizations for selective queries**

## Recommended Path Forward

### Don't Do
- Don't implement SQLite-style lazy value parsing (architecture mismatch)
- Don't add complex state management for deferred parsing

### Do Instead
1. **Add header caching** - Cache serial types and offsets in BTreeCursor
2. **Optimize VM hot path** - Fast path for common Column instruction patterns
3. **Batch column access** - Recognize and optimize sequential column reads
4. **SIMD header parsing** - Vectorize varint decoding

## Why This Will Work

- Limbo's eager parsing is already 40% faster than SQLite on 100-column tables
- The optimizations above attack the actual bottleneck (repeated header parsing)
- They align with Limbo's architecture rather than fighting it
- Combined impact could close most of the 96% gap on selective queries

## Bottom Line

SQLite's optimization is simpler than expected - it's just smart caching of metadata, not complex lazy evaluation. Limbo can achieve similar performance by adding header caching and optimizing the VM path, without the architectural changes that the previous lazy parsing attempts required.