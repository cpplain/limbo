# Wide Table Performance Optimization

## 🎯 Start Here

**Engineers**: Read [IMPLEMENTATION_GUIDE.md](./IMPLEMENTATION_GUIDE.md) - it has everything you need to implement the optimizations.

## What This Is

We're fixing Limbo's performance on wide tables with selective queries. Currently, Limbo is 96% slower than SQLite when selecting 3 columns from a 100-column table.

## Directory Structure

```
📁 issue-30-lazy-parsing/
  📄 README.md                    # You are here
  📄 IMPLEMENTATION_GUIDE.md      # ⭐ START HERE - Implementation tasks
  📁 benchmarks/                  # Performance testing tools
  📄 CLEANUP_PROPOSAL.md          # How we'll reorganize these docs
  📄 (20+ other files)            # Historical context - IGNORE for now
```

## For Engineers

1. **Read**: [IMPLEMENTATION_GUIDE.md](./IMPLEMENTATION_GUIDE.md) (10 minutes)
2. **Run**: Benchmarks to see current performance
3. **Implement**: Start with Task 1 (Header Caching)
4. **Test**: Verify improvements with benchmarks
5. **Ship**: PR when you hit 20%+ improvement

## For Archaeologists

If you need historical context about failed attempts:
- `CONSOLIDATED_SUMMARY.md` - Why lazy parsing failed
- `sqlite-optimization-analysis.md` - What SQLite actually does
- Other files document various failed approaches

## TL;DR

SQLite caches column header metadata. We don't. Adding header caching should give us 20-40% improvement on selective queries.

---

**Questions?** The implementation guide has answers. If not, ask on Slack #performance.