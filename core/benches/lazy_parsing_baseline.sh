#!/bin/bash
# Script to capture baseline measurements for lazy parsing benchmarks

set -e

echo "Capturing baseline measurements for lazy parsing benchmarks..."
echo "============================================================="
echo ""
echo "This will run benchmarks for column access patterns on wide tables."
echo "The results will be used to detect regressions after implementing lazy parsing."
echo ""

# Run only the lazy parsing benchmarks
cargo bench -p limbo_core --bench benchmark -- lazy_parsing

echo ""
echo "Baseline measurements captured!"
echo ""
echo "Key metrics to watch:"
echo "1. SELECT * performance - MUST NOT regress more than 5%"
echo "2. SELECT first 3 columns - Should improve with lazy parsing"
echo "3. SELECT sparse columns - Should show significant improvement"
echo "4. SELECT last column - Best case for lazy parsing improvement"
echo ""
echo "Results are saved in target/criterion/lazy_parsing_*"