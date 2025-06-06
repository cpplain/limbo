# Lazy Record Parsing Investigation

This directory contains analysis and planning documents for implementing lazy record parsing in Limbo (Issue #30).

## Files

- **analysis.md** - Detailed analysis of the current implementation, SQLite's approach, and why PR #250 failed
- **implementation-plan.md** - Step-by-step plan for implementing lazy parsing in Limbo
- **baseline-results.md** - Performance measurements before implementing lazy parsing
- **implementation-summary.md** - Summary of the implementation work completed
- **critical-insights.md** - Key insights and lessons learned from the analysis
- **next-steps.md** - Current status and immediate next actions

## Implementation Status (January 2025)

### ✅ Completed
- Core lazy parsing infrastructure added to `BTreeCursor`
- Incremental header parsing functions implemented
- Sequential access optimization for SELECT * queries
- Feature flag `LAZY_PARSING_ENABLED` enabled
- Integration with Column opcode
- **Critical bug fixes**:
  - Cache invalidation on all cursor movements
  - Fixed payload copy bug (reduced from O(n) to O(1) copies)
  - Fixed sequential access detection logic
  - Added payload caching to avoid repeated allocations
- Comprehensive unit tests passing
- All existing tests pass with no regressions

### 📊 Performance Results
- **Functional**: All tests passing, no correctness issues
- **Performance**: Needs optimization
  - 100-column SELECT *: 4.3% regression (within 5% target)
  - 50-column SELECT *: 5.9% regression (exceeds 5% target)
  - Small tables show significant overhead (62% on 10-column tables)
  - No improvement yet on selective column queries

### 📋 Future Work
1. Profile and optimize hot paths
2. Consider hybrid approach for small tables
3. Verify sequential detection is working
4. Optimize state management and allocations
5. Consider extending to index operations

## Quick Summary

### The Problem
Limbo currently parses entire database records when the cursor moves to them, even if only a few columns are needed. This wastes CPU cycles and memory, especially for:
- Wide tables (many columns)
- Tables with large TEXT/BLOB values
- Queries that only select a few columns

### The Solution
Implement SQLite-style lazy parsing:
1. Parse only the record header initially
2. Parse column metadata (types, offsets) on-demand
3. Deserialize column values only when accessed
4. Cache parsed values for repeated access
5. Optimize for sequential column access (SELECT *)

### Expected Impact
- 20-50% performance improvement for selective queries on wide tables
- Reduced memory usage
- Better compatibility with SQLite's behavior

## References
- [Issue #30](https://github.com/tursodatabase/limbo/issues/30)
- [PR #250](https://github.com/tursodatabase/limbo/pull/250) (failed attempt)
- SQLite source: `src/vdbe.c` - OP_Column implementation