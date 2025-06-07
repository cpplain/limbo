# Documentation Cleanup Proposal

## Current Problem
- 20+ documentation files
- Most document failed attempts
- Engineers get lost in historical context
- Unclear what's actionable vs. what's historical

## Proposed Structure

```
issue-30-lazy-parsing/
├── README.md                    # Start here - clear roadmap
├── IMPLEMENTATION_GUIDE.md      # The ONE file engineers need
├── benchmarks/                  # Keep benchmark tools
│   └── (existing benchmark code)
└── archive/                     # Historical context (if needed)
    ├── failed-attempts/
    │   ├── lazy-parsing-attempts.md
    │   ├── bitmap-optimization.md
    │   └── sparse-record-analysis.md
    └── analysis/
        ├── original-investigation.md
        └── performance-measurements.md
```

## What Goes Where

### IMPLEMENTATION_GUIDE.md (One consolidated file)
1. **The Problem** (3 sentences max)
   - Limbo is 96% slower than SQLite on selective queries (3 cols from 100)
   - SQLite uses header caching, not lazy value parsing
   - We need to add header caching and optimize the VM path

2. **What NOT To Do** (bullets)
   - Don't implement lazy value parsing
   - Don't add complex state management
   - Don't try to change RefValue/Value architecture

3. **Implementation Plan** (prioritized)
   - Task 1: Header Caching in BTreeCursor
   - Task 2: Batch Column Access
   - Task 3: SIMD Varint Decoding
   - Task 4: VM Fast Path

4. **Code Examples** (actual code to copy/paste)

5. **Success Metrics**
   - Benchmark command
   - Expected improvements
   - How to measure

### Archive (Move everything else here)
- All historical attempts
- All analysis documents
- All "lessons learned"
- Keep for reference but out of the way

## Benefits
1. **One file to rule them all** - Engineers open IMPLEMENTATION_GUIDE.md and start coding
2. **No confusion** - Clear separation between "do this" and "historical context"
3. **Preserves history** - Nothing deleted, just organized
4. **Fast onboarding** - New engineers can understand the task in 10 minutes

## Next Steps
1. Create the new structure
2. Consolidate actionable content into IMPLEMENTATION_GUIDE.md
3. Move historical docs to archive/
4. Update README.md with simple navigation