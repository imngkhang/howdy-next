//! User-model file decode, encode, and label exports backed by `howdy-user-model`.

use std::ffi::c_char;

use howdy_user_model::{EntryRef, Error, LabelProblem, Metric, Models};

use super::{HowdyStr, borrow, write_out};
use crate::HowdyFfiStatus;
use crate::boundary::guard;

// Limits spelled as literals because cbindgen cannot evaluate a path into another crate; each
// assertion keeps the literal equal to the `howdy_user_model` constant the codec enforces.

/// Largest model file, in bytes, that decoding and encoding accept.
pub const HOWDY_USER_MODEL_MAX_FILE_BYTES: usize = 1_048_576;
const _: () = assert!(HOWDY_USER_MODEL_MAX_FILE_BYTES == howdy_user_model::MAX_FILE_BYTES);
/// Most entries one model file holds.
pub const HOWDY_USER_MODEL_MAX_MODELS: usize = 256;
const _: () = assert!(HOWDY_USER_MODEL_MAX_MODELS == howdy_user_model::MAX_MODELS);
/// Most encodings one entry holds.
pub const HOWDY_USER_MODEL_MAX_ENCODINGS_PER_MODEL: usize = 32;
const _: () =
    assert!(HOWDY_USER_MODEL_MAX_ENCODINGS_PER_MODEL == howdy_user_model::MAX_ENCODINGS_PER_MODEL);
/// Most values one encoding holds.
pub const HOWDY_USER_MODEL_MAX_ENCODING_LEN: usize = 1024;
const _: () = assert!(HOWDY_USER_MODEL_MAX_ENCODING_LEN == howdy_user_model::MAX_ENCODING_LEN);
/// Largest entry id; the stored next id is at most one above it.
pub const HOWDY_USER_MODEL_MAX_ID: u32 = 2_147_483_646;
const _: () = assert!(HOWDY_USER_MODEL_MAX_ID == howdy_user_model::MAX_ID);
/// Longest model label, in bytes.
pub const HOWDY_USER_MODEL_MAX_LABEL_BYTES: usize = 24;
const _: () = assert!(HOWDY_USER_MODEL_MAX_LABEL_BYTES == howdy_user_model::MAX_LABEL_BYTES);

/// Why decoding or encoding failed; mirrors `howdy_user_model::Error`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HowdyUserModelErrorKind {
    TooLarge = 0,
    Truncated = 1,
    BadMagic = 2,
    UnsupportedVersion = 3,
    ChecksumMismatch = 4,
    TrailingBytes = 5,
    NoModels = 6,
    TooManyModels = 7,
    IdOutOfRange = 8,
    IdNotAscending = 9,
    InvalidMetric = 10,
    InvalidLabel = 11,
    InvalidMetadata = 12,
    NoEncodings = 13,
    TooManyEncodings = 14,
    EncodingLength = 15,
    EncodingLengthMismatch = 16,
    NonFiniteValue = 17,
    EncodeMismatch = 18,
    InvalidNextId = 19,
}

impl From<Error> for HowdyUserModelErrorKind {
    fn from(error: Error) -> Self {
        match error {
            Error::TooLarge => Self::TooLarge,
            Error::Truncated => Self::Truncated,
            Error::BadMagic => Self::BadMagic,
            Error::UnsupportedVersion => Self::UnsupportedVersion,
            Error::ChecksumMismatch => Self::ChecksumMismatch,
            Error::TrailingBytes => Self::TrailingBytes,
            Error::NoModels => Self::NoModels,
            Error::TooManyModels => Self::TooManyModels,
            Error::IdOutOfRange => Self::IdOutOfRange,
            Error::IdNotAscending => Self::IdNotAscending,
            Error::InvalidMetric => Self::InvalidMetric,
            Error::InvalidLabel => Self::InvalidLabel,
            Error::InvalidMetadata => Self::InvalidMetadata,
            Error::NoEncodings => Self::NoEncodings,
            Error::TooManyEncodings => Self::TooManyEncodings,
            Error::EncodingLength => Self::EncodingLength,
            Error::EncodingLengthMismatch => Self::EncodingLengthMismatch,
            Error::NonFiniteValue => Self::NonFiniteValue,
            Error::EncodeMismatch => Self::EncodeMismatch,
            Error::InvalidNextId => Self::InvalidNextId,
        }
    }
}

/// Distance metric of an entry; the discriminants are the stored codes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HowdyUserModelMetric {
    Cosine = 1,
    L2 = 2,
    L2Norm = 3,
}

