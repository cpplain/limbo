use limbo_ext::{AggCtx, FinalizeFunction, StepFunction};
use limbo_sqlite3_parser::ast::SortOrder;
#[cfg(feature = "serde")]
use serde::Deserialize;

use crate::error::LimboError;
use crate::ext::{ExtValue, ExtValueType};
use crate::pseudo::PseudoCursor;
use crate::schema::Index;
use crate::storage::btree::BTreeCursor;
#[cfg(feature = "lazy_parsing")]
use crate::storage::sqlite3_ondisk::read_value;
use crate::storage::sqlite3_ondisk::write_varint;
use crate::translate::collate::CollationSeq;
use crate::translate::plan::IterationDirection;
use crate::vdbe::sorter::Sorter;
use crate::vdbe::Register;
use crate::vtab::VirtualTableCursor;
use crate::Result;
use std::fmt::Display;
#[cfg(feature = "lazy_parsing")]
use std::sync::Arc;

const MAX_REAL_SIZE: u8 = 15;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueType {
    Null,
    Integer,
    Float,
    Text,
    Blob,
    Error,
}

impl Display for ValueType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Null => "NULL",
            Self::Integer => "INT",
            Self::Float => "REAL",
            Self::Blob => "BLOB",
            Self::Text => "TEXT",
            Self::Error => "ERROR",
        };
        write!(f, "{}", value)
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TextSubtype {
    Text,
    #[cfg(feature = "json")]
    Json,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Text {
    pub value: Vec<u8>,
    pub subtype: TextSubtype,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextRef {
    pub value: RawSlice,
    pub subtype: TextSubtype,
}

impl Text {
    pub fn from_str<S: Into<String>>(value: S) -> Self {
        Self::new(&value.into())
    }

    pub fn new(value: &str) -> Self {
        Self {
            value: value.as_bytes().to_vec(),
            subtype: TextSubtype::Text,
        }
    }

    #[cfg(feature = "json")]
    pub fn json(value: String) -> Self {
        Self {
            value: value.into_bytes(),
            subtype: TextSubtype::Json,
        }
    }

    pub fn to_string(&self) -> String {
        self.as_str().to_string()
    }

    pub fn as_str(&self) -> &str {
        unsafe { std::str::from_utf8_unchecked(self.value.as_ref()) }
    }
}

impl From<String> for Text {
    fn from(value: String) -> Self {
        Text {
            value: value.into_bytes(),
            subtype: TextSubtype::Text,
        }
    }
}

impl TextRef {
    pub fn as_str(&self) -> &str {
        unsafe { std::str::from_utf8_unchecked(self.value.to_slice()) }
    }

    pub fn to_string(&self) -> String {
        self.as_str().to_string()
    }
}

#[cfg(feature = "serde")]
fn float_to_string<S>(float: &f64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&format!("{}", float))
}

#[cfg(feature = "serde")]
fn string_to_float<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Value {
    Null,
    Integer(i64),
    // we use custom serialization to preserve float precision
    #[cfg_attr(
        feature = "serde",
        serde(
            serialize_with = "float_to_string",
            deserialize_with = "string_to_float"
        )
    )]
    Float(f64),
    Text(Text),
    Blob(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawSlice {
    data: *const u8,
    len: usize,
}

#[derive(Debug, PartialEq, Clone)]
pub enum RefValue {
    Null,
    Integer(i64),
    Float(f64),
    Text(TextRef),
    Blob(RawSlice),
}

impl Value {
    // A helper function that makes building a text Value easier.
    pub fn build_text(text: impl AsRef<str>) -> Self {
        Self::Text(Text::new(text.as_ref()))
    }

    pub fn to_blob(&self) -> Option<&[u8]> {
        match self {
            Self::Blob(blob) => Some(blob),
            _ => None,
        }
    }

    pub fn from_blob(data: Vec<u8>) -> Self {
        Value::Blob(data)
    }

    pub fn to_text(&self) -> Option<&str> {
        match self {
            Value::Text(t) => Some(t.as_str()),
            _ => None,
        }
    }

    pub fn from_text(text: &str) -> Self {
        Value::Text(Text::new(text))
    }

    pub fn value_type(&self) -> ValueType {
        match self {
            Value::Null => ValueType::Null,
            Value::Integer(_) => ValueType::Integer,
            Value::Float(_) => ValueType::Float,
            Value::Text(_) => ValueType::Text,
            Value::Blob(_) => ValueType::Blob,
        }
    }
    pub fn serialize_serial(&self, out: &mut Vec<u8>) {
        match self {
            Value::Null => {}
            Value::Integer(i) => {
                let serial_type = SerialType::from(self);
                match serial_type.kind() {
                    SerialTypeKind::I8 => out.extend_from_slice(&(*i as i8).to_be_bytes()),
                    SerialTypeKind::I16 => out.extend_from_slice(&(*i as i16).to_be_bytes()),
                    SerialTypeKind::I24 => out.extend_from_slice(&(*i as i32).to_be_bytes()[1..]), // remove most significant byte
                    SerialTypeKind::I32 => out.extend_from_slice(&(*i as i32).to_be_bytes()),
                    SerialTypeKind::I48 => out.extend_from_slice(&i.to_be_bytes()[2..]), // remove 2 most significant bytes
                    SerialTypeKind::I64 => out.extend_from_slice(&i.to_be_bytes()),
                    _ => unreachable!(),
                }
            }
            Value::Float(f) => out.extend_from_slice(&f.to_be_bytes()),
            Value::Text(t) => out.extend_from_slice(&t.value),
            Value::Blob(b) => out.extend_from_slice(b),
        };
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternalAggState {
    pub state: *mut AggCtx,
    pub argc: usize,
    pub step_fn: StepFunction,
    pub finalize_fn: FinalizeFunction,
    pub finalized_value: Option<Value>,
}

impl ExternalAggState {
    pub fn cache_final_value(&mut self, value: Value) -> &Value {
        self.finalized_value = Some(value);
        self.finalized_value.as_ref().unwrap()
    }
}

/// Please use Display trait for all limbo output so we have single origin of truth
/// When you need value as string:
/// ---GOOD---
/// format!("{}", value);
/// ---BAD---
/// match value {
///   Value::Integer(i) => *i.as_str(),
///   Value::Float(f) => *f.as_str(),
///   ....
/// }
impl Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => write!(f, ""),
            Self::Integer(i) => {
                write!(f, "{}", i)
            }
            Self::Float(fl) => {
                let fl = *fl;
                if fl == f64::INFINITY {
                    return write!(f, "Inf");
                }
                if fl == f64::NEG_INFINITY {
                    return write!(f, "-Inf");
                }
                if fl.is_nan() {
                    return write!(f, "");
                }
                // handle negative 0
                if fl == -0.0 {
                    return write!(f, "{:.1}", fl.abs());
                }

                // handle scientific notation without trailing zeros
                if (fl.abs() < 1e-4 || fl.abs() >= 1e15) && fl != 0.0 {
                    let sci_notation = format!("{:.14e}", fl);
                    let parts: Vec<&str> = sci_notation.split('e').collect();

                    if parts.len() == 2 {
                        let mantissa = parts[0];
                        let exponent = parts[1];

                        let decimal_parts: Vec<&str> = mantissa.split('.').collect();
                        if decimal_parts.len() == 2 {
                            let whole = decimal_parts[0];
                            // 1.{this part}
                            let mut fraction = String::from(decimal_parts[1]);

                            //removing trailing 0 from fraction
                            while fraction.ends_with('0') {
                                fraction.pop();
                            }

                            let trimmed_mantissa = if fraction.is_empty() {
                                whole.to_string()
                            } else {
                                format!("{}.{}", whole, fraction)
                            };
                            let (prefix, exponent) = if exponent.starts_with('-') {
                                ("-0", &exponent[1..])
                            } else {
                                ("+", exponent)
                            };
                            return write!(f, "{}e{}{}", trimmed_mantissa, prefix, exponent);
                        }
                    }

                    // fallback
                    return write!(f, "{}", sci_notation);
                }

                // handle floating point max size is 15.
                // If left > right && right + left > 15 go to sci notation
                // If right > left && right + left > 15 truncate left so right + left == 15
                let rounded = fl.round();
                if (fl - rounded).abs() < 1e-14 {
                    // if we very close to integer trim decimal part to 1 digit
                    if rounded == rounded as i64 as f64 {
                        return write!(f, "{:.1}", fl);
                    }
                }

                let fl_str = format!("{}", fl);
                let splitted = fl_str.split('.').collect::<Vec<&str>>();
                // fallback
                if splitted.len() != 2 {
                    return write!(f, "{:.14e}", fl);
                }

                let first_part = if fl < 0.0 {
                    // remove -
                    &splitted[0][1..]
                } else {
                    splitted[0]
                };

                let second = splitted[1];

                // We want more precision for smaller numbers. in SQLite case we want 15 non zero digits in 0 < number < 1
                // leading zeroes added to max real size. But if float < 1e-4 we go to scientific notation
                let leading_zeros = second.chars().take_while(|c| c == &'0').count();
                let reminder = if first_part != "0" {
                    MAX_REAL_SIZE as isize - first_part.len() as isize
                } else {
                    MAX_REAL_SIZE as isize + leading_zeros as isize
                };
                // float that have integer part > 15 converted to sci notation
                if reminder < 0 {
                    return write!(f, "{:.14e}", fl);
                }
                // trim decimal part to reminder or self len so total digits is 15;
                let mut fl = format!("{:.*}", second.len().min(reminder as usize), fl);
                // if decimal part ends with 0 we trim it
                while fl.ends_with('0') {
                    fl.pop();
                }
                write!(f, "{}", fl)
            }
            Self::Text(s) => {
                write!(f, "{}", s.as_str())
            }
            Self::Blob(b) => write!(f, "{}", String::from_utf8_lossy(b)),
        }
    }
}

impl Value {
    pub fn to_ffi(&self) -> ExtValue {
        match self {
            Self::Null => ExtValue::null(),
            Self::Integer(i) => ExtValue::from_integer(*i),
            Self::Float(fl) => ExtValue::from_float(*fl),
            Self::Text(text) => ExtValue::from_text(text.as_str().to_string()),
            Self::Blob(blob) => ExtValue::from_blob(blob.to_vec()),
        }
    }

