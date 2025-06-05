# Lazy Record Parsing Investigation

This directory contains analysis and planning documents for implementing lazy record parsing in Limbo (Issue #30).

## Files

- **analysis.md** - Detailed analysis of the current implementation, SQLite's approach, and why PR #250 failed
- **implementation-plan.md** - Step-by-step plan for implementing lazy parsing in Limbo

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