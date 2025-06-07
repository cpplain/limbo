use criterion::{black_box, criterion_group, criterion_main, Criterion};
use limbo_core::{Database, PlatformIO, IO};
use std::sync::Arc;
use tempfile::NamedTempFile;

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
    conn.close().unwrap();
    
    temp_file
}

fn bench_wide_table_access(c: &mut Criterion) {
    println!("\n=== Lazy Parsing Benchmarks ===");
    println!("Testing column access patterns on wide tables");
    println!("Projection-based parsing is now active for SELECT queries");
    println!();
    
    // Test with different table widths
    for &num_columns in &[10, 50, 100] {
        let temp_db = create_wide_table_database(num_columns);
        let db_path = temp_db.path().to_str().unwrap();
        
        #[allow(clippy::arc_with_non_send_sync)]
        let io = Arc::new(PlatformIO::new().unwrap());
        let db = Database::open_file(io.clone(), db_path, false).unwrap();
        let conn = db.connect().unwrap();
        
        // Test 1: SELECT * - Critical benchmark
        {
            let mut group = c.benchmark_group(format!("{}_columns_select_all", num_columns));
            group.sample_size(if num_columns > 50 { 20 } else { 100 });
            
            group.bench_function("limbo", |b| {
                let mut stmt = conn.prepare("SELECT * FROM wide_table LIMIT 100").unwrap();
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
                            limbo_core::StepResult::Done => break,
                            _ => unreachable!(),
                        }
                    }
                    assert_eq!(row_count, 100);
                    stmt.reset();
                });
            });
            
            // SQLite comparison
            let sqlite_conn = rusqlite::Connection::open(db_path).unwrap();
            group.bench_function("sqlite", |b| {
                let mut stmt = sqlite_conn.prepare("SELECT * FROM wide_table LIMIT 100").unwrap();
                b.iter(|| {
                    let mut rows = stmt.raw_query();
                    let mut row_count = 0;
                    while let Some(row) = rows.next().unwrap() {
                        row_count += 1;
                        for i in 0..num_columns {
                            black_box(row.get_ref_unwrap(i));
                        }
                    }
                    assert_eq!(row_count, 100);
                });
            });
            
            group.finish();
        }
        
        // Test 2: SELECT first 3 columns - Should benefit from lazy parsing
        {
            let mut group = c.benchmark_group(format!("{}_columns_select_partial", num_columns));
            group.sample_size(if num_columns > 50 { 20 } else { 100 });
            
            group.bench_function("limbo", |b| {
                let mut stmt = conn.prepare("SELECT col1, col2, col3 FROM wide_table LIMIT 100").unwrap();
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
                            limbo_core::StepResult::Done => break,
                            _ => unreachable!(),
                        }
                    }
                    assert_eq!(row_count, 100);
                    stmt.reset();
                });
            });
            
            let sqlite_conn = rusqlite::Connection::open(db_path).unwrap();
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
            
            group.finish();
        }
        
        // Test 3: Sparse column access (if table is wide enough)
        if num_columns >= 50 {
            let mut group = c.benchmark_group(format!("{}_columns_select_sparse", num_columns));
            group.sample_size(20);
            
            let query = format!("SELECT col1, col{}, col{} FROM wide_table LIMIT 100", 
                num_columns / 2, num_columns);
            
            group.bench_function("limbo", |b| {
                let mut stmt = conn.prepare(&query).unwrap();
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
                            limbo_core::StepResult::Done => break,
                            _ => unreachable!(),
                        }
                    }
                    assert_eq!(row_count, 100);
                    stmt.reset();
                });
            });
            
            let sqlite_conn = rusqlite::Connection::open(db_path).unwrap();
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
            
            group.finish();
        }
    }
}


criterion_group! {
    name = benches;
    config = Criterion::default();
    targets = bench_wide_table_access
}

criterion_main!(benches);