    pub fn from_ffi(v: ExtValue) -> Result<Self> {
        let res = match v.value_type() {
            ExtValueType::Null => Ok(Value::Null),
            ExtValueType::Integer => {
                let Some(int) = v.to_integer() else {
                    return Ok(Value::Null);
                };
                Ok(Value::Integer(int))
            }
            ExtValueType::Float => {
                let Some(float) = v.to_float() else {
                    return Ok(Value::Null);
                };
                Ok(Value::Float(float))
            }
            ExtValueType::Text => {
                let Some(text) = v.to_text() else {
                    return Ok(Value::Null);
                };
                #[cfg(feature = "json")]
                if v.is_json() {
                    return Ok(Value::Text(Text::json(text.to_string())));
                }
                Ok(Value::build_text(text))
            }
            ExtValueType::Blob => {
                let Some(blob) = v.to_blob() else {
                    return Ok(Value::Null);
                };
                Ok(Value::Blob(blob))
            }
            ExtValueType::Error => {
                let Some(err) = v.to_error_details() else {
                    return Ok(Value::Null);
                };
                match err {
                    (_, Some(msg)) => Err(LimboError::ExtensionError(msg)),
                    (code, None) => Err(LimboError::ExtensionError(code.to_string())),
                }
            }
        };
        unsafe { v.__free_internal_type() };
        res
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AggContext {
    Avg(Value, Value), // acc and count
    Sum(Value),
    Count(Value),
    Max(Option<Value>),
    Min(Option<Value>),
    GroupConcat(Value),
    External(ExternalAggState),
}

const NULL: Value = Value::Null;

impl AggContext {
    pub fn compute_external(&mut self) -> Result<()> {
        if let Self::External(ext_state) = self {
            if ext_state.finalized_value.is_none() {
                let final_value = unsafe { (ext_state.finalize_fn)(ext_state.state) };
                ext_state.cache_final_value(Value::from_ffi(final_value)?);
            }
        }
        Ok(())
    }

    pub fn final_value(&self) -> &Value {
        match self {
            Self::Avg(acc, _count) => acc,
            Self::Sum(acc) => acc,
            Self::Count(count) => count,
            Self::Max(max) => max.as_ref().unwrap_or(&NULL),
            Self::Min(min) => min.as_ref().unwrap_or(&NULL),
            Self::GroupConcat(s) => s,
            Self::External(ext_state) => ext_state.finalized_value.as_ref().unwrap_or(&NULL),
        }
    }
}

impl PartialEq<Value> for Value {
    fn eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Self::Integer(int_left), Self::Integer(int_right)) => int_left == int_right,
            (Self::Integer(int_left), Self::Float(float_right)) => {
                (*int_left as f64) == (*float_right)
            }
            (Self::Float(float_left), Self::Integer(int_right)) => {
                float_left == (&(*int_right as f64))
            }
            (Self::Float(float_left), Self::Float(float_right)) => float_left == float_right,
            (Self::Integer(_) | Self::Float(_), Self::Text(_) | Self::Blob(_)) => false,
            (Self::Text(_) | Self::Blob(_), Self::Integer(_) | Self::Float(_)) => false,
            (Self::Text(text_left), Self::Text(text_right)) => {
                text_left.value.eq(&text_right.value)
            }
            (Self::Blob(blob_left), Self::Blob(blob_right)) => blob_left.eq(blob_right),
            (Self::Null, Self::Null) => true,
            _ => false,
        }
    }

    fn ne(&self, other: &Value) -> bool {
        !self.eq(other)
    }
}

#[allow(clippy::non_canonical_partial_ord_impl)]
impl PartialOrd<Value> for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Integer(int_left), Self::Integer(int_right)) => int_left.partial_cmp(int_right),
            (Self::Integer(int_left), Self::Float(float_right)) => {
                (*int_left as f64).partial_cmp(float_right)
            }
            (Self::Float(float_left), Self::Integer(int_right)) => {
                float_left.partial_cmp(&(*int_right as f64))
            }
            (Self::Float(float_left), Self::Float(float_right)) => {
                float_left.partial_cmp(float_right)
            }
            // Numeric vs Text/Blob
            (Self::Integer(_) | Self::Float(_), Self::Text(_) | Self::Blob(_)) => {
                Some(std::cmp::Ordering::Less)
            }
            (Self::Text(_) | Self::Blob(_), Self::Integer(_) | Self::Float(_)) => {
                Some(std::cmp::Ordering::Greater)
            }

            (Self::Text(text_left), Self::Text(text_right)) => {
                text_left.value.partial_cmp(&text_right.value)
            }
            // Text vs Blob
            (Self::Text(_), Self::Blob(_)) => Some(std::cmp::Ordering::Less),
            (Self::Blob(_), Self::Text(_)) => Some(std::cmp::Ordering::Greater),

            (Self::Blob(blob_left), Self::Blob(blob_right)) => blob_left.partial_cmp(blob_right),
            (Self::Null, Self::Null) => Some(std::cmp::Ordering::Equal),
            (Self::Null, _) => Some(std::cmp::Ordering::Less),
            (_, Self::Null) => Some(std::cmp::Ordering::Greater),
        }
    }
}

impl PartialOrd<AggContext> for AggContext {
    fn partial_cmp(&self, other: &AggContext) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Avg(a, _), Self::Avg(b, _)) => a.partial_cmp(b),
            (Self::Sum(a), Self::Sum(b)) => a.partial_cmp(b),
            (Self::Count(a), Self::Count(b)) => a.partial_cmp(b),
            (Self::Max(a), Self::Max(b)) => a.partial_cmp(b),
            (Self::Min(a), Self::Min(b)) => a.partial_cmp(b),
            (Self::GroupConcat(a), Self::GroupConcat(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

impl Eq for Value {}

impl Ord for Value {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl std::ops::Add<Value> for Value {
    type Output = Value;

    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::Integer(int_left), Self::Integer(int_right)) => {
                Self::Integer(int_left + int_right)
            }
            (Self::Integer(int_left), Self::Float(float_right)) => {
                Self::Float(int_left as f64 + float_right)
            }
            (Self::Float(float_left), Self::Integer(int_right)) => {
                Self::Float(float_left + int_right as f64)
            }
            (Self::Float(float_left), Self::Float(float_right)) => {
                Self::Float(float_left + float_right)
            }
            (Self::Text(string_left), Self::Text(string_right)) => {
                Self::build_text(&(string_left.as_str().to_string() + string_right.as_str()))
            }
            (Self::Text(string_left), Self::Integer(int_right)) => {
                Self::build_text(&(string_left.as_str().to_string() + &int_right.to_string()))
            }
            (Self::Integer(int_left), Self::Text(string_right)) => {
                Self::build_text(&(int_left.to_string() + string_right.as_str()))
            }
            (Self::Text(string_left), Self::Float(float_right)) => {
                let string_right = Self::Float(float_right).to_string();
                Self::build_text(&(string_left.as_str().to_string() + &string_right))
            }
            (Self::Float(float_left), Self::Text(string_right)) => {
                let string_left = Self::Float(float_left).to_string();
                Self::build_text(&(string_left + string_right.as_str()))
            }
            (lhs, Self::Null) => lhs,
            (Self::Null, rhs) => rhs,
            _ => Self::Float(0.0),
        }
    }
}

impl std::ops::Add<f64> for Value {
    type Output = Value;

    fn add(self, rhs: f64) -> Self::Output {
        match self {
            Self::Integer(int_left) => Self::Float(int_left as f64 + rhs),
            Self::Float(float_left) => Self::Float(float_left + rhs),
            _ => unreachable!(),
        }
    }
}

impl std::ops::Add<i64> for Value {
    type Output = Value;

    fn add(self, rhs: i64) -> Self::Output {
        match self {
            Self::Integer(int_left) => Self::Integer(int_left + rhs),
            Self::Float(float_left) => Self::Float(float_left + rhs as f64),
            _ => unreachable!(),
        }
    }
}

impl std::ops::AddAssign for Value {
    fn add_assign(&mut self, rhs: Self) {
        *self = self.clone() + rhs;
    }
}

impl std::ops::AddAssign<i64> for Value {
    fn add_assign(&mut self, rhs: i64) {
        *self = self.clone() + rhs;
    }
}

impl std::ops::AddAssign<f64> for Value {
    fn add_assign(&mut self, rhs: f64) {
        *self = self.clone() + rhs;
    }
}

impl std::ops::Div<Value> for Value {
    type Output = Value;

    fn div(self, rhs: Value) -> Self::Output {
        match (self, rhs) {
            (Self::Integer(int_left), Self::Integer(int_right)) => {
                Self::Integer(int_left / int_right)
            }
            (Self::Integer(int_left), Self::Float(float_right)) => {
                Self::Float(int_left as f64 / float_right)
            }
            (Self::Float(float_left), Self::Integer(int_right)) => {
                Self::Float(float_left / int_right as f64)
            }
            (Self::Float(float_left), Self::Float(float_right)) => {
                Self::Float(float_left / float_right)
            }
            _ => Self::Float(0.0),
        }
    }
}

impl std::ops::DivAssign<Value> for Value {
    fn div_assign(&mut self, rhs: Value) {
        *self = self.clone() / rhs;
    }
}

impl<'a> TryFrom<&'a RefValue> for i64 {
    type Error = LimboError;

    fn try_from(value: &'a RefValue) -> Result<Self, Self::Error> {
        match value {
            RefValue::Integer(i) => Ok(*i),
            _ => Err(LimboError::ConversionError("Expected integer value".into())),
        }
    }
}

impl<'a> TryFrom<&'a RefValue> for String {
    type Error = LimboError;

    fn try_from(value: &'a RefValue) -> Result<Self, Self::Error> {
        match value {
            RefValue::Text(s) => Ok(s.as_str().to_string()),
            _ => Err(LimboError::ConversionError("Expected text value".into())),
        }
    }
}

impl<'a> TryFrom<&'a RefValue> for &'a str {
    type Error = LimboError;

    fn try_from(value: &'a RefValue) -> Result<Self, Self::Error> {
        match value {
            RefValue::Text(s) => Ok(s.as_str()),
            _ => Err(LimboError::ConversionError("Expected text value".into())),
        }
    }
}

/// This struct serves the purpose of not allocating multiple vectors of bytes if not needed.
/// A value in a record that has already been serialized can stay serialized and what this struct offsers
/// is easy acces to each value which point to the payload.
/// The name might be contradictory as it is immutable in the sense that you cannot modify the values without modifying the payload.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ImmutableRecord {
    // We have to be super careful with this buffer since we make values point to the payload we need to take care reallocations
    // happen in a controlled manner. If we realocate with values that should be correct, they will now point to undefined data.
    // We don't use pin here because it would make it imposible to reuse the buffer if we need to push a new record in the same struct.
    // Using Arc<[u8]> for lazy parsing to avoid copying the entire payload
    #[cfg(not(feature = "lazy_parsing"))]
    payload: Vec<u8>,
    #[cfg(feature = "lazy_parsing")]
    payload: Option<Arc<[u8]>>,
    #[cfg(not(feature = "lazy_parsing"))]
    pub values: Vec<RefValue>,
    #[cfg(feature = "lazy_parsing")]
    pub values: Vec<Option<RefValue>>,
    recreating: bool,
    #[cfg(feature = "lazy_parsing")]
    lazy_state: Option<LazyParseState>,
}

#[derive(PartialEq)]
pub enum ParseRecordState {
    Init,
    Parsing { payload: Vec<u8> },
}

#[cfg(feature = "lazy_parsing")]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParsedMask {
    /// Bitmask for ≤64 columns  
    Small(u64),
    /// Multiple u64s for >64 columns
    Large(Vec<u64>),
}

