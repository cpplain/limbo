use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use limbo_core::{Database, PlatformIO, StepResult, IO};
use pprof::criterion::{Output, PProfProfiler};
use std::sync::Arc;
use tempfile::TempDir;

// Database configuration for benchmarks
const NUM_ROWS: usize = 100_000;
const TABLE_WIDTHS: &[usize] = &[10, 25, 50, 100];

fn setup_wide_table_database(num_columns: usize) -> (TempDir, String) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join(format!("wide_table_{}.db", num_columns));
    let db_path_str = db_path.to_str().unwrap().to_string();
    
    // Create database and table using rusqlite
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    
    // Build CREATE TABLE statement
    let mut columns = Vec::new();
    let mut types = Vec::new();
    for i in 0..num_columns {
        columns.push(format!("col_{}", i));
        match i % 4 {
            0 => types.push("INTEGER"),
            1 => types.push("REAL"),
            2 => types.push("TEXT"),
            _ => types.push("BLOB"),
        }
    }
    
    let create_sql = format!(
        "CREATE TABLE wide_table ({})",
        columns.iter().zip(types.iter())
            .map(|(col, typ)| format!("{} {}", col, typ))
            .collect::<Vec<_>>()
            .join(", ")
    );
    
    conn.execute(&create_sql, []).unwrap();
    
    // Create index on first column for WHERE clauses
    conn.execute("CREATE INDEX idx_col_0 ON wide_table(col_0)", []).unwrap();
    
    // Insert data in batches
    let insert_sql = format!(
        "INSERT INTO wide_table ({}) VALUES ({})",
        columns.join(", "),
        columns.iter().map(|_| "?").collect::<Vec<_>>().join(", ")
    );
    
    conn.execute("BEGIN TRANSACTION", []).unwrap();
    
    let mut stmt = conn.prepare(&insert_sql).unwrap();
    
    for row in 0..NUM_ROWS {
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        for col in 0..num_columns {
            match col % 4 {
                0 => values.push(((row * 100 + col) as i64).into()),
                1 => values.push(((row as f64) * 1.5 + col as f64).into()),
                2 => values.push(format!("text_{}_{}", row, col).into()),
                _ => values.push(vec![0u8; 20].into()), // Small blob
            }
        }
        stmt.execute(rusqlite::params_from_iter(values)).unwrap();
    }
    
    conn.execute("COMMIT", []).unwrap();
    
    (temp_dir, db_path_str)
}

fn bench_column_selectivity(c: &mut Criterion) {
    let enable_rusqlite = std::env::var("DISABLE_RUSQLITE_BENCHMARK").is_err();
    
    for &num_columns in TABLE_WIDTHS {
        let (_temp_dir, db_path) = setup_wide_table_database(num_columns);
        
        // Test different selectivity percentages
        for &pct in &[10, 25, 50, 100] {
            let num_cols_to_select = std::cmp::max(1, num_columns * pct / 100);
            
            let query = if pct == 100 {
                "SELECT * FROM wide_table WHERE col_0 < 1000".to_string()
            } else {
                let cols: Vec<String> = (0..num_cols_to_select)
                    .map(|i| format!("col_{}", i))
                    .collect();
                format!(
                    "SELECT {} FROM wide_table WHERE col_0 < 1000",
                    cols.join(", ")
                )
            };
            
            let bench_name = format!("selectivity_{}pct_{}cols", pct, num_columns);
            let mut group = c.benchmark_group(format!("Column Selectivity: {}", bench_name));
            
            // Benchmark Limbo
            #[allow(clippy::arc_with_non_send_sync)]
            let io = Arc::new(PlatformIO::new().unwrap());
            let db = Database::open_file(io.clone(), &db_path, false).unwrap();
            let limbo_conn = db.connect().unwrap();
            
            group.bench_with_input(
                BenchmarkId::new("limbo", &bench_name),
                &query,
                |b, query| {
                    b.iter(|| {
                        let mut stmt = limbo_conn.prepare(query).unwrap();
                        let io = io.clone();
                        let mut count = 0;
                        loop {
                            match stmt.step().unwrap() {
                                StepResult::Row => {
                                    black_box(stmt.row());
                                    count += 1;
                                }
                                StepResult::IO => {
                                    let _ = io.run_once();
                                }
                                StepResult::Done => {
                                    break;
                                }
                                StepResult::Interrupt | StepResult::Busy => {
                                    unreachable!();
                                }
                            }
                        }
                        stmt.reset();
                        black_box(count);
                    });
                },
            );
            
            // Benchmark rusqlite
            if enable_rusqlite {
                let sqlite_conn = rusqlite::Connection::open(&db_path).unwrap();
                sqlite_conn.pragma_update(None, "locking_mode", "EXCLUSIVE").unwrap();
                
                group.bench_with_input(
                    BenchmarkId::new("rusqlite", &bench_name),
                    &query,
                    |b, query| {
                        b.iter(|| {
                            let mut stmt = sqlite_conn.prepare(query).unwrap();
                            let mut rows = stmt.query([]).unwrap();
                            let mut count = 0;
                            while let Some(row) = rows.next().unwrap() {
                                let _ = black_box(row);
                                count += 1;
                            }
                            black_box(count);
                        });
                    },
                );
            }
            
            group.finish();
        }
    }
}

