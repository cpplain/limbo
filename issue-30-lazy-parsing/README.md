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
- Feature flag `LAZY_PARSING_ENABLED` for safe rollout
- Integration with Column opcode
- **Critical bug fixes**:
  - Cache invalidation on all cursor movements
  - Eliminated performance-killing payload copy
  - Fixed sequential access detection logic
- Comprehensive unit tests passing
- All existing tests pass with no regressions

### 📋 Next Steps
1. Enable `LAZY_PARSING_ENABLED` flag and validate
2. Run performance benchmarks
3. Document performance improvements
4. Consider extending to index operations

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