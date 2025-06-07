#!/bin/bash
# Script to run lazy parsing benchmarks and capture results

set -e

echo "==================================="
echo "Lazy Parsing Benchmark Suite"
echo "==================================="
echo ""

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    echo "Error: Please run this script from the issue-30-lazy-parsing/benchmarks directory"
    exit 1
fi

# Function to run benchmarks and save results
run_benchmark() {
    local name=$1
    local timestamp=$(date +%Y%m%d_%H%M%S)
    local results_dir="results/${timestamp}_${name}"
    
    echo "Running benchmarks: $name"
    echo "Results will be saved to: $results_dir"
    echo ""
    
    # Run the benchmark
    cargo bench
    
    # Copy results
    mkdir -p "$results_dir"
    cp -r ../../target/criterion/* "$results_dir/" 2>/dev/null || true
    
    echo ""
    echo "Benchmark complete. HTML report available at:"
    echo "  $results_dir/report/index.html"
    echo ""
}

# Main execution
echo "1. Running baseline measurements (lazy parsing OFF)"
run_benchmark "baseline"

echo ""
echo "==================================="
echo "Benchmark Summary"
echo "==================================="
echo ""
echo "Key metrics to watch:"
echo "- 100-column SELECT *: Must stay under 480µs (5% regression limit)"
echo "- 100-column SELECT 3 cols: Should improve from ~286µs to ~100-140µs"
echo "- Compare limbo vs sqlite times to see relative performance"
echo ""
echo "To enable lazy parsing and re-run:"
echo "  1. Set LAZY_PARSING_ENABLED feature flag to true"
echo "  2. Run this script again"
echo "  3. Compare results between baseline and lazy runs"