fn bench_aggregations(c: &mut Criterion) {
    let enable_rusqlite = std::env::var("DISABLE_RUSQLITE_BENCHMARK").is_err();
    
    for &num_columns in TABLE_WIDTHS {
        let (_temp_dir, db_path) = setup_wide_table_database(num_columns);
        
        let queries = vec![
            ("COUNT(*) FROM wide_table", "count_star"),
            ("COUNT(col_0) FROM wide_table", "count_column"),
            ("SUM(col_0) FROM wide_table", "sum_single"),
            ("AVG(col_0), MIN(col_4 % 10), MAX(col_8 % 10) FROM wide_table", "multi_aggregate"),
        ];
        
        for (query_template, name) in queries {
            let query = format!("SELECT {}", query_template);
            let bench_name = format!("{}_{}_cols", name, num_columns);
            let mut group = c.benchmark_group(format!("Aggregation: {}", bench_name));
            
            // Benchmark Limbo
            #[allow(clippy::arc_with_non_send_sync)]
            let io = Arc::new(PlatformIO::new().unwrap());
            let db = Database::open_file(io.clone(), &db_path, false).unwrap();
            let limbo_conn = db.connect().unwrap();
            
            group.bench_with_input(
                BenchmarkId::new("limbo", &bench_name),
                &query,
                |b, query| {
                    b.iter(|| {
                        let mut stmt = limbo_conn.prepare(query).unwrap();
                        let io = io.clone();
                        loop {
                            match stmt.step().unwrap() {
                                StepResult::Row => {
                                    black_box(stmt.row());
                                }
                                StepResult::IO => {
                                    let _ = io.run_once();
                                }
                                StepResult::Done => {
                                    break;
                                }
                                StepResult::Interrupt | StepResult::Busy => {
                                    unreachable!();
                                }
                            }
                        }
                        stmt.reset();
                    });
                },
            );
            
            // Benchmark rusqlite
            if enable_rusqlite {
                let sqlite_conn = rusqlite::Connection::open(&db_path).unwrap();
                sqlite_conn.pragma_update(None, "locking_mode", "EXCLUSIVE").unwrap();
                
                group.bench_with_input(
                    BenchmarkId::new("rusqlite", &bench_name),
                    &query,
                    |b, query| {
                        b.iter(|| {
                            let mut stmt = sqlite_conn.prepare(query).unwrap();
                            let mut rows = stmt.query([]).unwrap();
                            while let Some(row) = rows.next().unwrap() {
                                let _ = black_box(row);
                            }
                        });
                    },
                );
            }
            
            group.finish();
        }
    }
}