impl From<Metric> for HowdyUserModelMetric {
    fn from(metric: Metric) -> Self {
        match metric {
            Metric::Cosine => Self::Cosine,
            Metric::L2 => Self::L2,
            Metric::L2Norm => Self::L2Norm,
        }
    }
}

/// A decoded entry; every pointer is borrowed from the decode handle. `values` holds
/// `encoding_count * encoding_len` floats, one encoding after another.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct HowdyUserModelEntryView {
    pub id: u32,
    pub time: i64,
    pub metric: HowdyUserModelMetric,
    pub label: HowdyStr,
    pub backend: HowdyStr,
    pub model: HowdyStr,
    pub encoding_count: usize,
    pub encoding_len: usize,
    pub values: *const f32,
}

/// One encoding to encode: `len` floats at `values`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct HowdyUserModelEncoding {
    pub values: *const f32,
    pub len: usize,
}

/// One entry to encode. Read from C memory, so `metric` is a plain integer holding a
/// [`HowdyUserModelMetric`] discriminant rather than the enum (an invalid enum value is UB).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct HowdyUserModelEntryInput {
    pub id: u32,
    pub time: i64,
    pub metric: u32,
    pub label: HowdyStr,
    pub backend: HowdyStr,
    pub model: HowdyStr,
    pub encodings: *const HowdyUserModelEncoding,
    pub encoding_count: usize,
}

/// Opaque result of [`howdy_user_model_decode`].
pub struct HowdyUserModelDecoded {
    result: Result<Models, Error>,
}

/// Opaque result of [`howdy_user_model_encode`].
pub struct HowdyUserModelEncoded {
    result: Result<Vec<u8>, Error>,
}

/// Reports whether `result` failed, writing the error kind when it did.
///
/// # Safety
///
/// `out_has_error` and `out_kind` must be null or valid for one write each.
unsafe fn report_error<T>(
    result: &Result<T, Error>,
    out_has_error: *mut bool,
    out_kind: *mut HowdyUserModelErrorKind,
) -> HowdyFfiStatus {
    if out_has_error.is_null() || out_kind.is_null() {
        return HowdyFfiStatus::InvalidArgument;
    }
    // SAFETY: both pointers are non-null and valid for writes per the caller contract.
    unsafe {
        if let Err(error) = result {
            write_out(out_kind, HowdyUserModelErrorKind::from(*error));
        }
        write_out(out_has_error, result.is_err())
    }
}

/// Decodes `content` into a new handle stored in `out`.
///
/// The handle exists whenever this returns `Ok`, including for a rejected file; read the
/// outcome with [`howdy_user_model_decoded_error`].
///
/// # Safety
///
/// `content` follows the [`HowdyStr`] contract and `out` must be valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_decode(
    content: HowdyStr,
    out: *mut *mut HowdyUserModelDecoded,
) -> HowdyFfiStatus {
    guard(|| {
        if out.is_null() {
            return HowdyFfiStatus::InvalidArgument;
        }
        if content.len > HOWDY_USER_MODEL_MAX_FILE_BYTES {
            return HowdyFfiStatus::TooLarge;
        }
        // SAFETY: forwarded caller contract.
        let Some(bytes) = (unsafe { content.bytes() }) else {
            return HowdyFfiStatus::InvalidArgument;
        };
        let handle = Box::new(HowdyUserModelDecoded {
            result: howdy_user_model::decode(bytes),
        });
        // SAFETY: `out` is non-null and valid for writes per the caller contract.
        unsafe { write_out(out, Box::into_raw(handle)) }
    })
}

/// Writes whether `decoded` holds an error and, if so, its kind.
///
/// # Safety
///
/// `decoded` must be a live handle from [`howdy_user_model_decode`]; the out pointers must be
/// valid for one write each.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_decoded_error(
    decoded: *const HowdyUserModelDecoded,
    out_has_error: *mut bool,
    out_kind: *mut HowdyUserModelErrorKind,
) -> HowdyFfiStatus {
    guard(|| {
        // SAFETY: the caller passes null or a live handle.
        let Some(decoded) = (unsafe { decoded.as_ref() }) else {
            return HowdyFfiStatus::InvalidArgument;
        };
        // SAFETY: forwarded caller contract.
        unsafe { report_error(&decoded.result, out_has_error, out_kind) }
    })
}

