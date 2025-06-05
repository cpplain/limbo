use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use limbo_core::{Database, PlatformIO, IO};
use pprof::criterion::{Output, PProfProfiler};
use std::sync::Arc;
use tempfile::NamedTempFile;

fn rusqlite_open() -> rusqlite::Connection {
    let sqlite_conn = rusqlite::Connection::open("../testing/testing.db").unwrap();
    sqlite_conn
        .pragma_update(None, "locking_mode", "EXCLUSIVE")
        .unwrap();
    sqlite_conn
}

fn bench_prepare_query(criterion: &mut Criterion) {
    // https://github.com/tursodatabase/limbo/issues/174
    // The rusqlite benchmark crashes on Mac M1 when using the flamegraph features
    let enable_rusqlite = std::env::var("DISABLE_RUSQLITE_BENCHMARK").is_err();

    #[allow(clippy::arc_with_non_send_sync)]
    let io = Arc::new(PlatformIO::new().unwrap());
    let db = Database::open_file(io.clone(), "../testing/testing.db", false).unwrap();
    let limbo_conn = db.connect().unwrap();

    let queries = [
        "SELECT 1",
        "SELECT * FROM users LIMIT 1",
        "SELECT first_name, count(1) FROM users GROUP BY first_name HAVING count(1) > 1 ORDER BY count(1)  LIMIT 1",
    ];

    for query in queries.iter() {
        let mut group = criterion.benchmark_group(format!("Prepare `{}`", query));

        group.bench_with_input(
            BenchmarkId::new("limbo_parse_query", query),
            query,
            |b, query| {
                b.iter(|| {
                    limbo_conn.prepare(query).unwrap();
                });
            },
        );

        if enable_rusqlite {
            let sqlite_conn = rusqlite_open();

            group.bench_with_input(
                BenchmarkId::new("sqlite_parse_query", query),
                query,
                |b, query| {
                    b.iter(|| {
                        sqlite_conn.prepare(query).unwrap();
                    });
                },
            );
        }

        group.finish();
    }
}

fn bench_execute_select_rows(criterion: &mut Criterion) {
    // https://github.com/tursodatabase/limbo/issues/174
    // The rusqlite benchmark crashes on Mac M1 when using the flamegraph features
    let enable_rusqlite = std::env::var("DISABLE_RUSQLITE_BENCHMARK").is_err();

    #[allow(clippy::arc_with_non_send_sync)]
    let io = Arc::new(PlatformIO::new().unwrap());
    let db = Database::open_file(io.clone(), "../testing/testing.db", false).unwrap();
    let limbo_conn = db.connect().unwrap();

    let mut group = criterion.benchmark_group("Execute `SELECT * FROM users LIMIT ?`");

    for i in [1, 10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::new("limbo_execute_select_rows", i),
            &i,
            |b, i| {
                // TODO: LIMIT doesn't support query parameters.
                let mut stmt = limbo_conn
                    .prepare(format!("SELECT * FROM users LIMIT {}", *i))
                    .unwrap();
                let io = io.clone();
                b.iter(|| {
                    loop {
                        match stmt.step().unwrap() {
                            limbo_core::StepResult::Row => {
                                black_box(stmt.row());
                            }
                            limbo_core::StepResult::IO => {
                                let _ = io.run_once();
                            }
                            limbo_core::StepResult::Done => {
                                break;
                            }
                            limbo_core::StepResult::Interrupt | limbo_core::StepResult::Busy => {
                                unreachable!();
                            }
                        }
                    }
                    stmt.reset();
                });
            },
        );

        if enable_rusqlite {
            let sqlite_conn = rusqlite_open();

            group.bench_with_input(
                BenchmarkId::new("sqlite_execute_select_rows", i),
                &i,
                |b, i| {
                    // TODO: Use parameters once we fix the above.
                    let mut stmt = sqlite_conn
                        .prepare(&format!("SELECT * FROM users LIMIT {}", *i))
                        .unwrap();
                    b.iter(|| {
                        let mut rows = stmt.raw_query();
                        while let Some(row) = rows.next().unwrap() {
                            black_box(row);
                        }
                    });
                },
            );
        }
    }

    group.finish();
}