fn bench_real_world_patterns(c: &mut Criterion) {
    let enable_rusqlite = std::env::var("DISABLE_RUSQLITE_BENCHMARK").is_err();
    
    for &num_columns in TABLE_WIDTHS {
        let (_temp_dir, db_path) = setup_wide_table_database(num_columns);
        
        // Filter and project
        let filter_query = "SELECT col_0, col_1, col_2 FROM wide_table WHERE col_0 BETWEEN 1000 AND 5000";
        let bench_name = format!("filter_project_{}_cols", num_columns);
        let mut group = c.benchmark_group(format!("Real World: {}", bench_name));
        
        // Benchmark Limbo
        #[allow(clippy::arc_with_non_send_sync)]
        let io = Arc::new(PlatformIO::new().unwrap());
        let db = Database::open_file(io.clone(), &db_path, false).unwrap();
        let limbo_conn = db.connect().unwrap();
        
        group.bench_with_input(
            BenchmarkId::new("limbo", &bench_name),
            &filter_query,
            |b, query| {
                b.iter(|| {
                    let mut stmt = limbo_conn.prepare(query).unwrap();
                    let io = io.clone();
                    let mut count = 0;
                    loop {
                        match stmt.step().unwrap() {
                            StepResult::Row => {
                                black_box(stmt.row());
                                count += 1;
                            }
                            StepResult::IO => {
                                let _ = io.run_once();
                            }
                            StepResult::Done => {
                                break;
                            }
                            StepResult::Interrupt | StepResult::Busy => {
                                unreachable!();
                            }
                        }
                    }
                    stmt.reset();
                    black_box(count);
                });
            },
        );
        
        // Benchmark rusqlite
        if enable_rusqlite {
            let sqlite_conn = rusqlite::Connection::open(&db_path).unwrap();
            sqlite_conn.pragma_update(None, "locking_mode", "EXCLUSIVE").unwrap();
            
            group.bench_with_input(
                BenchmarkId::new("rusqlite", &bench_name),
                &filter_query,
                |b, query| {
                    b.iter(|| {
                        let mut stmt = sqlite_conn.prepare(query).unwrap();
                        let mut rows = stmt.query([]).unwrap();
                        let mut count = 0;
                        while let Some(row) = rows.next().unwrap() {
                            let _ = black_box(row);
                            count += 1;
                        }
                        black_box(count);
                    });
                },
            );
        }
        
        group.finish();
        
        // Group by with aggregation (smaller sample for performance)
        let group_query = "SELECT col_0 % 100 as grp, COUNT(*), AVG(col_1) FROM wide_table WHERE col_0 < 10000 GROUP BY grp";
        let bench_name = format!("group_by_{}_cols", num_columns);
        let mut group = c.benchmark_group(format!("Real World: {}", bench_name));
        
        group.bench_with_input(
            BenchmarkId::new("limbo", &bench_name),
            &group_query,
            |b, query| {
                b.iter(|| {
                    let mut stmt = limbo_conn.prepare(query).unwrap();
                    let io = io.clone();
                    let mut count = 0;
                    loop {
                        match stmt.step().unwrap() {
                            StepResult::Row => {
                                black_box(stmt.row());
                                count += 1;
                            }
                            StepResult::IO => {
                                let _ = io.run_once();
                            }
                            StepResult::Done => {
                                break;
                            }
                            StepResult::Interrupt | StepResult::Busy => {
                                unreachable!();
                            }
                        }
                    }
                    stmt.reset();
                    black_box(count);
                });
            },
        );
        
        if enable_rusqlite {
            let sqlite_conn = rusqlite::Connection::open(&db_path).unwrap();
            sqlite_conn.pragma_update(None, "locking_mode", "EXCLUSIVE").unwrap();
            
            group.bench_with_input(
                BenchmarkId::new("rusqlite", &bench_name),
                &group_query,
                |b, query| {
                    b.iter(|| {
                        let mut stmt = sqlite_conn.prepare(query).unwrap();
                        let mut rows = stmt.query([]).unwrap();
                        let mut count = 0;
                        while let Some(row) = rows.next().unwrap() {
                            let _ = black_box(row);
                            count += 1;
                        }
                        black_box(count);
                    });
                },
            );
        }
        
        group.finish();
    }
}

// Configure criterion with profiler support
fn criterion_config() -> Criterion {
    Criterion::default()
        .with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)))
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = bench_column_selectivity, bench_aggregations, bench_real_world_patterns
}

criterion_main!(benches);