/// Writes the number of entries in a successfully decoded `decoded`.
///
/// # Safety
///
/// `decoded` must be a live handle and `out` valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_decoded_count(
    decoded: *const HowdyUserModelDecoded,
    out: *mut usize,
) -> HowdyFfiStatus {
    guard(|| {
        // SAFETY: the caller passes null or a live handle.
        let Some(Ok(models)) = (unsafe { decoded.as_ref() }).map(|handle| &handle.result) else {
            return HowdyFfiStatus::InvalidArgument;
        };
        // SAFETY: the caller guarantees `out` is null or valid for writes.
        unsafe { write_out(out, models.entries.len()) }
    })
}

/// Writes the stored next id of a successfully decoded `decoded`.
///
/// # Safety
///
/// `decoded` must be a live handle and `out` valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_decoded_next_id(
    decoded: *const HowdyUserModelDecoded,
    out: *mut u32,
) -> HowdyFfiStatus {
    guard(|| {
        // SAFETY: the caller passes null or a live handle.
        let Some(Ok(models)) = (unsafe { decoded.as_ref() }).map(|handle| &handle.result) else {
            return HowdyFfiStatus::InvalidArgument;
        };
        // SAFETY: the caller guarantees `out` is null or valid for writes.
        unsafe { write_out(out, models.next_id) }
    })
}

/// Writes entry `index` of a successfully decoded `decoded`.
///
/// # Safety
///
/// `decoded` must be a live handle and `out` valid for one write. Borrowed strings and values
/// stay valid until the handle is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_decoded_entry(
    decoded: *const HowdyUserModelDecoded,
    index: usize,
    out: *mut HowdyUserModelEntryView,
) -> HowdyFfiStatus {
    guard(|| {
        // SAFETY: the caller passes null or a live handle.
        let Some(Ok(models)) = (unsafe { decoded.as_ref() }).map(|handle| &handle.result) else {
            return HowdyFfiStatus::InvalidArgument;
        };
        let Some(entry) = models.entries.get(index) else {
            return HowdyFfiStatus::InvalidArgument;
        };
        let view = HowdyUserModelEntryView {
            id: entry.id,
            time: entry.time,
            metric: entry.metric.into(),
            label: HowdyStr::from_str(&entry.label),
            backend: HowdyStr::from_str(&entry.backend),
            model: HowdyStr::from_str(&entry.model),
            encoding_count: entry.encoding_count(),
            encoding_len: entry.encoding_len,
            values: entry.values.as_ptr(),
        };
        // SAFETY: the caller guarantees `out` is null or valid for writes.
        unsafe { write_out(out, view) }
    })
}

/// Releases a decode handle; null is ignored.
///
/// # Safety
///
/// `decoded` must be null or a handle from [`howdy_user_model_decode`] not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_decoded_free(decoded: *mut HowdyUserModelDecoded) {
    let _ = guard(|| {
        if !decoded.is_null() {
            // SAFETY: the handle came from `Box::into_raw` and is freed exactly once.
            drop(unsafe { Box::from_raw(decoded) });
        }
        HowdyFfiStatus::Ok
    });
}

/// Outcome of borrowing the C entries.
enum Borrowed<'a> {
    /// Every entry was borrowed.
    Entries(Vec<BorrowedEntry<'a>>),
    /// A count broke its limit; reported as the codec error it would cause.
    Rejected(Error),
    /// The caller broke the call contract (null pointer, unknown metric).
    Invalid,
}

/// Text and encodings borrowed from one [`HowdyUserModelEntryInput`].
struct BorrowedEntry<'a> {
    input: HowdyUserModelEntryInput,
    metric: Metric,
    label: &'a [u8],
    backend: &'a [u8],
    model: &'a [u8],
    encodings: Vec<&'a [f32]>,
}