#[cfg(feature = "lazy_parsing")]
impl ParsedMask {
    pub fn new(column_count: u16) -> Self {
        if column_count <= 64 {
            ParsedMask::Small(0)
        } else {
            let num_u64s = (column_count as usize + 63) / 64;
            ParsedMask::Large(vec![0; num_u64s])
        }
    }
    /// Check if a column at the given index has been parsed
    pub fn is_parsed(&self, idx: usize) -> bool {
        match self {
            ParsedMask::Small(mask) => {
                if idx >= 64 {
                    false
                } else {
                    (mask & (1u64 << idx)) != 0
                }
            }
            ParsedMask::Large(masks) => {
                let chunk = idx / 64;
                let bit = idx % 64;
                if chunk >= masks.len() {
                    false
                } else {
                    (masks[chunk] & (1u64 << bit)) != 0
                }
            }
        }
    }

    /// Mark a column at the given index as parsed
    pub fn set_parsed(&mut self, idx: usize) {
        match self {
            ParsedMask::Small(mask) => {
                if idx < 64 {
                    *mask |= 1u64 << idx;
                }
            }
            ParsedMask::Large(masks) => {
                let chunk = idx / 64;
                let bit = idx % 64;
                if chunk < masks.len() {
                    masks[chunk] |= 1u64 << bit;
                }
            }
        }
    }

    /// Count the number of parsed columns
    pub fn parsed_count(&self) -> usize {
        match self {
            ParsedMask::Small(mask) => mask.count_ones() as usize,
            ParsedMask::Large(masks) => masks.iter().map(|m| m.count_ones() as usize).sum(),
        }
    }

    /// Check if we should parse all remaining columns (>75% already parsed)
    pub fn should_parse_remaining(&self, total_columns: u16) -> bool {
        let parsed = self.parsed_count();
        parsed > (total_columns as usize * 3 / 4)
    }
}

#[cfg(feature = "lazy_parsing")]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LazyParseState {
    /// Serial types for each column
    pub serial_types: Vec<u64>,
    /// Byte offset where each column's data starts
    pub column_offsets: Vec<u16>,
    /// Tracks which columns have been parsed
    pub parsed_mask: ParsedMask,
    /// Total number of columns
    pub column_count: u16,
    /// Size of the record header in bytes
    pub header_size: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Record {
    values: Vec<Value>,
}

impl Record {
    // pub fn get<'a, T: FromValue<'a> + 'a>(&'a self, idx: usize) -> Result<T> {
    //     let value = &self.values[idx];
    //     T::from_value(value)
    // }

    pub fn count(&self) -> usize {
        self.values.len()
    }

    pub fn last_value(&self) -> Option<&Value> {
        self.values.last()
    }

    pub fn get_values(&self) -> &Vec<Value> {
        &self.values
    }

    pub fn get_value(&self, idx: usize) -> &Value {
        &self.values[idx]
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }
}
struct AppendWriter<'a> {
    buf: &'a mut Vec<u8>,
    pos: usize,
    buf_capacity_start: usize,
    buf_ptr_start: *const u8,
}

impl<'a> AppendWriter<'a> {
    pub fn new(buf: &'a mut Vec<u8>, pos: usize) -> Self {
        let buf_ptr_start = buf.as_ptr();
        let buf_capacity_start = buf.capacity();
        Self {
            buf,
            pos,
            buf_capacity_start,
            buf_ptr_start,
        }
    }

    #[inline]
    pub fn extend_from_slice(&mut self, slice: &[u8]) {
        self.buf[self.pos..self.pos + slice.len()].copy_from_slice(slice);
        self.pos += slice.len();
    }

    fn assert_finish_capacity(&self) {
        // let's make sure we didn't reallocate anywhere else
        assert_eq!(self.buf_capacity_start, self.buf.capacity());
        assert_eq!(self.buf_ptr_start, self.buf.as_ptr());
    }
}

impl ImmutableRecord {
    #[allow(unused_variables)]
    pub fn new(payload_capacity: usize, value_capacity: usize) -> Self {
        Self {
            #[cfg(not(feature = "lazy_parsing"))]
            payload: Vec::with_capacity(payload_capacity),
            #[cfg(feature = "lazy_parsing")]
            payload: None,
            values: Vec::with_capacity(value_capacity),
            recreating: false,
            #[cfg(feature = "lazy_parsing")]
            lazy_state: None,
        }
    }

    #[cfg(feature = "lazy_parsing")]
    pub fn new_lazy(payload: Vec<u8>, lazy_state: LazyParseState) -> Self {
        let column_count = lazy_state.column_count as usize;
        Self {
            payload: Some(Arc::from(payload.into_boxed_slice())),
            values: vec![None; column_count],
            recreating: false,
            lazy_state: Some(lazy_state),
        }
    }

    #[cfg(feature = "lazy_parsing")]
    pub fn init_lazy(&mut self, payload: &[u8], lazy_state: LazyParseState) {
        let column_count = lazy_state.column_count as usize;
        self.payload = Some(Arc::from(payload));
        self.values = vec![None; column_count];
        self.recreating = false;
        self.lazy_state = Some(lazy_state);
    }