fn bench_execute_select_1(criterion: &mut Criterion) {
    // https://github.com/tursodatabase/limbo/issues/174
    // The rusqlite benchmark crashes on Mac M1 when using the flamegraph features
    let enable_rusqlite = std::env::var("DISABLE_RUSQLITE_BENCHMARK").is_err();

    #[allow(clippy::arc_with_non_send_sync)]
    let io = Arc::new(PlatformIO::new().unwrap());
    let db = Database::open_file(io.clone(), "../testing/testing.db", false).unwrap();
    let limbo_conn = db.connect().unwrap();

    let mut group = criterion.benchmark_group("Execute `SELECT 1`");

    group.bench_function("limbo_execute_select_1", |b| {
        let mut stmt = limbo_conn.prepare("SELECT 1").unwrap();
        let io = io.clone();
        b.iter(|| {
            loop {
                match stmt.step().unwrap() {
                    limbo_core::StepResult::Row => {
                        black_box(stmt.row());
                    }
                    limbo_core::StepResult::IO => {
                        let _ = io.run_once();
                    }
                    limbo_core::StepResult::Done => {
                        break;
                    }
                    limbo_core::StepResult::Interrupt | limbo_core::StepResult::Busy => {
                        unreachable!();
                    }
                }
            }
            stmt.reset();
        });
    });

    if enable_rusqlite {
        let sqlite_conn = rusqlite_open();

        group.bench_function("sqlite_execute_select_1", |b| {
            let mut stmt = sqlite_conn.prepare("SELECT 1").unwrap();
            b.iter(|| {
                let mut rows = stmt.raw_query();
                while let Some(row) = rows.next().unwrap() {
                    black_box(row);
                }
            });
        });
    }

    group.finish();
}

fn bench_execute_select_count(criterion: &mut Criterion) {
    // https://github.com/tursodatabase/limbo/issues/174
    // The rusqlite benchmark crashes on Mac M1 when using the flamegraph features
    let enable_rusqlite = std::env::var("DISABLE_RUSQLITE_BENCHMARK").is_err();

    #[allow(clippy::arc_with_non_send_sync)]
    let io = Arc::new(PlatformIO::new().unwrap());
    let db = Database::open_file(io.clone(), "../testing/testing.db", false).unwrap();
    let limbo_conn = db.connect().unwrap();

    let mut group = criterion.benchmark_group("Execute `SELECT count() FROM users`");

    group.bench_function("limbo_execute_select_count", |b| {
        let mut stmt = limbo_conn.prepare("SELECT count() FROM users").unwrap();
        let io = io.clone();
        b.iter(|| {
            loop {
                match stmt.step().unwrap() {
                    limbo_core::StepResult::Row => {
                        black_box(stmt.row());
                    }
                    limbo_core::StepResult::IO => {
                        let _ = io.run_once();
                    }
                    limbo_core::StepResult::Done => {
                        break;
                    }
                    limbo_core::StepResult::Interrupt | limbo_core::StepResult::Busy => {
                        unreachable!();
                    }
                }
            }
            stmt.reset();
        });
    });

    if enable_rusqlite {
        let sqlite_conn = rusqlite_open();

        group.bench_function("sqlite_execute_select_count", |b| {
            let mut stmt = sqlite_conn.prepare("SELECT count() FROM users").unwrap();
            b.iter(|| {
                let mut rows = stmt.raw_query();
                while let Some(row) = rows.next().unwrap() {
                    black_box(row);
                }
            });
        });
    }

    group.finish();
}