/// Borrows the C entries, checking every count against its limit before a slice is built. A
/// count over its limit becomes the codec error it would cause; null pointers and unknown
/// metrics are caller errors.
///
/// These bounds run for every entry before the codec sees any of them, so when one call breaks
/// several rules the reported error can differ from the codec's field order (an oversized count
/// in a later entry wins over a bad id in an earlier one). Both paths fail closed; never building
/// a slice from an unchecked C length is the boundary rule that takes priority.
///
/// # Safety
///
/// Every pointer in `inputs` follows the [`borrow`] contract for its stated length.
unsafe fn borrow_entries<'a>(inputs: &[HowdyUserModelEntryInput]) -> Borrowed<'a> {
    let mut entries = Vec::with_capacity(inputs.len());
    for input in inputs {
        let Some(metric) = u8::try_from(input.metric).ok().and_then(Metric::from_code) else {
            return Borrowed::Invalid;
        };
        if input.encoding_count > HOWDY_USER_MODEL_MAX_ENCODINGS_PER_MODEL {
            return Borrowed::Rejected(Error::TooManyEncodings);
        }
        // SAFETY: forwarded caller contract.
        let (label, backend, model, encodings) = unsafe {
            (
                input.label.bytes(),
                input.backend.bytes(),
                input.model.bytes(),
                borrow(input.encodings, input.encoding_count),
            )
        };
        let (Some(label), Some(backend), Some(model), Some(encodings)) =
            (label, backend, model, encodings)
        else {
            return Borrowed::Invalid;
        };
        let mut values = Vec::with_capacity(encodings.len());
        for encoding in encodings {
            if encoding.len > HOWDY_USER_MODEL_MAX_ENCODING_LEN {
                return Borrowed::Rejected(Error::EncodingLength);
            }
            // SAFETY: forwarded caller contract.
            let Some(slice) = (unsafe { borrow(encoding.values, encoding.len) }) else {
                return Borrowed::Invalid;
            };
            values.push(slice);
        }
        entries.push(BorrowedEntry {
            input: *input,
            metric,
            label,
            backend,
            model,
            encodings: values,
        });
    }
    Borrowed::Entries(entries)
}

/// Encodes `count` entries at `entries`, with `next_id` stored in the header, into a new handle
/// stored in `out`.
///
/// The handle exists whenever this returns `Ok`, including for rejected entries; read it with
/// [`howdy_user_model_encoded_error`] and [`howdy_user_model_encoded_content`].
///
/// # Safety
///
/// `entries` must be valid for reads of `count` inputs (or `count == 0`), every pointer inside
/// them valid for its stated length, and `out` valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_encode(
    next_id: u32,
    entries: *const HowdyUserModelEntryInput,
    count: usize,
    out: *mut *mut HowdyUserModelEncoded,
) -> HowdyFfiStatus {
    guard(|| {
        if out.is_null() {
            return HowdyFfiStatus::InvalidArgument;
        }
        let result = if count > HOWDY_USER_MODEL_MAX_MODELS {
            Err(Error::TooManyModels)
        } else {
            // SAFETY: forwarded caller contract.
            let Some(inputs) = (unsafe { borrow(entries, count) }) else {
                return HowdyFfiStatus::InvalidArgument;
            };
            // SAFETY: forwarded caller contract.
            match unsafe { borrow_entries(inputs) } {
                Borrowed::Invalid => return HowdyFfiStatus::InvalidArgument,
                Borrowed::Rejected(error) => Err(error),
                Borrowed::Entries(borrowed) => {
                    let refs: Vec<EntryRef<'_>> = borrowed
                        .iter()
                        .map(|entry| EntryRef {
                            id: entry.input.id,
                            time: entry.input.time,
                            metric: entry.metric,
                            label: entry.label,
                            backend: entry.backend,
                            model: entry.model,
                            encodings: &entry.encodings,
                        })
                        .collect();
                    howdy_user_model::encode(next_id, &refs)
                }
            }
        };
        let handle = Box::new(HowdyUserModelEncoded { result });
        // SAFETY: `out` is non-null and valid for writes per the caller contract.
        unsafe { write_out(out, Box::into_raw(handle)) }
    })
}

/// Writes whether `encoded` holds an error and, if so, its kind.
///
/// # Safety
///
/// `encoded` must be a live handle from [`howdy_user_model_encode`]; the out pointers must be
/// valid for one write each.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_encoded_error(
    encoded: *const HowdyUserModelEncoded,
    out_has_error: *mut bool,
    out_kind: *mut HowdyUserModelErrorKind,
) -> HowdyFfiStatus {
    guard(|| {
        // SAFETY: the caller passes null or a live handle.
        let Some(encoded) = (unsafe { encoded.as_ref() }) else {
            return HowdyFfiStatus::InvalidArgument;
        };
        // SAFETY: forwarded caller contract.
        unsafe { report_error(&encoded.result, out_has_error, out_kind) }
    })
}

/// Writes the file bytes of a successful `encoded`.
///
/// # Safety
///
/// `encoded` must be a live handle and `out` valid for one write. The bytes stay valid until
/// the handle is freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_encoded_content(
    encoded: *const HowdyUserModelEncoded,
    out: *mut HowdyStr,
) -> HowdyFfiStatus {
    guard(|| {
        // SAFETY: the caller passes null or a live handle.
        let Some(Ok(bytes)) = (unsafe { encoded.as_ref() }).map(|handle| &handle.result) else {
            return HowdyFfiStatus::InvalidArgument;
        };
        // SAFETY: the caller guarantees `out` is null or valid for writes.
        unsafe { write_out(out, HowdyStr::from_bytes(bytes)) }
    })
}

