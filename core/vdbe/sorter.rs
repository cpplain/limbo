use limbo_sqlite3_parser::ast::SortOrder;

use crate::{
    translate::collate::CollationSeq,
    types::{ImmutableRecord, IndexKeySortOrder},
};

#[cfg(not(feature = "lazy_parsing"))]
use crate::types::compare_immutable;

#[cfg(feature = "lazy_parsing")]
use crate::types::RefValue;

pub struct Sorter {
    records: Vec<ImmutableRecord>,
    current: Option<ImmutableRecord>,
    order: IndexKeySortOrder,
    key_len: usize,
    collations: Vec<CollationSeq>,
}

impl Sorter {
    pub fn new(order: &[SortOrder], collations: Vec<CollationSeq>) -> Self {
        Self {
            records: Vec::new(),
            current: None,
            key_len: order.len(),
            order: IndexKeySortOrder::from_list(order),
            collations,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn has_more(&self) -> bool {
        self.current.is_some()
    }

    // We do the sorting here since this is what is called by the SorterSort instruction
    pub fn sort(&mut self) {
        #[cfg(feature = "lazy_parsing")]
        {
            // For lazy parsing, we need to pre-parse sort keys to enable comparison
            // This is a necessary trade-off since Rust's sort_by doesn't allow mutation
            for record in &mut self.records {
                for i in 0..self.key_len {
                    let _ = record.parse_column(i); // Parse only sort keys
                }
            }
            
            // Now sort using standard comparison
            self.records.sort_by(|a, b| {
                use std::cmp::Ordering;
                
                // Compare pre-parsed key columns
                for i in 0..self.key_len {
                    let val_a = a.values.get(i).and_then(|opt| opt.as_ref());
                    let val_b = b.values.get(i).and_then(|opt| opt.as_ref());
                    
                    match (val_a, val_b) {
                        (Some(a_val), Some(b_val)) => {
                            let column_order = self.order.get_sort_order_for_col(i);
                            let collation = self.collations.get(i).copied().unwrap_or_default();
                            
                            let cmp = match (a_val, b_val) {
                                (RefValue::Text(left), RefValue::Text(right)) => {
                                    collation.compare_strings(left.as_str(), right.as_str())
                                }
                                _ => a_val.partial_cmp(b_val).unwrap_or(Ordering::Equal),
                            };
                            
                            if !cmp.is_eq() {
                                return match column_order {
                                    SortOrder::Asc => cmp,
                                    SortOrder::Desc => cmp.reverse(),
                                };
                            }
                        }
                        (None, None) => continue,
                        (None, Some(_)) => return Ordering::Less,
                        (Some(_), None) => return Ordering::Greater,
                    }
                }
                
                Ordering::Equal
            });
        }
        
        #[cfg(not(feature = "lazy_parsing"))]
        {
            self.records.sort_by(|a, b| {
                compare_immutable(
                    &a.values[..self.key_len],
                    &b.values[..self.key_len],
                    self.order,
                    &self.collations,
                )
            });
        }
        
        self.records.reverse();
        self.next()
    }
    pub fn next(&mut self) {
        self.current = self.records.pop();
    }
    pub fn record(&self) -> Option<&ImmutableRecord> {
        self.current.as_ref()
    }

    pub fn insert(&mut self, record: &ImmutableRecord) {
        self.records.push(record.clone());
    }
}