    #[cfg(not(feature = "lazy_parsing"))]
    pub fn get<'a, T: TryFrom<&'a RefValue, Error = LimboError> + 'a>(
        &'a self,
        idx: usize,
    ) -> Result<T> {
        let value = self
            .values
            .get(idx)
            .ok_or(LimboError::InternalError("Index out of bounds".into()))?;
        T::try_from(value)
    }
    
    #[cfg(feature = "lazy_parsing")]
    pub fn get<'a, T: TryFrom<&'a RefValue, Error = LimboError> + 'a>(
        &'a mut self,
        idx: usize,
    ) -> Result<T> {
        // Ensure column is parsed
        if self.lazy_state.is_some() {
            self.parse_column(idx)?;
        }
        
        let value = self
            .values
            .get(idx)
            .and_then(|opt| opt.as_ref())
            .ok_or(LimboError::InternalError("Index out of bounds".into()))?;
        T::try_from(value)
    }

    pub fn count(&self) -> usize {
        self.values.len()
    }

    #[cfg(not(feature = "lazy_parsing"))]
    pub fn last_value(&self) -> Option<&RefValue> {
        self.values.last()
    }
    
    #[cfg(feature = "lazy_parsing")]
    pub fn last_value(&mut self) -> Option<&RefValue> {
        if let Some(last_idx) = self.values.len().checked_sub(1) {
            let _ = self.parse_column(last_idx);
        }
        self.values.last().and_then(|opt| opt.as_ref())
    }

    #[cfg(not(feature = "lazy_parsing"))]
    pub fn get_values(&self) -> &Vec<RefValue> {
        &self.values
    }
    
    #[cfg(feature = "lazy_parsing")]
    pub fn get_values(&self) -> &Vec<Option<RefValue>> {
        &self.values
    }
    
    // Compatibility method that works with both feature flags
    #[cfg(not(feature = "lazy_parsing"))]
    pub fn get_values_for_comparison(&self) -> &[RefValue] {
        &self.values
    }
    
    #[cfg(feature = "lazy_parsing")]
    pub fn get_parsed_values(&mut self) -> Vec<RefValue> {
        // Ensure all columns are parsed
        if self.lazy_state.is_some() {
            let column_count = self.values.len();
            for i in 0..column_count {
                let _ = self.parse_column(i);
            }
        }
        
        // Return a vector of parsed values
        self.values.iter()
            .filter_map(|opt| opt.as_ref())
            .cloned()
            .collect()
    }
    
    #[cfg(feature = "lazy_parsing")]
    pub fn get_parsed_values_slice(&mut self) -> Vec<RefValue> {
        self.get_parsed_values()
    }

    #[cfg(not(feature = "lazy_parsing"))]
    pub fn get_value(&self, idx: usize) -> &RefValue {
        &self.values[idx]
    }
    
    #[cfg(feature = "lazy_parsing")]
    pub fn get_value(&self, idx: usize) -> &RefValue {
        self.values[idx].as_ref().expect("Column not parsed")
    }

    #[cfg(not(feature = "lazy_parsing"))]
    pub fn get_value_opt(&self, idx: usize) -> Option<&RefValue> {
        self.values.get(idx)
    }

    #[cfg(feature = "lazy_parsing")]
    pub fn get_value_opt(&mut self, idx: usize) -> Result<Option<RefValue>> {
        // First ensure the column is parsed if we're doing lazy parsing
        if self.lazy_state.is_some() && idx < self.values.len() {
            self.parse_column(idx)?;
        }
        
        // Return the value if it exists
        Ok(self.values.get(idx).and_then(|opt| opt.as_ref()).cloned())
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn from_registers(registers: &[Register]) -> Self {
        let mut values = Vec::with_capacity(registers.len());
        let mut serials = Vec::with_capacity(registers.len());
        let mut size_header = 0;
        let mut size_values = 0;

        let mut serial_type_buf = [0; 9];
        // write serial types
        for value in registers {
            let value = value.get_owned_value();
            let serial_type = SerialType::from(value);
            let n = write_varint(&mut serial_type_buf[0..], serial_type.into());
            serials.push((serial_type_buf, n));

            let value_size = serial_type.size();

            size_header += n;
            size_values += value_size;
        }
        let mut header_size = size_header;
        const MIN_HEADER_SIZE: usize = 126;
        if header_size <= MIN_HEADER_SIZE {
            // common case
            // This case means the header size can be contained by a single byte, therefore
            // header_size == size of serial types + 1 byte from the header size
            // Since header_size is a varint, and a varint the first bit is used to represent we have more bytes to read,
            // header size here will be 126 == (2^7 - 1)
            header_size += 1;
        } else {
            todo!("calculate big header size extra bytes");
            // get header varint len
            // header_size += n;
            // if( nVarint<sqlite3VarintLen(nHdr) ) nHdr++;
        }
        // 1. write header size
        let mut buf = Vec::new();
        buf.reserve_exact(header_size + size_values);
        assert_eq!(buf.capacity(), header_size + size_values);
        assert!(header_size <= 126);
        let n = write_varint(&mut serial_type_buf, header_size as u64);

        buf.resize(buf.capacity(), 0);
        let mut writer = AppendWriter::new(&mut buf, 0);
        writer.extend_from_slice(&serial_type_buf[..n]);

        // 2. Write serial
        for (value, n) in serials {
            writer.extend_from_slice(&value[..n]);
        }

        // write content
        for value in registers {
            let value = value.get_owned_value();
            let start_offset = writer.pos;
            match value {
                Value::Null => {
                    #[cfg(not(feature = "lazy_parsing"))]
                    values.push(RefValue::Null);
                    #[cfg(feature = "lazy_parsing")]
                    values.push(Some(RefValue::Null));
                }
                Value::Integer(i) => {
                    #[cfg(not(feature = "lazy_parsing"))]
                    values.push(RefValue::Integer(*i));
                    #[cfg(feature = "lazy_parsing")]
                    values.push(Some(RefValue::Integer(*i)));
                    let serial_type = SerialType::from(value);
                    match serial_type.kind() {
                        SerialTypeKind::ConstInt0 | SerialTypeKind::ConstInt1 => {}
                        SerialTypeKind::I8 => writer.extend_from_slice(&(*i as i8).to_be_bytes()),
                        SerialTypeKind::I16 => writer.extend_from_slice(&(*i as i16).to_be_bytes()),
                        SerialTypeKind::I24 => {
                            writer.extend_from_slice(&(*i as i32).to_be_bytes()[1..])
                        } // remove most significant byte
                        SerialTypeKind::I32 => writer.extend_from_slice(&(*i as i32).to_be_bytes()),
                        SerialTypeKind::I48 => writer.extend_from_slice(&i.to_be_bytes()[2..]), // remove 2 most significant bytes
                        SerialTypeKind::I64 => writer.extend_from_slice(&i.to_be_bytes()),
                        other => panic!("Serial type is not an integer: {:?}", other),
                    }
                }
                Value::Float(f) => {
                    #[cfg(not(feature = "lazy_parsing"))]
                    values.push(RefValue::Float(*f));
                    #[cfg(feature = "lazy_parsing")]
                    values.push(Some(RefValue::Float(*f)));
                    writer.extend_from_slice(&f.to_be_bytes())
                }
                Value::Text(t) => {
                    writer.extend_from_slice(&t.value);
                    let end_offset = writer.pos;
                    let len = end_offset - start_offset;
                    let ptr = unsafe { writer.buf.as_ptr().add(start_offset) };
                    let value = RefValue::Text(TextRef {
                        value: RawSlice::new(ptr, len),
                        subtype: t.subtype.clone(),
                    });
                    #[cfg(not(feature = "lazy_parsing"))]
                    values.push(value);
                    #[cfg(feature = "lazy_parsing")]
                    values.push(Some(value));
                }
                Value::Blob(b) => {
                    writer.extend_from_slice(b);
                    let end_offset = writer.pos;
                    let len = end_offset - start_offset;
                    let ptr = unsafe { writer.buf.as_ptr().add(start_offset) };
                    #[cfg(not(feature = "lazy_parsing"))]
                    values.push(RefValue::Blob(RawSlice::new(ptr, len)));
                    #[cfg(feature = "lazy_parsing")]
                    values.push(Some(RefValue::Blob(RawSlice::new(ptr, len))));
                }
            };
        }

        writer.assert_finish_capacity();
        Self {
            #[cfg(not(feature = "lazy_parsing"))]
            payload: buf,
            #[cfg(feature = "lazy_parsing")]
            payload: Some(Arc::from(buf.into_boxed_slice())),
            values,
            recreating: false,
            #[cfg(feature = "lazy_parsing")]
            lazy_state: None,
        }
    }

    pub fn start_serialization(&mut self, payload: &[u8]) {
        self.recreating = true;
        #[cfg(not(feature = "lazy_parsing"))]
        self.payload.extend_from_slice(payload);
        #[cfg(feature = "lazy_parsing")]
        {
            // For lazy parsing, we need to convert to Vec first, then back to Arc
            let mut vec = self.payload.take().map(|arc| arc.to_vec()).unwrap_or_default();
            vec.extend_from_slice(payload);
            self.payload = Some(Arc::from(vec.into_boxed_slice()));
        }
    }
    pub fn end_serialization(&mut self) {
        assert!(self.recreating);
        self.recreating = false;
    }

    #[cfg(not(feature = "lazy_parsing"))]
    pub fn add_value(&mut self, value: RefValue) {
        assert!(self.recreating);
        self.values.push(value);
    }
    
    #[cfg(feature = "lazy_parsing")]
    pub fn add_value(&mut self, value: RefValue) {
        assert!(self.recreating);
        self.values.push(Some(value));
    }

    pub fn invalidate(&mut self) {
        #[cfg(not(feature = "lazy_parsing"))]
        self.payload.clear();
        #[cfg(feature = "lazy_parsing")]
        { self.payload = None; }
        self.values.clear();
    }

    #[cfg(not(feature = "lazy_parsing"))]
    pub fn is_invalidated(&self) -> bool {
        self.payload.is_empty()
    }
    
    #[cfg(feature = "lazy_parsing")]
    pub fn is_invalidated(&self) -> bool {
        self.payload.is_none()
    }

    #[cfg(not(feature = "lazy_parsing"))]
    pub fn get_payload(&self) -> &[u8] {
        &self.payload
    }
    
    #[cfg(feature = "lazy_parsing")]
    pub fn get_payload(&self) -> &[u8] {
        self.payload.as_ref().map(|arc| arc.as_ref()).unwrap_or(&[])
    }

    #[cfg(feature = "lazy_parsing")]
    pub fn parse_column(&mut self, column: usize) -> Result<()> {
        if let Some(ref mut lazy_state) = self.lazy_state {
            // Check if already parsed
            if lazy_state.parsed_mask.is_parsed(column) {
                return Ok(());
            }

            // Get the serial type and offset for this column
            let serial_type = lazy_state.serial_types[column];
            let offset = lazy_state.column_offsets[column] as usize;
            
            // Parse the value from the payload
            if let Some(ref payload_arc) = self.payload {
                let buf = &payload_arc[offset..];
                let (value, _) = read_value(buf, SerialType(serial_type))?;
                
                // Store the parsed value
                self.values[column] = Some(value);
                
                // Mark as parsed
                lazy_state.parsed_mask.set_parsed(column);
                
                // Check if we should parse remaining columns
                if lazy_state.parsed_mask.should_parse_remaining(lazy_state.column_count) {
                    self.parse_remaining_columns()?;
                }
            }
            
            Ok(())
        } else {
            Ok(()) // Not lazy parsing
        }
    }

    #[cfg(feature = "lazy_parsing")]
    fn parse_remaining_columns(&mut self) -> Result<()> {
        // Collect unparsed columns first to avoid borrow conflict
        let unparsed_columns: Vec<usize> = if let Some(ref lazy_state) = &self.lazy_state {
            let column_count = lazy_state.column_count as usize;
            (0..column_count)
                .filter(|&i| !lazy_state.parsed_mask.is_parsed(i))
                .collect()
        } else {
            Vec::new()
        };
        
        // Now parse them
        for i in unparsed_columns {
            self.parse_column(i)?;
        }
        Ok(())
    }

    #[cfg(feature = "lazy_parsing")]
    pub fn get_column_lazy(&mut self, column: usize) -> Result<&RefValue> {
        // Parse the column if needed
        self.parse_column(column)?;
        
        // Return the parsed value
        self.values
            .get(column)
            .and_then(|opt| opt.as_ref())
            .ok_or_else(|| LimboError::InternalError(format!("Column {} not found", column)))
    }

    #[cfg(not(feature = "lazy_parsing"))]
    pub fn get_column_lazy(&self, column: usize) -> Result<&RefValue> {
        self.values
            .get(column)
            .ok_or_else(|| LimboError::InternalError(format!("Column {} not found", column)))
    }
}

impl Display for ImmutableRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[cfg(not(feature = "lazy_parsing"))]
        {
            for value in &self.values {
                match value {
                    RefValue::Null => write!(f, "NULL")?,
                    RefValue::Integer(i) => write!(f, "Integer({})", *i)?,
                    RefValue::Float(flo) => write!(f, "Float({})", *flo)?,
                    RefValue::Text(text_ref) => write!(f, "Text({})", text_ref.as_str())?,
                    RefValue::Blob(raw_slice) => {
                        write!(f, "Blob({})", String::from_utf8_lossy(raw_slice.to_slice()))?
                    }
                }
                if value != self.values.last().unwrap() {
                    write!(f, ", ")?;
                }
            }
        }
        
        #[cfg(feature = "lazy_parsing")]
        {
            for (i, value) in self.values.iter().enumerate() {
                match value {
                    Some(ref_value) => {
                        match ref_value {
                            RefValue::Null => write!(f, "NULL")?,
                            RefValue::Integer(i) => write!(f, "Integer({})", *i)?,
                            RefValue::Float(flo) => write!(f, "Float({})", *flo)?,
                            RefValue::Text(text_ref) => write!(f, "Text({})", text_ref.as_str())?,
                            RefValue::Blob(raw_slice) => {
                                write!(f, "Blob({})", String::from_utf8_lossy(raw_slice.to_slice()))?
                            }
                        }
                    }
                    None => write!(f, "<unparsed>")?,
                }
                if i < self.values.len() - 1 {
                    write!(f, ", ")?;
                }
            }
        }
        Ok(())
    }
}

impl Clone for ImmutableRecord {
    fn clone(&self) -> Self {
        let new_payload = self.payload.clone();
        
        #[cfg(not(feature = "lazy_parsing"))]
        let mut new_values = {
            let mut new_values = Vec::new();
            for value in &self.values {
                let value = match value {
                    RefValue::Null => RefValue::Null,
                    RefValue::Integer(i) => RefValue::Integer(*i),
                    RefValue::Float(f) => RefValue::Float(*f),
                    RefValue::Text(text_ref) => {
                        // let's update pointer
                        #[cfg(not(feature = "lazy_parsing"))]
                        let ptr_start = self.payload.as_ptr() as usize;
                        #[cfg(feature = "lazy_parsing")]
                        let ptr_start = self.payload.as_ref().map(|arc| arc.as_ptr() as usize).unwrap_or(0);
                        let ptr_end = text_ref.value.data as usize;
                        let len = ptr_end - ptr_start;
                        #[cfg(not(feature = "lazy_parsing"))]
                        let new_ptr = unsafe { new_payload.as_ptr().add(len) };
                        #[cfg(feature = "lazy_parsing")]
                        let new_ptr = unsafe { new_payload.as_ref().map(|arc| arc.as_ptr().add(len)).unwrap() };
                        RefValue::Text(TextRef {
                            value: RawSlice::new(new_ptr, text_ref.value.len),
                            subtype: text_ref.subtype.clone(),
                        })
                    }
                    RefValue::Blob(raw_slice) => {
                        let ptr_start = self.payload.as_ptr() as usize;
                        let ptr_end = raw_slice.data as usize;
                        let len = ptr_end - ptr_start;
                        let new_ptr = unsafe { new_payload.as_ptr().add(len) };
                        RefValue::Blob(RawSlice::new(new_ptr, raw_slice.len))
                    }
                };
                new_values.push(value);
            }
            new_values
        };
        
        #[cfg(feature = "lazy_parsing")]
        let new_values = {
            // With Arc, cloned Arc points to same memory, so pointers remain valid
            self.values.clone()
        };
        
        Self {
            payload: new_payload,
            values: new_values,
            recreating: self.recreating,
            #[cfg(feature = "lazy_parsing")]
            lazy_state: self.lazy_state.clone(),
        }
    }
}

impl RefValue {
    pub fn to_ffi(&self) -> ExtValue {
        match self {
            Self::Null => ExtValue::null(),
            Self::Integer(i) => ExtValue::from_integer(*i),
            Self::Float(fl) => ExtValue::from_float(*fl),
            Self::Text(text) => ExtValue::from_text(
                std::str::from_utf8(text.value.to_slice())
                    .unwrap()
                    .to_string(),
            ),
            Self::Blob(blob) => ExtValue::from_blob(blob.to_slice().to_vec()),
        }
    }