/// Releases an encode handle; null is ignored.
///
/// # Safety
///
/// `encoded` must be null or a handle from [`howdy_user_model_encode`] not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_encoded_free(encoded: *mut HowdyUserModelEncoded) {
    let _ = guard(|| {
        if !encoded.is_null() {
            // SAFETY: the handle came from `Box::into_raw` and is freed exactly once.
            drop(unsafe { Box::from_raw(encoded) });
        }
        HowdyFfiStatus::Ok
    });
}

/// Why a label is not valid for a new entry; mirrors `howdy_user_model::LabelProblem`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HowdyUserModelLabelProblem {
    None = 0,
    Empty = 1,
    TooLong = 2,
    Malformed = 3,
    Hidden = 4,
}

impl From<Option<LabelProblem>> for HowdyUserModelLabelProblem {
    fn from(problem: Option<LabelProblem>) -> Self {
        match problem {
            None => Self::None,
            Some(LabelProblem::Empty) => Self::Empty,
            Some(LabelProblem::TooLong) => Self::TooLong,
            Some(LabelProblem::Malformed) => Self::Malformed,
            Some(LabelProblem::Hidden) => Self::Hidden,
        }
    }
}

/// Writes what, if anything, makes `label` invalid for a new entry.
///
/// # Safety
///
/// `label` follows the [`HowdyStr`] contract and `out` must be valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_label_problem(
    label: HowdyStr,
    out: *mut HowdyUserModelLabelProblem,
) -> HowdyFfiStatus {
    guard(|| {
        // SAFETY: forwarded caller contract.
        let Some(bytes) = (unsafe { label.bytes() }) else {
            return HowdyFfiStatus::InvalidArgument;
        };
        let problem = howdy_user_model::label_problem(bytes).into();
        // SAFETY: the caller guarantees `out` is null or valid for writes.
        unsafe { write_out(out, problem) }
    })
}

/// Longest text [`howdy_user_model_display_label`] writes.
pub const HOWDY_USER_MODEL_MAX_DISPLAY_LABEL_BYTES: usize = 192;
const _: () =
    assert!(HOWDY_USER_MODEL_MAX_DISPLAY_LABEL_BYTES == howdy_user_model::MAX_DISPLAY_LABEL_BYTES);

/// Writes `label` with invisible format and control characters escaped as `<U+XXXX>` into the
/// `capacity` bytes at `out`, and its length to `out_len`. Labels longer than a stored label
/// or not UTF-8 are refused, so the result always fits
/// [`HOWDY_USER_MODEL_MAX_DISPLAY_LABEL_BYTES`].
///
/// # Safety
///
/// `label` follows the [`HowdyStr`] contract, `out` must be valid for `capacity` byte writes,
/// and `out_len` valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_display_label(
    label: HowdyStr,
    out: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> HowdyFfiStatus {
    guard(|| {
        if label.len > HOWDY_USER_MODEL_MAX_LABEL_BYTES || out.is_null() {
            return HowdyFfiStatus::InvalidArgument;
        }
        // SAFETY: forwarded caller contract.
        let Some(bytes) = (unsafe { label.bytes() }) else {
            return HowdyFfiStatus::InvalidArgument;
        };
        let Ok(text) = std::str::from_utf8(bytes) else {
            return HowdyFfiStatus::InvalidUtf8;
        };
        let shown = howdy_user_model::display_label(text);
        if shown.len() > capacity {
            return HowdyFfiStatus::InvalidArgument;
        }
        // SAFETY: `out` is non-null and valid for `capacity >= shown.len()` byte writes.
        unsafe { std::ptr::copy_nonoverlapping(shown.as_ptr().cast::<c_char>(), out, shown.len()) };
        // SAFETY: the caller guarantees `out_len` is null or valid for writes.
        unsafe { write_out(out_len, shown.len()) }
    })
}

/// Writes the next id to store after adding an entry with id `last_id`.
///
/// # Safety
///
/// `out` must be null or valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_next_id_after(
    last_id: u32,
    out: *mut u32,
) -> HowdyFfiStatus {
    // SAFETY: the caller guarantees `out` is null or valid for one write.
    guard(|| unsafe { write_out(out, howdy_user_model::next_id_after(last_id)) })
}