fn create_wide_table_database(num_columns: usize) -> NamedTempFile {
    let temp_file = NamedTempFile::new().unwrap();
    let path = temp_file.path().to_str().unwrap();
    
    // Create database with SQLite to ensure compatibility
    let conn = rusqlite::Connection::open(path).unwrap();
    
    // Build CREATE TABLE statement
    let mut create_sql = String::from("CREATE TABLE wide_table (id INTEGER PRIMARY KEY");
    for i in 1..=num_columns {
        create_sql.push_str(&format!(", col{} ", i));
        match i % 4 {
            0 => create_sql.push_str("INTEGER"),
            1 => create_sql.push_str("TEXT"),
            2 => create_sql.push_str("REAL"),
            3 => create_sql.push_str("BLOB"),
            _ => unreachable!(),
        }
    }
    create_sql.push_str(")");
    
    conn.execute(&create_sql, []).unwrap();
    
    // Insert test data - 1000 rows
    let mut insert_sql = String::from("INSERT INTO wide_table VALUES (?");
    for _ in 1..=num_columns {
        insert_sql.push_str(", ?");
    }
    insert_sql.push_str(")");
    
    let mut stmt = conn.prepare(&insert_sql).unwrap();
    
    for row_id in 1..=1000 {
        let mut values: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::from(row_id)];
        
        for col in 1..=num_columns {
            match col % 4 {
                0 => values.push(rusqlite::types::Value::from(col as i64 * row_id)),
                1 => values.push(rusqlite::types::Value::from(format!("text_{}_{}", row_id, col))),
                2 => values.push(rusqlite::types::Value::from(col as f64 * row_id as f64 / 3.14)),
                3 => {
                    // Create BLOB data - larger for columns near the end to test lazy parsing benefits
                    let size = if col > num_columns * 3 / 4 { 4096 } else { 64 };
                    let blob_data = vec![((row_id * col as i64) % 256) as u8; size];
                    values.push(rusqlite::types::Value::from(blob_data));
                }
                _ => unreachable!(),
            }
        }
        
        stmt.execute(rusqlite::params_from_iter(values)).unwrap();
    }
    
    drop(stmt);
    drop(conn);
    
    temp_file
}

