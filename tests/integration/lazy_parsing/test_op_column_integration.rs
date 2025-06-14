#[cfg(feature = "lazy_parsing")]
#[cfg(test)]
mod tests {
    use crate::common::TempDatabase;
    use limbo_core::{Value, StepResult};
    
    #[test]
    fn test_lazy_parsing_op_column_basic() {
        let tmp_db = TempDatabase::new_empty();
        let conn = tmp_db.connect_limbo();
            
            // Create a table with many columns
            let create_sql = "CREATE TABLE wide_table (
                id INTEGER PRIMARY KEY,
                col1 TEXT,
                col2 INTEGER,
                col3 REAL,
                col4 BLOB,
                col5 TEXT,
                col6 INTEGER,
                col7 REAL,
                col8 BLOB,
                col9 TEXT,
                col10 INTEGER
            )";
            
            conn.execute(create_sql).unwrap();
            
            // Insert a test row
            let insert_sql = "INSERT INTO wide_table VALUES (
                1, 'text1', 100, 3.14, x'4142', 'text2', 200, 2.71, x'4344', 'text3', 300
            )";
            
            conn.execute(insert_sql).unwrap();
            
            // Test 1: Select only a few columns (should trigger lazy parsing)
            let select_sql = "SELECT id, col2, col5 FROM wide_table WHERE id = 1";
            let mut stmt = conn.prepare(select_sql).unwrap();
            
            let mut found = false;
            loop {
                match stmt.step() {
                    Ok(StepResult::Row) => {
                        found = true;
                        let row = stmt.row().unwrap();
                        
                        // Verify values
                        assert_eq!(*row.get::<&Value>(0).unwrap(), Value::Integer(1));
                        assert_eq!(*row.get::<&Value>(1).unwrap(), Value::Integer(100));
                        assert_eq!(*row.get::<&Value>(2).unwrap(), Value::Text("text2".to_string().into()));
                    }
                    Ok(StepResult::Done) => break,
                    Ok(StepResult::IO) => {
                        tmp_db.io.run_once().unwrap();
                    }
                    _ => panic!("Unexpected step result"),
                }
            }
            
            assert!(found, "Should have found at least one row");
            
            // Test 2: Select ALL columns (should still work with lazy parsing)
            let select_all_sql = "SELECT * FROM wide_table WHERE id = 1";
            let mut stmt_all = conn.prepare(select_all_sql).unwrap();
            
            let mut found_all = false;
            loop {
                match stmt_all.step() {
                    Ok(StepResult::Row) => {
                        found_all = true;
                        let row = stmt_all.row().unwrap();
                        
                        // Verify a few values
                        assert_eq!(*row.get::<&Value>(0).unwrap(), Value::Integer(1));
                        assert_eq!(*row.get::<&Value>(1).unwrap(), Value::Text("text1".to_string().into()));
                        assert_eq!(*row.get::<&Value>(2).unwrap(), Value::Integer(100));
                        assert_eq!(*row.get::<&Value>(3).unwrap(), Value::Float(3.14));
                    }
                    Ok(StepResult::Done) => break,
                    Ok(StepResult::IO) => {
                        tmp_db.io.run_once().unwrap();
                    }
                    _ => panic!("Unexpected step result"),
                }
            }
            
            assert!(found_all, "Should have found at least one row with SELECT *");
    }
    
    #[test]
    fn test_lazy_parsing_null_handling() {
        let tmp_db = TempDatabase::new_empty();
        let conn = tmp_db.connect_limbo();
            
            // Create a table with nullable columns
            conn.execute("CREATE TABLE test_nulls (id INTEGER PRIMARY KEY, a TEXT, b INTEGER, c REAL)").unwrap();
            
            // Insert rows with various NULL patterns
            conn.execute("INSERT INTO test_nulls VALUES (1, NULL, 42, 3.14)").unwrap();
            conn.execute("INSERT INTO test_nulls VALUES (2, 'hello', NULL, 2.71)").unwrap();
            conn.execute("INSERT INTO test_nulls VALUES (3, 'world', 99, NULL)").unwrap();
            conn.execute("INSERT INTO test_nulls VALUES (4, NULL, NULL, NULL)").unwrap();
            
            // Test selecting columns with NULLs
            let query = "SELECT id, a, b FROM test_nulls ORDER BY id";
            let mut stmt = conn.prepare(query).unwrap();
            
            let mut row_count = 0;
            loop {
                match stmt.step() {
                    Ok(StepResult::Row) => {
                        row_count += 1;
                        let row = stmt.row().unwrap();
                        
                        match row.get::<&Value>(0).unwrap() {
                            Value::Integer(1) => {
                                assert_eq!(*row.get::<&Value>(1).unwrap(), Value::Null);
                                assert_eq!(*row.get::<&Value>(2).unwrap(), Value::Integer(42));
                            },
                            Value::Integer(2) => {
                                assert_eq!(*row.get::<&Value>(1).unwrap(), Value::Text("hello".to_string().into()));
                                assert_eq!(*row.get::<&Value>(2).unwrap(), Value::Null);
                            },
                            Value::Integer(3) => {
                                assert_eq!(*row.get::<&Value>(1).unwrap(), Value::Text("world".to_string().into()));
                                assert_eq!(*row.get::<&Value>(2).unwrap(), Value::Integer(99));
                            },
                            Value::Integer(4) => {
                                assert_eq!(*row.get::<&Value>(1).unwrap(), Value::Null);
                                assert_eq!(*row.get::<&Value>(2).unwrap(), Value::Null);
                            },
                            _ => panic!("Unexpected id value"),
                        }
                    }
                    Ok(StepResult::Done) => break,
                    Ok(StepResult::IO) => {
                        tmp_db.io.run_once().unwrap();
                    }
                    _ => panic!("Unexpected step result"),
                }
            }
            
            assert_eq!(row_count, 4, "Should have found 4 rows");
    }
}