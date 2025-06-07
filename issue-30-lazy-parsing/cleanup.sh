#!/bin/bash

# Script to reorganize documentation for clarity
# Run from issue-30-lazy-parsing directory

echo "📁 Creating archive structure..."

# Create archive directories
mkdir -p archive/failed-attempts
mkdir -p archive/analysis
mkdir -p archive/historical

echo "📦 Archiving failed attempt documentation..."

# Move failed attempt docs
mv -f bitmap-optimization-results.md archive/failed-attempts/ 2>/dev/null || true
mv -f bitmap-optimization-summary.md archive/failed-attempts/ 2>/dev/null || true
mv -f sparse-record-analysis.md archive/failed-attempts/ 2>/dev/null || true
mv -f quick-win-implementation.md archive/failed-attempts/ 2>/dev/null || true
mv -f phase1-summary.md archive/failed-attempts/ 2>/dev/null || true

echo "📊 Archiving analysis documentation..."

# Move analysis docs
mv -f analysis.md archive/analysis/ 2>/dev/null || true
mv -f optimization-analysis-final.md archive/analysis/ 2>/dev/null || true
mv -f performance-analysis-and-recommendations.md archive/analysis/ 2>/dev/null || true
mv -f performance-bug-analysis.md archive/analysis/ 2>/dev/null || true
mv -f technical-details.md archive/analysis/ 2>/dev/null || true
mv -f sqlite-optimization-analysis.md archive/analysis/ 2>/dev/null || true

echo "📜 Archiving historical documentation..."

# Move historical docs
mv -f historical-benchmarks.md archive/historical/ 2>/dev/null || true
mv -f CONSOLIDATED_SUMMARY.md archive/historical/ 2>/dev/null || true
mv -f LESSONS_LEARNED.md archive/historical/ 2>/dev/null || true
mv -f PRESERVATION_PLAN.md archive/historical/ 2>/dev/null || true
mv -f action-plan.md archive/historical/ 2>/dev/null || true
mv -f optimization-recommendations.md archive/historical/ 2>/dev/null || true

echo "🔄 Updating main documentation..."

# Keep only the essential files at root
mv -f README_NEW.md README.md 2>/dev/null || true

# Remove now-redundant files that were consolidated
rm -f actionable-optimizations.md 2>/dev/null || true
rm -f executive-summary.md 2>/dev/null || true

echo "✅ Cleanup complete!"
echo ""
echo "New structure:"
echo "  📄 README.md                 - Start here"
echo "  📄 IMPLEMENTATION_GUIDE.md   - Everything engineers need"
echo "  📄 CLEANUP_PROPOSAL.md       - This cleanup plan"
echo "  📁 benchmarks/               - Performance testing"
echo "  📁 archive/                  - Historical context"
echo "      📁 failed-attempts/      - What didn't work"
echo "      📁 analysis/             - Deep investigations"
echo "      📁 historical/           - Project history"
echo ""
echo "🎯 Engineers should now read IMPLEMENTATION_GUIDE.md to get started!"