fn bench_lazy_parsing_column_access(criterion: &mut Criterion) {
    let enable_rusqlite = std::env::var("DISABLE_RUSQLITE_BENCHMARK").is_err();
    
    // Test different table widths
    let column_counts = vec![10, 50, 100, 200];
    
    for &num_columns in &column_counts {
        let temp_db = create_wide_table_database(num_columns);
        let db_path = temp_db.path().to_str().unwrap();
        
        #[allow(clippy::arc_with_non_send_sync)]
        let io = Arc::new(PlatformIO::new().unwrap());
        let db = Database::open_file(io.clone(), db_path, false).unwrap();
        let limbo_conn = db.connect().unwrap();
        
        // Test 1: SELECT * - This MUST NOT regress significantly
        {
            let mut group = criterion.benchmark_group(format!("lazy_parsing_{}_columns_select_all", num_columns));
            group.sample_size(20); // Reduce sample size for large tables
            
            group.bench_function("limbo", |b| {
                let mut stmt = limbo_conn.prepare("SELECT * FROM wide_table LIMIT 100").unwrap();
                let io = io.clone();
                b.iter(|| {
                    let mut row_count = 0;
                    loop {
                        match stmt.step().unwrap() {
                            limbo_core::StepResult::Row => {
                                let row = stmt.row().unwrap();
                                row_count += 1;
                                // Access all columns to simulate real SELECT * usage
                                for i in 0..num_columns {
                                    black_box(row.get::<&limbo_core::Value>(i).unwrap());
                                }
                            }
                            limbo_core::StepResult::IO => {
                                let _ = io.run_once();
                            }
                            limbo_core::StepResult::Done => {
                                break;
                            }
                            limbo_core::StepResult::Interrupt | limbo_core::StepResult::Busy => {
                                unreachable!();
                            }
                        }
                    }
                    assert_eq!(row_count, 100);
                    stmt.reset();
                });
            });
            
            if enable_rusqlite {
                // Create a copy of the database for SQLite to avoid locking issues
                let sqlite_db = NamedTempFile::new().unwrap();
                std::fs::copy(db_path, sqlite_db.path()).unwrap();
                let sqlite_conn = rusqlite::Connection::open(sqlite_db.path()).unwrap();
                group.bench_function("sqlite", |b| {
                    let mut stmt = sqlite_conn.prepare("SELECT * FROM wide_table LIMIT 100").unwrap();
                    b.iter(|| {
                        let mut rows = stmt.raw_query();
                        let mut row_count = 0;
                        while let Some(row) = rows.next().unwrap() {
                            row_count += 1;
                            // Access all columns
                            for i in 0..num_columns {
                                black_box(row.get_ref_unwrap(i));
                            }
                        }
                        assert_eq!(row_count, 100);
                    });
                });
            }
            
            group.finish();
        }
        
        // Test 2: SELECT first few columns - Sequential partial access
        {
            let mut group = criterion.benchmark_group(format!("lazy_parsing_{}_columns_select_first_3", num_columns));
            group.sample_size(20);
            
            group.bench_function("limbo", |b| {
                let mut stmt = limbo_conn.prepare("SELECT col1, col2, col3 FROM wide_table LIMIT 100").unwrap();
                let io = io.clone();
                b.iter(|| {
                    let mut row_count = 0;
                    loop {
                        match stmt.step().unwrap() {
                            limbo_core::StepResult::Row => {
                                let row = stmt.row().unwrap();
                                row_count += 1;
                                black_box(row.get::<&limbo_core::Value>(0).unwrap());
                                black_box(row.get::<&limbo_core::Value>(1).unwrap());
                                black_box(row.get::<&limbo_core::Value>(2).unwrap());
                            }
                            limbo_core::StepResult::IO => {
                                let _ = io.run_once();
                            }
                            limbo_core::StepResult::Done => {
                                break;
                            }
                            limbo_core::StepResult::Interrupt | limbo_core::StepResult::Busy => {
                                unreachable!();
                            }
                        }
                    }
                    assert_eq!(row_count, 100);
                    stmt.reset();
                });
            });
            
            if enable_rusqlite {
                // Create a copy of the database for SQLite to avoid locking issues
                let sqlite_db = NamedTempFile::new().unwrap();
                std::fs::copy(db_path, sqlite_db.path()).unwrap();
                let sqlite_conn = rusqlite::Connection::open(sqlite_db.path()).unwrap();
                group.bench_function("sqlite", |b| {
                    let mut stmt = sqlite_conn.prepare("SELECT col1, col2, col3 FROM wide_table LIMIT 100").unwrap();
                    b.iter(|| {
                        let mut rows = stmt.raw_query();
                        let mut row_count = 0;
                        while let Some(row) = rows.next().unwrap() {
                            row_count += 1;
                            black_box(row.get_ref_unwrap(0));
                            black_box(row.get_ref_unwrap(1));
                            black_box(row.get_ref_unwrap(2));
                        }
                        assert_eq!(row_count, 100);
                    });
                });
            }
            
            group.finish();
        }
        
        // Test 3: SELECT sparse columns - Non-sequential access pattern
        if num_columns >= 50 {
            let mut group = criterion.benchmark_group(format!("lazy_parsing_{}_columns_select_sparse", num_columns));
            group.sample_size(20);
            
            let last_col = num_columns - 1;
            let middle_col = num_columns / 2;
            let query = format!("SELECT col1, col{}, col{} FROM wide_table LIMIT 100", middle_col, last_col);
            
            group.bench_function("limbo", |b| {
                let mut stmt = limbo_conn.prepare(&query).unwrap();
                let io = io.clone();
                b.iter(|| {
                    let mut row_count = 0;
                    loop {
                        match stmt.step().unwrap() {
                            limbo_core::StepResult::Row => {
                                let row = stmt.row().unwrap();
                                row_count += 1;
                                black_box(row.get::<&limbo_core::Value>(0).unwrap());
                                black_box(row.get::<&limbo_core::Value>(1).unwrap());
                                black_box(row.get::<&limbo_core::Value>(2).unwrap());
                            }
                            limbo_core::StepResult::IO => {
                                let _ = io.run_once();
                            }
                            limbo_core::StepResult::Done => {
                                break;
                            }
                            limbo_core::StepResult::Interrupt | limbo_core::StepResult::Busy => {
                                unreachable!();
                            }
                        }
                    }
                    assert_eq!(row_count, 100);
                    stmt.reset();
                });
            });
            
            if enable_rusqlite {
                // Create a copy of the database for SQLite to avoid locking issues
                let sqlite_db = NamedTempFile::new().unwrap();
                std::fs::copy(db_path, sqlite_db.path()).unwrap();
                let sqlite_conn = rusqlite::Connection::open(sqlite_db.path()).unwrap();
                group.bench_function("sqlite", |b| {
                    let mut stmt = sqlite_conn.prepare(&query).unwrap();
                    b.iter(|| {
                        let mut rows = stmt.raw_query();
                        let mut row_count = 0;
                        while let Some(row) = rows.next().unwrap() {
                            row_count += 1;
                            black_box(row.get_ref_unwrap(0));
                            black_box(row.get_ref_unwrap(1));
                            black_box(row.get_ref_unwrap(2));
                        }
                        assert_eq!(row_count, 100);
                    });
                });
            }
            
            group.finish();
        }
        
        // Test 4: SELECT single column near end - Best case for lazy parsing
        if num_columns >= 50 {
            let mut group = criterion.benchmark_group(format!("lazy_parsing_{}_columns_select_last", num_columns));
            group.sample_size(20);
            
            let last_col = num_columns - 1;
            let query = format!("SELECT col{} FROM wide_table LIMIT 100", last_col);
            
            group.bench_function("limbo", |b| {
                let mut stmt = limbo_conn.prepare(&query).unwrap();
                let io = io.clone();
                b.iter(|| {
                    let mut row_count = 0;
                    loop {
                        match stmt.step().unwrap() {
                            limbo_core::StepResult::Row => {
                                let row = stmt.row().unwrap();
                                row_count += 1;
                                black_box(row.get::<&limbo_core::Value>(0).unwrap());
                            }
                            limbo_core::StepResult::IO => {
                                let _ = io.run_once();
                            }
                            limbo_core::StepResult::Done => {
                                break;
                            }
                            limbo_core::StepResult::Interrupt | limbo_core::StepResult::Busy => {
                                unreachable!();
                            }
                        }
                    }
                    assert_eq!(row_count, 100);
                    stmt.reset();
                });
            });
            
            if enable_rusqlite {
                // Create a copy of the database for SQLite to avoid locking issues
                let sqlite_db = NamedTempFile::new().unwrap();
                std::fs::copy(db_path, sqlite_db.path()).unwrap();
                let sqlite_conn = rusqlite::Connection::open(sqlite_db.path()).unwrap();
                group.bench_function("sqlite", |b| {
                    let mut stmt = sqlite_conn.prepare(&query).unwrap();
                    b.iter(|| {
                        let mut rows = stmt.raw_query();
                        let mut row_count = 0;
                        while let Some(row) = rows.next().unwrap() {
                            row_count += 1;
                            black_box(row.get_ref_unwrap(0));
                        }
                        assert_eq!(row_count, 100);
                    });
                });
            }
            
            group.finish();
        }
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = bench_prepare_query, bench_execute_select_1, bench_execute_select_rows, bench_execute_select_count, bench_lazy_parsing_column_access
}
criterion_main!(benches);