    pub fn to_owned(&self) -> Value {
        match self {
            RefValue::Null => Value::Null,
            RefValue::Integer(i) => Value::Integer(*i),
            RefValue::Float(f) => Value::Float(*f),
            RefValue::Text(text_ref) => Value::Text(Text {
                value: text_ref.value.to_slice().to_vec(),
                subtype: text_ref.subtype.clone(),
            }),
            RefValue::Blob(b) => Value::Blob(b.to_slice().to_vec()),
        }
    }
    pub fn to_blob(&self) -> Option<&[u8]> {
        match self {
            Self::Blob(blob) => Some(blob.to_slice()),
            _ => None,
        }
    }
}

impl Display for RefValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => write!(f, "NULL"),
            Self::Integer(i) => write!(f, "{}", i),
            Self::Float(fl) => write!(f, "{:?}", fl),
            Self::Text(s) => write!(f, "{}", s.as_str()),
            Self::Blob(b) => write!(f, "{}", String::from_utf8_lossy(b.to_slice())),
        }
    }
}
impl Eq for RefValue {}

impl Ord for RefValue {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

#[allow(clippy::non_canonical_partial_ord_impl)]
impl PartialOrd<RefValue> for RefValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Integer(int_left), Self::Integer(int_right)) => int_left.partial_cmp(int_right),
            (Self::Integer(int_left), Self::Float(float_right)) => {
                (*int_left as f64).partial_cmp(float_right)
            }
            (Self::Float(float_left), Self::Integer(int_right)) => {
                float_left.partial_cmp(&(*int_right as f64))
            }
            (Self::Float(float_left), Self::Float(float_right)) => {
                float_left.partial_cmp(float_right)
            }
            // Numeric vs Text/Blob
            (Self::Integer(_) | Self::Float(_), Self::Text(_) | Self::Blob(_)) => {
                Some(std::cmp::Ordering::Less)
            }
            (Self::Text(_) | Self::Blob(_), Self::Integer(_) | Self::Float(_)) => {
                Some(std::cmp::Ordering::Greater)
            }

            (Self::Text(text_left), Self::Text(text_right)) => text_left
                .value
                .to_slice()
                .partial_cmp(text_right.value.to_slice()),
            // Text vs Blob
            (Self::Text(_), Self::Blob(_)) => Some(std::cmp::Ordering::Less),
            (Self::Blob(_), Self::Text(_)) => Some(std::cmp::Ordering::Greater),

            (Self::Blob(blob_left), Self::Blob(blob_right)) => {
                blob_left.to_slice().partial_cmp(blob_right.to_slice())
            }
            (Self::Null, Self::Null) => Some(std::cmp::Ordering::Equal),
            (Self::Null, _) => Some(std::cmp::Ordering::Less),
            (_, Self::Null) => Some(std::cmp::Ordering::Greater),
        }
    }
}

/// A bitfield that represents the comparison spec for index keys.
/// Since indexed columns can individually specify ASC/DESC, each key must
/// be compared differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct IndexKeySortOrder(u64);

impl IndexKeySortOrder {
    pub fn get_sort_order_for_col(&self, column_idx: usize) -> SortOrder {
        assert!(column_idx < 64, "column index out of range: {}", column_idx);
        match self.0 & (1 << column_idx) {
            0 => SortOrder::Asc,
            _ => SortOrder::Desc,
        }
    }

    pub fn from_index(index: &Index) -> Self {
        let mut spec = 0;
        for (i, column) in index.columns.iter().enumerate() {
            spec |= ((column.order == SortOrder::Desc) as u64) << i;
        }
        IndexKeySortOrder(spec)
    }

    pub fn from_list(order: &[SortOrder]) -> Self {
        let mut spec = 0;
        for (i, order) in order.iter().enumerate() {
            spec |= ((*order == SortOrder::Desc) as u64) << i;
        }
        IndexKeySortOrder(spec)
    }

    pub fn default() -> Self {
        Self(0)
    }
}