/// Writes the length of the longest prefix of `text` that fits a label without splitting a
/// UTF-8 sequence.
///
/// # Safety
///
/// `text` follows the [`HowdyStr`] contract and `out_len` must be valid for one write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn howdy_user_model_label_prefix_len(
    text: HowdyStr,
    out_len: *mut usize,
) -> HowdyFfiStatus {
    guard(|| {
        // SAFETY: forwarded caller contract.
        let Some(bytes) = (unsafe { text.bytes() }) else {
            return HowdyFfiStatus::InvalidArgument;
        };
        // SAFETY: the caller guarantees `out_len` is null or valid for writes.
        unsafe { write_out(out_len, howdy_user_model::label_prefix_len(bytes)) }
    })
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;

    fn bytes(value: &[u8]) -> HowdyStr {
        HowdyStr::from_bytes(value)
    }

    fn input(encodings: &[HowdyUserModelEncoding], metric: u32) -> HowdyUserModelEntryInput {
        HowdyUserModelEntryInput {
            id: 3,
            time: 1_700_000_000,
            metric,
            label: bytes(b"desk"),
            backend: bytes(b"opencv_dnn_sface"),
            model: bytes(b"sface.onnx"),
            encodings: encodings.as_ptr(),
            encoding_count: encodings.len(),
        }
    }

    fn encoding(values: &[f32]) -> HowdyUserModelEncoding {
        HowdyUserModelEncoding {
            values: values.as_ptr(),
            len: values.len(),
        }
    }

    /// One above the last input id, or 0 without inputs.
    fn next_id_after(inputs: &[HowdyUserModelEntryInput]) -> u32 {
        inputs.last().map_or(0, |input| input.id + 1)
    }

    /// Encodes `inputs` and returns the handle; panics on a non-`Ok` status.
    unsafe fn encode(inputs: &[HowdyUserModelEntryInput]) -> *mut HowdyUserModelEncoded {
        let mut handle = ptr::null_mut();
        // SAFETY: forwarded test contract.
        let status = unsafe {
            howdy_user_model_encode(
                next_id_after(inputs),
                inputs.as_ptr(),
                inputs.len(),
                &raw mut handle,
            )
        };
        assert_eq!(status, HowdyFfiStatus::Ok);
        handle
    }

    unsafe fn error_of(encoded: *const HowdyUserModelEncoded) -> Option<HowdyUserModelErrorKind> {
        let (mut has_error, mut kind) = (false, HowdyUserModelErrorKind::TooLarge);
        // SAFETY: live handle and valid out pointers.
        let status =
            unsafe { howdy_user_model_encoded_error(encoded, &raw mut has_error, &raw mut kind) };
        assert_eq!(status, HowdyFfiStatus::Ok);
        has_error.then_some(kind)
    }

    #[test]
    fn encodes_and_decodes_through_handles() {
        let (first, second) = ([0.5_f32, -1.0], [2.0_f32, 3.0]);
        let encodings = [encoding(&first), encoding(&second)];
        // SAFETY: inputs point into live locals; handles are freed once.
        unsafe {
            let encoded = encode(&[input(&encodings, 3)]);
            assert_eq!(error_of(encoded), None);
            let mut content = bytes(b"");
            assert_eq!(
                howdy_user_model_encoded_content(encoded, &raw mut content),
                HowdyFfiStatus::Ok
            );

            let mut decoded = ptr::null_mut();
            assert_eq!(
                howdy_user_model_decode(content, &raw mut decoded),
                HowdyFfiStatus::Ok
            );
            let (mut has_error, mut kind) = (true, HowdyUserModelErrorKind::TooLarge);
            assert_eq!(
                howdy_user_model_decoded_error(decoded, &raw mut has_error, &raw mut kind),
                HowdyFfiStatus::Ok
            );
            assert!(!has_error);
            let mut count = 0;
            assert_eq!(
                howdy_user_model_decoded_count(decoded, &raw mut count),
                HowdyFfiStatus::Ok
            );
            assert_eq!(count, 1);
            let mut next_id = 0;
            assert_eq!(
                howdy_user_model_decoded_next_id(decoded, &raw mut next_id),
                HowdyFfiStatus::Ok
            );
            assert_eq!(next_id, 4);
            let mut view = HowdyUserModelEntryView {
                id: 0,
                time: 0,
                metric: HowdyUserModelMetric::Cosine,
                label: HowdyStr::from_str(""),
                backend: HowdyStr::from_str(""),
                model: HowdyStr::from_str(""),
                encoding_count: 0,
                encoding_len: 0,
                values: ptr::null(),
            };
            assert_eq!(
                howdy_user_model_decoded_entry(decoded, 0, &raw mut view),
                HowdyFfiStatus::Ok
            );
            assert_eq!((view.id, view.time), (3, 1_700_000_000));
            assert_eq!(view.metric, HowdyUserModelMetric::L2Norm);
            assert_eq!(view.label.bytes(), Some(&b"desk"[..]));
            assert_eq!((view.encoding_count, view.encoding_len), (2, 2));
            assert_eq!(borrow(view.values, 4), Some(&[0.5_f32, -1.0, 2.0, 3.0][..]));
            assert_eq!(
                howdy_user_model_decoded_entry(decoded, 1, &raw mut view),
                HowdyFfiStatus::InvalidArgument
            );
            howdy_user_model_decoded_free(decoded);
            howdy_user_model_encoded_free(encoded);
        }
    }

    #[test]
    fn decode_errors_live_in_the_handle() {
        // SAFETY: valid input; the handle is freed once.
        unsafe {
            let mut decoded = ptr::null_mut();
            assert_eq!(
                howdy_user_model_decode(bytes(b"HOWDYMDL\x01\x00"), &raw mut decoded),
                HowdyFfiStatus::Ok
            );
            let (mut has_error, mut kind) = (false, HowdyUserModelErrorKind::TooLarge);
            howdy_user_model_decoded_error(decoded, &raw mut has_error, &raw mut kind);
            assert!(has_error);
            assert_eq!(kind, HowdyUserModelErrorKind::Truncated);
            let mut count = 0;
            assert_eq!(
                howdy_user_model_decoded_count(decoded, &raw mut count),
                HowdyFfiStatus::InvalidArgument
            );
            howdy_user_model_decoded_free(decoded);
        }
    }

    #[test]
    fn encode_errors_live_in_the_handle() {
        let values = [f32::NAN];
        let encodings = [encoding(&values)];
        // SAFETY: inputs point into live locals; handles are freed once.
        unsafe {
            let encoded = encode(&[input(&encodings, 1)]);
            assert_eq!(
                error_of(encoded),
                Some(HowdyUserModelErrorKind::NonFiniteValue)
            );
            let mut content = bytes(b"");
            assert_eq!(
                howdy_user_model_encoded_content(encoded, &raw mut content),
                HowdyFfiStatus::InvalidArgument
            );
            howdy_user_model_encoded_free(encoded);

            let encoded = encode(&[]);
            assert_eq!(error_of(encoded), Some(HowdyUserModelErrorKind::NoModels));
            howdy_user_model_encoded_free(encoded);
        }
    }

    #[test]
    fn rejects_invalid_arguments() {
        let values = [1.0_f32];
        let encodings = [encoding(&values)];
        let mut handle = ptr::null_mut();
        // SAFETY: each call passes null or valid pointers as documented.
        unsafe {
            assert_eq!(
                howdy_user_model_decode(bytes(b"x"), ptr::null_mut()),
                HowdyFfiStatus::InvalidArgument
            );
            let dangling = HowdyStr {
                ptr: ptr::null(),
                len: 4,
            };
            assert_eq!(
                howdy_user_model_decode(dangling, &raw mut handle),
                HowdyFfiStatus::InvalidArgument
            );
            for metric in [0, 4, 0x101] {
                let entries = [input(&encodings, metric)];
                let mut encoded = ptr::null_mut();
                assert_eq!(
                    howdy_user_model_encode(4, entries.as_ptr(), 1, &raw mut encoded),
                    HowdyFfiStatus::InvalidArgument,
                    "metric {metric}"
                );
            }
            let mut null_label = input(&encodings, 1);
            null_label.label = dangling;
            let mut encoded = ptr::null_mut();
            assert_eq!(
                howdy_user_model_encode(4, &raw const null_label, 1, &raw mut encoded),
                HowdyFfiStatus::InvalidArgument
            );
            assert_eq!(
                howdy_user_model_encode(4, ptr::null(), 1, &raw mut encoded),
                HowdyFfiStatus::InvalidArgument
            );
            let mut problem = HowdyUserModelLabelProblem::None;
            assert_eq!(
                howdy_user_model_label_problem(dangling, &raw mut problem),
                HowdyFfiStatus::InvalidArgument
            );
            let mut buffer = [0 as c_char; 4];
            let mut len = 0;
            assert_eq!(
                howdy_user_model_display_label(
                    bytes("ab\u{202E}".as_bytes()),
                    buffer.as_mut_ptr(),
                    buffer.len(),
                    &raw mut len
                ),
                HowdyFfiStatus::InvalidArgument,
                "a buffer too small for the escaped label is refused"
            );
            assert_eq!(
                howdy_user_model_label_prefix_len(bytes(b"x"), ptr::null_mut()),
                HowdyFfiStatus::InvalidArgument
            );
        }
    }

    #[test]
    fn oversized_inputs_are_refused_before_reading() {
        // SAFETY: every pointer is null, so success proves nothing was read.
        unsafe {
            let mut handle = ptr::null_mut();
            let huge = HowdyStr {
                ptr: ptr::null(),
                len: HOWDY_USER_MODEL_MAX_FILE_BYTES + 1,
            };
            assert_eq!(
                howdy_user_model_decode(huge, &raw mut handle),
                HowdyFfiStatus::TooLarge
            );

            let mut encoded = ptr::null_mut();
            assert_eq!(
                howdy_user_model_encode(
                    0,
                    ptr::null(),
                    HOWDY_USER_MODEL_MAX_MODELS + 1,
                    &raw mut encoded
                ),
                HowdyFfiStatus::Ok
            );
            assert_eq!(
                error_of(encoded),
                Some(HowdyUserModelErrorKind::TooManyModels)
            );
            howdy_user_model_encoded_free(encoded);

            let mut many = input(&[], 1);
            many.encodings = ptr::null();
            many.encoding_count = HOWDY_USER_MODEL_MAX_ENCODINGS_PER_MODEL + 1;
            let encoded = encode(&[many]);
            assert_eq!(
                error_of(encoded),
                Some(HowdyUserModelErrorKind::TooManyEncodings)
            );
            howdy_user_model_encoded_free(encoded);

            let long = [HowdyUserModelEncoding {
                values: ptr::null(),
                len: HOWDY_USER_MODEL_MAX_ENCODING_LEN + 1,
            }];
            let encoded = encode(&[input(&long, 1)]);
            assert_eq!(
                error_of(encoded),
                Some(HowdyUserModelErrorKind::EncodingLength)
            );
            howdy_user_model_encoded_free(encoded);
        }
    }

    #[test]
    fn label_exports_follow_the_codec() {
        let mut problem = HowdyUserModelLabelProblem::Hidden;
        let mut len = 0;
        let mut next_id = 0;
        // SAFETY: valid inputs and out pointers.
        unsafe {
            howdy_user_model_label_problem(bytes("Café".as_bytes()), &raw mut problem);
            assert_eq!(problem, HowdyUserModelLabelProblem::None);
            howdy_user_model_label_problem(bytes(b"a/b"), &raw mut problem);
            assert_eq!(problem, HowdyUserModelLabelProblem::Malformed);
            howdy_user_model_label_problem(bytes(b""), &raw mut problem);
            assert_eq!(problem, HowdyUserModelLabelProblem::Empty);
            howdy_user_model_label_problem(bytes("a\u{200B}".as_bytes()), &raw mut problem);
            assert_eq!(problem, HowdyUserModelLabelProblem::Hidden);
            let long = "y".repeat(HOWDY_USER_MODEL_MAX_LABEL_BYTES + 1);
            howdy_user_model_label_problem(bytes(long.as_bytes()), &raw mut problem);
            assert_eq!(problem, HowdyUserModelLabelProblem::TooLong);

            let text = format!("{}é", "x".repeat(HOWDY_USER_MODEL_MAX_LABEL_BYTES - 1));
            howdy_user_model_label_prefix_len(bytes(text.as_bytes()), &raw mut len);
            assert_eq!(len, HOWDY_USER_MODEL_MAX_LABEL_BYTES - 1);

            let mut buffer = [0 as c_char; HOWDY_USER_MODEL_MAX_DISPLAY_LABEL_BYTES];
            howdy_user_model_display_label(
                bytes("ab\u{202E}cd".as_bytes()),
                buffer.as_mut_ptr(),
                buffer.len(),
                &raw mut len,
            );
            let shown = HowdyStr {
                ptr: buffer.as_ptr(),
                len,
            };
            assert_eq!(shown.bytes(), Some(&b"ab<U+202E>cd"[..]));

            howdy_user_model_next_id_after(4, &raw mut next_id);
            assert_eq!(next_id, 5);
            howdy_user_model_next_id_after(HOWDY_USER_MODEL_MAX_ID, &raw mut next_id);
            assert_eq!(next_id, HOWDY_USER_MODEL_MAX_ID + 1);
        }
    }
}