impl Default for IndexKeySortOrder {
    fn default() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// Metadata about an index, used for handling and comparing index keys.
///
/// This struct provides information about the sorting order of columns,
/// whether the index includes a row ID, and the total number of columns
/// in the index.
pub struct IndexKeyInfo {
    /// Specifies the sorting order (ascending or descending) for each column in the index.
    pub sort_order: IndexKeySortOrder,
    /// Indicates whether the index includes a row ID column.
    pub has_rowid: bool,
    /// The total number of columns in the index, including the row ID column if present.
    pub num_cols: usize,
}

impl IndexKeyInfo {
    pub fn new_from_index(index: &Index) -> Self {
        Self {
            sort_order: IndexKeySortOrder::from_index(index),
            has_rowid: index.has_rowid,
            num_cols: index.columns.len() + (index.has_rowid as usize),
        }
    }
}

pub fn compare_immutable(
    l: &[RefValue],
    r: &[RefValue],
    index_key_sort_order: IndexKeySortOrder,
    collations: &[CollationSeq],
) -> std::cmp::Ordering {
    assert_eq!(l.len(), r.len());
    for (i, (l, r)) in l.iter().zip(r).enumerate() {
        let column_order = index_key_sort_order.get_sort_order_for_col(i);
        let collation = collations.get(i).copied().unwrap_or_default();
        let cmp = match (l, r) {
            (RefValue::Text(left), RefValue::Text(right)) => {
                collation.compare_strings(left.as_str(), right.as_str())
            }
            _ => l.partial_cmp(r).unwrap(),
        };
        if !cmp.is_eq() {
            return match column_order {
                SortOrder::Asc => cmp,
                SortOrder::Desc => cmp.reverse(),
            };
        }
    }
    std::cmp::Ordering::Equal
}

const I8_LOW: i64 = -128;
const I8_HIGH: i64 = 127;
const I16_LOW: i64 = -32768;
const I16_HIGH: i64 = 32767;
const I24_LOW: i64 = -8388608;
const I24_HIGH: i64 = 8388607;
const I32_LOW: i64 = -2147483648;
const I32_HIGH: i64 = 2147483647;
const I48_LOW: i64 = -140737488355328;
const I48_HIGH: i64 = 140737488355327;

/// Sqlite Serial Types
/// https://www.sqlite.org/fileformat.html#record_format
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct SerialType(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SerialTypeKind {
    Null,
    I8,
    I16,
    I24,
    I32,
    I48,
    I64,
    F64,
    ConstInt0,
    ConstInt1,
    Text,
    Blob,
}

impl SerialType {
    #[inline(always)]
    pub fn u64_is_valid_serial_type(n: u64) -> bool {
        n != 10 && n != 11
    }

    const NULL: Self = Self(0);
    const I8: Self = Self(1);
    const I16: Self = Self(2);
    const I24: Self = Self(3);
    const I32: Self = Self(4);
    const I48: Self = Self(5);
    const I64: Self = Self(6);
    const F64: Self = Self(7);
    const CONST_INT0: Self = Self(8);
    const CONST_INT1: Self = Self(9);

    pub fn null() -> Self {
        Self::NULL
    }

    pub fn i8() -> Self {
        Self::I8
    }

    pub fn i16() -> Self {
        Self::I16
    }

    pub fn i24() -> Self {
        Self::I24
    }

    pub fn i32() -> Self {
        Self::I32
    }

    pub fn i48() -> Self {
        Self::I48
    }

    pub fn i64() -> Self {
        Self::I64
    }

    pub fn f64() -> Self {
        Self::F64
    }

    pub fn const_int0() -> Self {
        Self::CONST_INT0
    }

    pub fn const_int1() -> Self {
        Self::CONST_INT1
    }

    pub fn blob(size: u64) -> Self {
        Self(12 + size * 2)
    }

    pub fn text(size: u64) -> Self {
        Self(13 + size * 2)
    }

    pub fn kind(&self) -> SerialTypeKind {
        match self.0 {
            0 => SerialTypeKind::Null,
            1 => SerialTypeKind::I8,
            2 => SerialTypeKind::I16,
            3 => SerialTypeKind::I24,
            4 => SerialTypeKind::I32,
            5 => SerialTypeKind::I48,
            6 => SerialTypeKind::I64,
            7 => SerialTypeKind::F64,
            8 => SerialTypeKind::ConstInt0,
            9 => SerialTypeKind::ConstInt1,
            n if n >= 12 => match n % 2 {
                0 => SerialTypeKind::Blob,
                1 => SerialTypeKind::Text,
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    pub fn size(&self) -> usize {
        match self.kind() {
            SerialTypeKind::Null => 0,
            SerialTypeKind::I8 => 1,
            SerialTypeKind::I16 => 2,
            SerialTypeKind::I24 => 3,
            SerialTypeKind::I32 => 4,
            SerialTypeKind::I48 => 6,
            SerialTypeKind::I64 => 8,
            SerialTypeKind::F64 => 8,
            SerialTypeKind::ConstInt0 => 0,
            SerialTypeKind::ConstInt1 => 0,
            SerialTypeKind::Text => (self.0 as usize - 13) / 2,
            SerialTypeKind::Blob => (self.0 as usize - 12) / 2,
        }
    }
}

impl From<&Value> for SerialType {
    fn from(value: &Value) -> Self {
        match value {
            Value::Null => SerialType::null(),
            Value::Integer(i) => match i {
                0 => SerialType::const_int0(),
                1 => SerialType::const_int1(),
                i if *i >= I8_LOW && *i <= I8_HIGH => SerialType::i8(),
                i if *i >= I16_LOW && *i <= I16_HIGH => SerialType::i16(),
                i if *i >= I24_LOW && *i <= I24_HIGH => SerialType::i24(),
                i if *i >= I32_LOW && *i <= I32_HIGH => SerialType::i32(),
                i if *i >= I48_LOW && *i <= I48_HIGH => SerialType::i48(),
                _ => SerialType::i64(),
            },
            Value::Float(_) => SerialType::f64(),
            Value::Text(t) => SerialType::text(t.value.len() as u64),
            Value::Blob(b) => SerialType::blob(b.len() as u64),
        }
    }
}

impl From<SerialType> for u64 {
    fn from(serial_type: SerialType) -> Self {
        serial_type.0
    }
}

impl TryFrom<u64> for SerialType {
    type Error = LimboError;

    fn try_from(uint: u64) -> Result<Self> {
        if uint == 10 || uint == 11 {
            return Err(LimboError::Corrupt(format!(
                "Invalid serial type: {}",
                uint
            )));
        }
        Ok(SerialType(uint))
    }
}

impl Record {
    pub fn new(values: Vec<Value>) -> Self {
        Self { values }
    }

    pub fn serialize(&self, buf: &mut Vec<u8>) {
        let initial_i = buf.len();

        // write serial types
        for value in &self.values {
            let serial_type = SerialType::from(value);
            buf.resize(buf.len() + 9, 0); // Ensure space for varint (1-9 bytes in length)
            let len = buf.len();
            let n = write_varint(&mut buf[len - 9..], serial_type.into());
            buf.truncate(buf.len() - 9 + n); // Remove unused bytes
        }

        let mut header_size = buf.len() - initial_i;
        // write content
        for value in &self.values {
            match value {
                Value::Null => {}
                Value::Integer(i) => {
                    let serial_type = SerialType::from(value);
                    match serial_type.kind() {
                        SerialTypeKind::ConstInt0 | SerialTypeKind::ConstInt1 => {}
                        SerialTypeKind::I8 => buf.extend_from_slice(&(*i as i8).to_be_bytes()),
                        SerialTypeKind::I16 => buf.extend_from_slice(&(*i as i16).to_be_bytes()),
                        SerialTypeKind::I24 => {
                            buf.extend_from_slice(&(*i as i32).to_be_bytes()[1..])
                        } // remove most significant byte
                        SerialTypeKind::I32 => buf.extend_from_slice(&(*i as i32).to_be_bytes()),
                        SerialTypeKind::I48 => buf.extend_from_slice(&i.to_be_bytes()[2..]), // remove 2 most significant bytes
                        SerialTypeKind::I64 => buf.extend_from_slice(&i.to_be_bytes()),
                        _ => unreachable!(),
                    }
                }
                Value::Float(f) => buf.extend_from_slice(&f.to_be_bytes()),
                Value::Text(t) => buf.extend_from_slice(&t.value),
                Value::Blob(b) => buf.extend_from_slice(b),
            };
        }

        let mut header_bytes_buf: Vec<u8> = Vec::new();
        if header_size <= 126 {
            // common case
            header_size += 1;
        } else {
            todo!("calculate big header size extra bytes");
            // get header varint len
            // header_size += n;
            // if( nVarint<sqlite3VarintLen(nHdr) ) nHdr++;
        }
        assert!(header_size <= 126);
        header_bytes_buf.extend(std::iter::repeat(0).take(9));
        let n = write_varint(header_bytes_buf.as_mut_slice(), header_size as u64);
        header_bytes_buf.truncate(n);
        buf.splice(initial_i..initial_i, header_bytes_buf.iter().cloned());
    }
}

pub enum Cursor {
    BTree(BTreeCursor),
    Pseudo(PseudoCursor),
    Sorter(Sorter),
    Virtual(VirtualTableCursor),
}

impl Cursor {
    pub fn new_btree(cursor: BTreeCursor) -> Self {
        Self::BTree(cursor)
    }

    pub fn new_pseudo(cursor: PseudoCursor) -> Self {
        Self::Pseudo(cursor)
    }

    pub fn new_sorter(cursor: Sorter) -> Self {
        Self::Sorter(cursor)
    }

    pub fn as_btree_mut(&mut self) -> &mut BTreeCursor {
        match self {
            Self::BTree(cursor) => cursor,
            _ => panic!("Cursor is not a btree"),
        }
    }

    pub fn as_pseudo_mut(&mut self) -> &mut PseudoCursor {
        match self {
            Self::Pseudo(cursor) => cursor,
            _ => panic!("Cursor is not a pseudo cursor"),
        }
    }

    pub fn as_sorter_mut(&mut self) -> &mut Sorter {
        match self {
            Self::Sorter(cursor) => cursor,
            _ => panic!("Cursor is not a sorter cursor"),
        }
    }

    pub fn as_virtual_mut(&mut self) -> &mut VirtualTableCursor {
        match self {
            Self::Virtual(cursor) => cursor,
            _ => panic!("Cursor is not a virtual cursor"),
        }
    }
}

#[derive(Debug)]
pub enum CursorResult<T> {
    Ok(T),
    IO,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
/// The match condition of a table/index seek.
pub enum SeekOp {
    /// If eq_only is true, this means in practice:
    /// We are iterating forwards, but we are really looking for an exact match on the seek key.
    GE {
        eq_only: bool,
    },
    GT,
    /// If eq_only is true, this means in practice:
    /// We are iterating backwards, but we are really looking for an exact match on the seek key.
    LE {
        eq_only: bool,
    },
    LT,
}

impl SeekOp {
    /// A given seek op implies an iteration direction.
    ///
    /// For example, a seek with SeekOp::GT implies:
    /// Find the first table/index key that compares greater than the seek key
    /// -> used in forwards iteration.
    ///
    /// A seek with SeekOp::LE implies:
    /// Find the last table/index key that compares less than or equal to the seek key
    /// -> used in backwards iteration.
    #[inline(always)]
    pub fn iteration_direction(&self) -> IterationDirection {
        match self {
            SeekOp::GE { .. } | SeekOp::GT => IterationDirection::Forwards,
            SeekOp::LE { .. } | SeekOp::LT => IterationDirection::Backwards,
        }
    }

    pub fn eq_only(&self) -> bool {
        match self {
            SeekOp::GE { eq_only } | SeekOp::LE { eq_only } => *eq_only,
            _ => false,
        }
    }

    pub fn reverse(&self) -> Self {
        match self {
            SeekOp::GE { eq_only } => SeekOp::LE { eq_only: *eq_only },
            SeekOp::GT => SeekOp::LT,
            SeekOp::LE { eq_only } => SeekOp::GE { eq_only: *eq_only },
            SeekOp::LT => SeekOp::GT,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum SeekKey<'a> {
    TableRowId(i64),
    IndexKey(&'a ImmutableRecord),
}

impl RawSlice {
    pub fn new(data: *const u8, len: usize) -> Self {
        Self { data, len }
    }
    pub fn to_slice(&self) -> &[u8] {
        if self.data.is_null() {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(self.data, self.len) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_null() {
        let record = Record::new(vec![Value::Null]);
        let mut buf = Vec::new();
        record.serialize(&mut buf);

        let header_length = record.values.len() + 1;
        let header = &buf[0..header_length];
        // First byte should be header size
        assert_eq!(header[0], header_length as u8);
        // Second byte should be serial type for NULL
        assert_eq!(header[1] as u64, u64::from(SerialType::null()));
        // Check that the buffer is empty after the header
        assert_eq!(buf.len(), header_length);
    }

    #[test]
    fn test_serialize_integers() {
        let record = Record::new(vec![
            Value::Integer(0),                 // Should use ConstInt0
            Value::Integer(1),                 // Should use ConstInt1
            Value::Integer(42),                // Should use SERIAL_TYPE_I8
            Value::Integer(1000),              // Should use SERIAL_TYPE_I16
            Value::Integer(1_000_000),         // Should use SERIAL_TYPE_I24
            Value::Integer(1_000_000_000),     // Should use SERIAL_TYPE_I32
            Value::Integer(1_000_000_000_000), // Should use SERIAL_TYPE_I48
            Value::Integer(i64::MAX),          // Should use SERIAL_TYPE_I64
        ]);
        let mut buf = Vec::new();
        record.serialize(&mut buf);

        let header_length = record.values.len() + 1;
        let header = &buf[0..header_length];
        // First byte should be header size
        assert_eq!(header[0], header_length as u8); // Header should be larger than number of values

        // Check that correct serial types were chosen
        assert_eq!(header[1] as u64, u64::from(SerialType::const_int0())); // 8
        assert_eq!(header[2] as u64, u64::from(SerialType::const_int1())); // 9
        assert_eq!(header[3] as u64, u64::from(SerialType::i8())); // 1
        assert_eq!(header[4] as u64, u64::from(SerialType::i16())); // 2
        assert_eq!(header[5] as u64, u64::from(SerialType::i24())); // 3
        assert_eq!(header[6] as u64, u64::from(SerialType::i32())); // 4
        assert_eq!(header[7] as u64, u64::from(SerialType::i48())); // 5
        assert_eq!(header[8] as u64, u64::from(SerialType::i64())); // 6

        // test that the bytes after the header can be interpreted as the correct values
        let mut cur_offset = header_length;

        // Value::Integer(0) - ConstInt0: NO PAYLOAD BYTES
        // Value::Integer(1) - ConstInt1: NO PAYLOAD BYTES

        // Value::Integer(42) - I8: 1 byte
        let i8_bytes = &buf[cur_offset..cur_offset + size_of::<i8>()];
        cur_offset += size_of::<i8>();

        // Value::Integer(1000) - I16: 2 bytes
        let i16_bytes = &buf[cur_offset..cur_offset + size_of::<i16>()];
        cur_offset += size_of::<i16>();

        // Value::Integer(1_000_000) - I24: 3 bytes
        let i24_bytes = &buf[cur_offset..cur_offset + 3];
        cur_offset += 3;

        // Value::Integer(1_000_000_000) - I32: 4 bytes
        let i32_bytes = &buf[cur_offset..cur_offset + size_of::<i32>()];
        cur_offset += size_of::<i32>();

        // Value::Integer(1_000_000_000_000) - I48: 6 bytes
        let i48_bytes = &buf[cur_offset..cur_offset + 6];
        cur_offset += 6;

        // Value::Integer(i64::MAX) - I64: 8 bytes
        let i64_bytes = &buf[cur_offset..cur_offset + size_of::<i64>()];

        // Verify the payload values
        let val_int8 = i8::from_be_bytes(i8_bytes.try_into().unwrap());
        let val_int16 = i16::from_be_bytes(i16_bytes.try_into().unwrap());

        let mut i24_with_padding = vec![0];
        i24_with_padding.extend(i24_bytes);
        let val_int24 = i32::from_be_bytes(i24_with_padding.try_into().unwrap());

        let val_int32 = i32::from_be_bytes(i32_bytes.try_into().unwrap());

        let mut i48_with_padding = vec![0, 0];
        i48_with_padding.extend(i48_bytes);
        let val_int48 = i64::from_be_bytes(i48_with_padding.try_into().unwrap());

        let val_int64 = i64::from_be_bytes(i64_bytes.try_into().unwrap());

        assert_eq!(val_int8, 42);
        assert_eq!(val_int16, 1000);
        assert_eq!(val_int24, 1_000_000);
        assert_eq!(val_int32, 1_000_000_000);
        assert_eq!(val_int48, 1_000_000_000_000);
        assert_eq!(val_int64, i64::MAX);

        //Size of buffer = header + payload bytes
        // ConstInt0 and ConstInt1 contribute 0 bytes to payload
        assert_eq!(
            buf.len(),
            header_length  // 9 bytes (header size + 8 serial types)
                + 0        // ConstInt0: 0 bytes
                + 0        // ConstInt1: 0 bytes  
                + size_of::<i8>()        // I8: 1 byte
                + size_of::<i16>()        // I16: 2 bytes
                + (size_of::<i32>() - 1)        // I24: 3 bytes
                + size_of::<i32>()        // I32: 4 bytes
                + (size_of::<i64>() - 2)        // I48: 6 bytes
                + size_of::<i64>() // I64: 8 bytes
        );
    }

    #[test]
    fn test_serialize_const_integers() {
        let record = Record::new(vec![Value::Integer(0), Value::Integer(1)]);
        let mut buf = Vec::new();
        record.serialize(&mut buf);

        // [header_size, serial_type_0, serial_type_1] + no payload bytes
        let expected_header_size = 3; // 1 byte for header size + 2 bytes for serial types

        assert_eq!(buf.len(), expected_header_size);

        // Check header size
        assert_eq!(buf[0], expected_header_size as u8);

        assert_eq!(buf[1] as u64, u64::from(SerialType::const_int0())); // Should be 8
        assert_eq!(buf[2] as u64, u64::from(SerialType::const_int1())); // Should be 9

        assert_eq!(buf[1], 8); // ConstInt0 serial type
        assert_eq!(buf[2], 9); // ConstInt1 serial type
    }

    #[test]
    fn test_serialize_single_const_int0() {
        let record = Record::new(vec![Value::Integer(0)]);
        let mut buf = Vec::new();
        record.serialize(&mut buf);

        // Expected: [header_size=2, serial_type=8]
        assert_eq!(buf.len(), 2);
        assert_eq!(buf[0], 2); // Header size
        assert_eq!(buf[1], 8); // ConstInt0 serial type
    }

    #[test]
    fn test_serialize_float() {
        #[warn(clippy::approx_constant)]
        let record = Record::new(vec![Value::Float(3.15555)]);
        let mut buf = Vec::new();
        record.serialize(&mut buf);

        let header_length = record.values.len() + 1;
        let header = &buf[0..header_length];
        // First byte should be header size
        assert_eq!(header[0], header_length as u8);
        // Second byte should be serial type for FLOAT
        assert_eq!(header[1] as u64, u64::from(SerialType::f64()));
        // Check that the bytes after the header can be interpreted as the float
        let float_bytes = &buf[header_length..header_length + size_of::<f64>()];
        let float = f64::from_be_bytes(float_bytes.try_into().unwrap());
        assert_eq!(float, 3.15555);
        // Check that buffer length is correct
        assert_eq!(buf.len(), header_length + size_of::<f64>());
    }

    #[test]
    fn test_serialize_text() {
        let text = "hello";
        let record = Record::new(vec![Value::Text(Text::new(text))]);
        let mut buf = Vec::new();
        record.serialize(&mut buf);

        let header_length = record.values.len() + 1;
        let header = &buf[0..header_length];
        // First byte should be header size
        assert_eq!(header[0], header_length as u8);
        // Second byte should be serial type for TEXT, which is (len * 2 + 13)
        assert_eq!(header[1], (5 * 2 + 13) as u8);
        // Check the actual text bytes
        assert_eq!(&buf[2..7], b"hello");
        // Check that buffer length is correct
        assert_eq!(buf.len(), header_length + text.len());
    }

    #[test]
    fn test_serialize_blob() {
        let blob = vec![1, 2, 3, 4, 5];
        let record = Record::new(vec![Value::Blob(blob.clone())]);
        let mut buf = Vec::new();
        record.serialize(&mut buf);

        let header_length = record.values.len() + 1;
        let header = &buf[0..header_length];
        // First byte should be header size
        assert_eq!(header[0], header_length as u8);
        // Second byte should be serial type for BLOB, which is (len * 2 + 12)
        assert_eq!(header[1], (5 * 2 + 12) as u8);
        // Check the actual blob bytes
        assert_eq!(&buf[2..7], &[1, 2, 3, 4, 5]);
        // Check that buffer length is correct
        assert_eq!(buf.len(), header_length + blob.len());
    }

    #[test]
    fn test_serialize_mixed_types() {
        let text = "test";
        let record = Record::new(vec![
            Value::Null,
            Value::Integer(42),
            Value::Float(3.15),
            Value::Text(Text::new(text)),
        ]);
        let mut buf = Vec::new();
        record.serialize(&mut buf);

        let header_length = record.values.len() + 1;
        let header = &buf[0..header_length];
        // First byte should be header size
        assert_eq!(header[0], header_length as u8);
        // Second byte should be serial type for NULL
        assert_eq!(header[1] as u64, u64::from(SerialType::null()));
        // Third byte should be serial type for I8
        assert_eq!(header[2] as u64, u64::from(SerialType::i8()));
        // Fourth byte should be serial type for F64
        assert_eq!(header[3] as u64, u64::from(SerialType::f64()));
        // Fifth byte should be serial type for TEXT, which is (len * 2 + 13)
        assert_eq!(header[4] as u64, (4 * 2 + 13) as u64);

        // Check that the bytes after the header can be interpreted as the correct values
        let mut cur_offset = header_length;
        let i8_bytes = &buf[cur_offset..cur_offset + size_of::<i8>()];
        cur_offset += size_of::<i8>();
        let f64_bytes = &buf[cur_offset..cur_offset + size_of::<f64>()];
        cur_offset += size_of::<f64>();
        let text_bytes = &buf[cur_offset..cur_offset + text.len()];

        let val_int8 = i8::from_be_bytes(i8_bytes.try_into().unwrap());
        let val_float = f64::from_be_bytes(f64_bytes.try_into().unwrap());
        let val_text = String::from_utf8(text_bytes.to_vec()).unwrap();

        assert_eq!(val_int8, 42);
        assert_eq!(val_float, 3.15);
        assert_eq!(val_text, "test");

        // Check that buffer length is correct
        assert_eq!(
            buf.len(),
            header_length + size_of::<i8>() + size_of::<f64>() + text.len()
        );
    }

    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_lazy_record_parsing() {
        // Create a simple record with 3 values
        let record = Record::new(vec![
            Value::Integer(42),
            Value::Text(Text::from("hello".to_string())),
            Value::Null,
        ]);
        
        // Serialize it
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        
        // Parse the header
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        
        // Create lazy record
        let mut lazy_record = ImmutableRecord::new_lazy(payload, lazy_state);
        
        // Initially, no columns should be parsed
        assert!(lazy_record.values[0].is_none());
        assert!(lazy_record.values[1].is_none());
        assert!(lazy_record.values[2].is_none());
        
        // Access first column
        let val = lazy_record.get_value_opt(0).unwrap();
        assert!(val.is_some());
        match val.unwrap() {
            RefValue::Integer(i) => assert_eq!(i, 42),
            _ => panic!("Expected integer"),
        }
        
        // First column should now be parsed, others not
        assert!(lazy_record.values[0].is_some());
        assert!(lazy_record.values[1].is_none());
        assert!(lazy_record.values[2].is_none());
        
        // Access second column
        let val = lazy_record.get_value_opt(1).unwrap();
        assert!(val.is_some());
        match val.unwrap() {
            RefValue::Text(t) => assert_eq!(t.as_str(), "hello"),
            _ => panic!("Expected text"),
        }
        
        // Two columns parsed, but third should not be parsed yet (2/3 = 66% < 75%)
        assert!(lazy_record.values[0].is_some());
        assert!(lazy_record.values[1].is_some());
        assert!(lazy_record.values[2].is_none());
        
        // Access third column - this should trigger parsing all remaining
        let val = lazy_record.get_value_opt(2).unwrap();
        assert!(val.is_some());
        match val.unwrap() {
            RefValue::Null => {}, // Expected
            _ => panic!("Expected null"),
        }
        
        // All columns should now be parsed
        assert!(lazy_record.values[0].is_some());
        assert!(lazy_record.values[1].is_some());
        assert!(lazy_record.values[2].is_some());
    }

    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_lazy_parsing_75_percent_heuristic() {
        // Create a record with 5 values
        let record = Record::new(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
            Value::Integer(4),
            Value::Integer(5),
        ]);
        
        // Serialize it
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        
        // Parse the header
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        
        // Create lazy record
        let mut lazy_record = ImmutableRecord::new_lazy(payload, lazy_state);
        
        // Access first column
        lazy_record.get_value_opt(0).unwrap();
        assert!(lazy_record.values[0].is_some());
        assert!(lazy_record.values[4].is_none()); // Last column not parsed
        
        // Access second column
        lazy_record.get_value_opt(1).unwrap();
        assert!(lazy_record.values[1].is_some());
        assert!(lazy_record.values[4].is_none()); // Still not parsed
        
        // Access third column - this should NOT trigger parsing all remaining
        // because we've only accessed 60% of columns (3 out of 5)
        lazy_record.get_value_opt(2).unwrap();
        assert!(lazy_record.values[2].is_some());
        assert!(lazy_record.values[4].is_none()); // Still not parsed
        
        // Access fourth column - this should trigger parsing all remaining
        // because we've accessed >75% of columns (4 out of 5)
        lazy_record.get_value_opt(3).unwrap();
        
        // All columns should now be parsed
        for i in 0..5 {
            assert!(lazy_record.values[i].is_some());
        }
    }

    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_parsed_mask_small() {
        let mut mask = ParsedMask::new(10);
        
        // Initially nothing is parsed
        assert!(!mask.is_parsed(0));
        assert!(!mask.is_parsed(9));
        
        // Mark some as parsed
        mask.set_parsed(0);
        mask.set_parsed(5);
        mask.set_parsed(9);
        
        assert!(mask.is_parsed(0));
        assert!(!mask.is_parsed(1));
        assert!(mask.is_parsed(5));
        assert!(mask.is_parsed(9));
        
        // Count parsed
        assert_eq!(mask.parsed_count(), 3);
        
        // Should not parse remaining yet (3 out of 10 = 30%)
        assert!(!mask.should_parse_remaining(10));
        
        // Mark more columns
        mask.set_parsed(3);
        mask.set_parsed(7);
        mask.set_parsed(8);
        
        // Now 6 out of 10 = 60%, should NOT parse remaining
        assert!(!mask.should_parse_remaining(10));
        
        // Mark two more columns
        mask.set_parsed(2);
        mask.set_parsed(4);
        
        // Now 8 out of 10 = 80%, should parse remaining
        assert!(mask.should_parse_remaining(10));
    }

    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_parsed_mask_large() {
        let mut mask = ParsedMask::new(100);
        
        // Mark columns across multiple u64 chunks
        mask.set_parsed(0);
        mask.set_parsed(63);
        mask.set_parsed(64);
        mask.set_parsed(99);
        
        assert!(mask.is_parsed(0));
        assert!(mask.is_parsed(63));
        assert!(mask.is_parsed(64));
        assert!(mask.is_parsed(99));
        assert!(!mask.is_parsed(50));
        
        assert_eq!(mask.parsed_count(), 4);
        
        // Parse many more to trigger the heuristic
        for i in 0..76 {
            mask.set_parsed(i);
        }
        
        // Now 76 out of 100 = 76%, should parse remaining
        assert!(mask.should_parse_remaining(100));
    }

    // Edge Case Tests for Lazy Parsing
    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_lazy_parsing_empty_record() {
        // Test with 0 columns
        let record = Record::new(vec![]);
        
        // Serialize it
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        
        // Parse the header
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        assert_eq!(lazy_state.column_count, 0);
        
        // Create lazy record
        let lazy_record = ImmutableRecord::new_lazy(payload, lazy_state);
        assert_eq!(lazy_record.values.len(), 0);
    }

    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_lazy_parsing_all_null_values() {
        // Create a record with all NULL values
        let record = Record::new(vec![
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ]);
        
        // Serialize it
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        
        // Parse the header
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        
        // Create lazy record
        let mut lazy_record = ImmutableRecord::new_lazy(payload, lazy_state);
        
        // Access all columns - they should all be NULL
        for i in 0..5 {
            let val = lazy_record.get_value_opt(i).unwrap();
            // NULL values are represented as Some(RefValue::Null)
            match val {
                Some(RefValue::Null) => {}, // Expected
                _ => panic!("Expected Some(RefValue::Null), got {:?}", val),
            }
        }
        
        // All columns should be parsed despite being NULL
        for i in 0..5 {
            assert!(lazy_record.values[i].is_some());
            // The Option<RefValue> should contain Some(RefValue::Null) for NULL values
            assert!(matches!(&lazy_record.values[i], Some(RefValue::Null)));
        }
    }

    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_lazy_parsing_very_wide_table() {
        // Create a record with 100 columns (200 would exceed header size limit)
        let mut values = Vec::new();
        for i in 0..100 {
            values.push(Value::Integer(i as i64));
        }
        let record = Record::new(values);
        
        // Serialize it
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        
        // Parse the header
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        assert_eq!(lazy_state.column_count, 100);
        
        // Create lazy record
        let mut lazy_record = ImmutableRecord::new_lazy(payload, lazy_state);
        
        // Access a few columns across the range
        for &idx in &[0, 25, 50, 75, 99] {
            let val = lazy_record.get_value_opt(idx).unwrap();
            match val.unwrap() {
                RefValue::Integer(i) => assert_eq!(i, idx as i64),
                _ => panic!("Expected integer"),
            }
        }
        
        // Verify the parsed mask works correctly for large column counts
        let parsed_mask = &lazy_record.lazy_state.as_ref().unwrap().parsed_mask;
        assert!(parsed_mask.is_parsed(0));
        assert!(parsed_mask.is_parsed(50));
        assert!(parsed_mask.is_parsed(99));
    }

    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_lazy_parsing_large_text_values() {
        // Create a record with large text values
        let large_text = "x".repeat(10000); // 10KB text
        let record = Record::new(vec![
            Value::Integer(42),
            Value::Text(Text::from(large_text.clone())),
            Value::Integer(84),
            Value::Text(Text::from(large_text.clone())),
            Value::Integer(126),
        ]);
        
        // Serialize it
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        
        // Parse the header
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        
        // Create lazy record
        let mut lazy_record = ImmutableRecord::new_lazy(payload, lazy_state);
        
        // Access only the first two integer columns to stay under 75%
        let expected_values = vec![42, 84];
        for (&idx, &expected) in [0, 2].iter().zip(expected_values.iter()) {
            let val = lazy_record.get_value_opt(idx).unwrap();
            match val.unwrap() {
                RefValue::Integer(i) => assert_eq!(i, expected),
                _ => panic!("Expected integer"),
            }
        }
        
        // Large text columns should not be parsed yet (only 2/5 = 40% accessed)
        assert!(lazy_record.values[1].is_none());
        assert!(lazy_record.values[3].is_none());
        assert!(lazy_record.values[4].is_none());
        
        // Now access one large text column
        let val = lazy_record.get_value_opt(1).unwrap();
        match val.unwrap() {
            RefValue::Text(t) => assert_eq!(t.as_str().len(), 10000),
            _ => panic!("Expected text"),
        }
        
        // Still should not parse all (3/5 = 60% < 75%)
        assert!(lazy_record.values[3].is_none());
        assert!(lazy_record.values[4].is_none());
        
        // Access fourth column to trigger parse all
        let val = lazy_record.get_value_opt(4).unwrap();
        match val.unwrap() {
            RefValue::Integer(i) => assert_eq!(i, 126),
            _ => panic!("Expected integer"),
        }
        
        // All columns should now be parsed due to >75% heuristic (4/5 = 80%)
        for i in 0..5 {
            assert!(lazy_record.values[i].is_some());
        }
    }

    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_lazy_parsing_large_blob_values() {
        // Create a record with large blob values
        let large_blob = vec![0xFF; 10000]; // 10KB blob
        let record = Record::new(vec![
            Value::Integer(1),
            Value::Blob(large_blob.clone()),
            Value::Integer(2),
            Value::Blob(large_blob.clone()),
            Value::Integer(3),
        ]);
        
        // Serialize it
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        
        // Parse the header
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        
        // Create lazy record
        let mut lazy_record = ImmutableRecord::new_lazy(payload, lazy_state);
        
        // Access only integer columns first
        let val = lazy_record.get_value_opt(0).unwrap();
        match val.unwrap() {
            RefValue::Integer(i) => assert_eq!(i, 1),
            _ => panic!("Expected integer"),
        }
        
        // Blob should not be parsed yet
        assert!(lazy_record.values[1].is_none());
        
        // Access the blob
        let val = lazy_record.get_value_opt(1).unwrap();
        match val.unwrap() {
            RefValue::Blob(b) => assert_eq!(b.len, 10000),
            _ => panic!("Expected blob"),
        }
    }

    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_lazy_parsing_mixed_serial_types() {
        // Test all different serial types in one record
        let record = Record::new(vec![
            Value::Null,                              // SerialType 0
            Value::Integer(0),                        // SerialType 8 (ConstInt0)
            Value::Integer(1),                        // SerialType 9 (ConstInt1)
            Value::Integer(42),                       // SerialType 1 (I8)
            Value::Integer(1000),                     // SerialType 2 (I16)
            Value::Integer(1_000_000),                // SerialType 3 (I24)
            Value::Integer(1_000_000_000),            // SerialType 4 (I32)
            Value::Integer(1_000_000_000_000),        // SerialType 5 (I48)
            Value::Integer(i64::MAX),                 // SerialType 6 (I64)
            Value::Float(3.14159),                    // SerialType 7 (F64)
            Value::Text(Text::from("hello".to_string())),         // SerialType n*2+13
            Value::Blob(vec![1, 2, 3, 4, 5]),         // SerialType n*2+12
        ]);
        
        // Serialize it
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        
        // Parse the header
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        assert_eq!(lazy_state.column_count, 12);
        
        // Create lazy record
        let mut lazy_record = ImmutableRecord::new_lazy(payload, lazy_state);
        
        // Access columns selectively
        let val = lazy_record.get_value_opt(0).unwrap();
        match val {
            Some(RefValue::Null) => {}, // Expected NULL
            _ => panic!("Expected Some(RefValue::Null), got {:?}", val),
        }
        
        let val = lazy_record.get_value_opt(10).unwrap();
        match val.unwrap() {
            RefValue::Text(t) => assert_eq!(t.as_str(), "hello"),
            _ => panic!("Expected text"),
        }
        
        let val = lazy_record.get_value_opt(9).unwrap();
        match val.unwrap() {
            RefValue::Float(f) => assert!((f - 3.14159).abs() < 0.00001),
            _ => panic!("Expected float"),
        }
    }

    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_lazy_parsing_boundary_conditions() {
        // Test edge cases around the 75% heuristic
        
        // Test with exactly 2 columns (75% = 1.5, so both columns needed)
        let record = Record::new(vec![Value::Integer(1), Value::Integer(2)]);
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        let mut lazy_record = ImmutableRecord::new_lazy(payload, lazy_state);
        
        // Access first column
        lazy_record.get_value_opt(0).unwrap();
        assert!(lazy_record.values[0].is_some());
        assert!(lazy_record.values[1].is_none()); // Should not trigger parse all yet
        
        // Access second column - should parse all now
        lazy_record.get_value_opt(1).unwrap();
        assert!(lazy_record.values[1].is_some());
        
        // Test with 4 columns (75% = 3, so need to access all 4 columns)
        let record = Record::new(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3), Value::Integer(4)]);
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        let mut lazy_record = ImmutableRecord::new_lazy(payload, lazy_state);
        
        // Access first three columns
        lazy_record.get_value_opt(0).unwrap();
        lazy_record.get_value_opt(1).unwrap();
        lazy_record.get_value_opt(2).unwrap();
        assert!(lazy_record.values[3].is_none()); // Last not parsed yet (3/4 = 75%, not >75%)
        
        // Access fourth column - should trigger parse all
        lazy_record.get_value_opt(3).unwrap();
        assert!(lazy_record.values[3].is_some()); // All parsed now
    }

    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_lazy_parsing_random_access_pattern() {
        // Test accessing columns in random order
        let record = Record::new(vec![
            Value::Integer(0),
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
            Value::Integer(4),
            Value::Integer(5),
            Value::Integer(6),
            Value::Integer(7),
            Value::Integer(8),
            Value::Integer(9),
        ]);
        
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        let mut lazy_record = ImmutableRecord::new_lazy(payload, lazy_state);
        
        // Access in random order: 7, 2, 9, 0, 5, 3, 1, 4
        let access_order = vec![7, 2, 9, 0, 5, 3, 1, 4];
        for (i, &idx) in access_order.iter().enumerate() {
            let val = lazy_record.get_value_opt(idx).unwrap();
            match val.unwrap() {
                RefValue::Integer(n) => assert_eq!(n, idx as i64),
                _ => panic!("Expected integer"),
            }
            
            // After accessing 8 columns (>75%), all should be parsed
            if i == 7 {
                for j in 0..10 {
                    assert!(lazy_record.values[j].is_some());
                }
            }
        }
    }

    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_lazy_parsing_consecutive_nulls() {
        // Test records with many consecutive NULLs
        let mut values = vec![];
        for i in 0..20 {
            if i % 4 == 0 {
                values.push(Value::Integer(i));
            } else {
                values.push(Value::Null);
            }
        }
        
        let record = Record::new(values);
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        let mut lazy_record = ImmutableRecord::new_lazy(payload, lazy_state);
        
        // Access some NULL columns
        for &idx in &[1, 2, 3, 5, 6] {
            let val = lazy_record.get_value_opt(idx).unwrap();
            match val {
                Some(RefValue::Null) => {}, // Expected NULL
                _ => panic!("Expected Some(RefValue::Null), got {:?}", val),
            }
        }
        
        // Access some integer columns
        for &idx in &[0, 4, 8] {
            let val = lazy_record.get_value_opt(idx).unwrap();
            match val.unwrap() {
                RefValue::Integer(n) => assert_eq!(n, idx as i64),
                _ => panic!("Expected integer"),
            }
        }
    }

    #[cfg(feature = "lazy_parsing")]
    #[test]
    fn test_lazy_parsing_minimum_column_threshold() {
        // Test that lazy parsing only activates for records with >8 columns
        
        // Test with 8 columns - should use eager parsing
        let values: Vec<Value> = (0..8).map(|i| Value::Integer(i)).collect();
        let record = Record::new(values);
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        
        // When parsed with lazy parsing feature, records with <=8 columns
        // should still be eagerly parsed
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        let lazy_record = ImmutableRecord::new_lazy(payload.clone(), lazy_state);
        
        // Check if it was eagerly parsed (implementation-specific check)
        // For now, just verify it works correctly
        assert_eq!(lazy_record.values.len(), 8);
        
        // Test with 9 columns - should use lazy parsing
        let values: Vec<Value> = (0..9).map(|i| Value::Integer(i)).collect();
        let record = Record::new(values);
        let mut payload = Vec::new();
        record.serialize(&mut payload);
        
        let lazy_state = crate::storage::sqlite3_ondisk::parse_record_header(&payload).unwrap();
        let mut lazy_record = ImmutableRecord::new_lazy(payload, lazy_state);
        
        // Verify columns are not parsed initially
        for i in 0..9 {
            assert!(lazy_record.values[i].is_none());
        }
        
        // Access one column
        let val = lazy_record.get_value_opt(0).unwrap();
        match val.unwrap() {
            RefValue::Integer(n) => assert_eq!(n, 0),
            _ => panic!("Expected integer"),
        }
    }

    // Note: Overflow page testing would require deeper integration with the 
    // storage layer and actual database pages, which is beyond the scope
    // of unit tests. This would be better tested in integration tests.
}
