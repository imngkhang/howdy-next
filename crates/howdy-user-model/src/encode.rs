//! Encoding entries into model file bytes.

use crate::checksum;
use crate::decode::{EntryView, parse};
use crate::entry::{
    check_count, check_id, check_metadata, check_next_id, check_shape, check_stored_label,
    check_value,
};
use crate::{
    CHECKSUM_LEN, Entry, Error, FORMAT_VERSION, HEADER_LEN, MAGIC, MAX_FILE_BYTES, Metric,
};

/// One entry to encode, borrowing its text and encodings from the caller.
#[derive(Clone, Copy, Debug)]
pub struct EntryRef<'a> {
    pub id: u32,
    pub time: i64,
    pub metric: Metric,
    pub label: &'a [u8],
    pub backend: &'a [u8],
    pub model: &'a [u8],
    pub encodings: &'a [&'a [f32]],
}

impl Entry {
    /// Borrows this entry for [`encode`]; `encodings` must come from [`Entry::encodings`].
    #[must_use]
    pub fn as_ref<'a>(&'a self, encodings: &'a [&'a [f32]]) -> EntryRef<'a> {
        EntryRef {
            id: self.id,
            time: self.time,
            metric: self.metric,
            label: self.label.as_bytes(),
            backend: self.backend.as_bytes(),
            model: self.model.as_bytes(),
            encodings,
        }
    }
}

/// Encodes entries into a model file.
///
/// The entries are checked with the same rules, in the same order, that [`decode`] applies, and
/// the result is decoded again and compared with the input before it is returned.
///
/// # Errors
///
/// Returns the first rule the entries break, [`Error::TooLarge`] when the file would exceed
/// [`MAX_FILE_BYTES`], or [`Error::EncodeMismatch`] if the self-check fails.
pub fn encode(next_id: u32, entries: &[EntryRef<'_>]) -> Result<Vec<u8>, Error> {
    check_count(entries.len())?;
    let count = u16::try_from(entries.len()).map_err(|_| Error::TooManyModels)?;
    let mut out = Vec::with_capacity(encoded_len(entries).min(MAX_FILE_BYTES));
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&next_id.to_le_bytes());
    let mut previous = None;
    for entry in entries {
        write_entry(&mut out, entry, previous)?;
        previous = Some(entry.id);
        // Checked per entry, so the buffer never grows far past the limit.
        if out.len() + CHECKSUM_LEN > MAX_FILE_BYTES {
            return Err(Error::TooLarge);
        }
    }
    check_next_id(next_id, previous.ok_or(Error::NoModels)?)?;
    out.extend_from_slice(&checksum(&out).to_le_bytes());

    // Self-check: read the bytes back with the decoder's parser and compare in place.
    let mut index = 0;
    let stored_next_id = parse(&out, |view| {
        let matches = entries.get(index).is_some_and(|want| same(&view, want));
        index += 1;
        if matches {
            Ok(())
        } else {
            Err(Error::EncodeMismatch)
        }
    })
    .map_err(|_| Error::EncodeMismatch)?;
    if stored_next_id != next_id || index != entries.len() {
        return Err(Error::EncodeMismatch);
    }
    Ok(out)
}

fn write_entry(
    out: &mut Vec<u8>,
    entry: &EntryRef<'_>,
    previous: Option<u32>,
) -> Result<(), Error> {
    check_id(entry.id, previous)?;
    let encoding_len = entry.encodings.first().map_or(0, |encoding| encoding.len());
    check_shape(entry.encodings.len(), encoding_len)?;
    if entry
        .encodings
        .iter()
        .any(|encoding| encoding.len() != encoding_len)
    {
        return Err(Error::EncodingLengthMismatch);
    }
    let label = check_stored_label(entry.label)?;
    let backend = check_metadata(entry.backend)?;
    let model = check_metadata(entry.model)?;

    out.extend_from_slice(&entry.id.to_le_bytes());
    out.extend_from_slice(&entry.time.to_le_bytes());
    out.push(entry.metric.code());
    // The shape and text checks above bound every length to its field width.
    out.push(u8::try_from(entry.encodings.len()).map_err(|_| Error::TooManyEncodings)?);
    out.extend_from_slice(
        &u16::try_from(encoding_len)
            .map_err(|_| Error::EncodingLength)?
            .to_le_bytes(),
    );
    push_text(out, label, Error::InvalidLabel)?;
    push_text(out, backend, Error::InvalidMetadata)?;
    push_text(out, model, Error::InvalidMetadata)?;
    for encoding in entry.encodings {
        for &value in *encoding {
            check_value(value)?;
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

/// The next id an append-only writer stores for `entries`, or 0 for an empty list.
#[cfg(any(test, feature = "fuzzing"))]
pub(crate) fn next_id_for(entries: &[EntryRef<'_>]) -> u32 {
    entries
        .last()
        .map_or(0, |entry| crate::next_id_after(entry.id))
}

/// Bytes `entries` encode to, saturating; only used to pre-size the output buffer.
fn encoded_len(entries: &[EntryRef<'_>]) -> usize {
    const ENTRY_FIXED: usize = 4 + 8 + 1 + 1 + 2 + 3;
    entries
        .iter()
        .fold(HEADER_LEN + CHECKSUM_LEN, |total, entry| {
            let values: usize = entry.encodings.iter().map(|encoding| encoding.len()).sum();
            total
                .saturating_add(
                    ENTRY_FIXED + entry.label.len() + entry.backend.len() + entry.model.len(),
                )
                .saturating_add(values.saturating_mul(size_of::<f32>()))
        })
}

/// Writes `text` with its one-byte length prefix.
fn push_text(out: &mut Vec<u8>, text: &str, too_long: Error) -> Result<(), Error> {
    out.push(u8::try_from(text.len()).map_err(|_| too_long)?);
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

/// Whether an entry read back carries exactly the input's fields, comparing values bit for bit.
fn same(got: &EntryView<'_>, want: &EntryRef<'_>) -> bool {
    got.id == want.id
        && got.time == want.time
        && got.metric == want.metric
        && got.label.as_bytes() == want.label
        && got.backend.as_bytes() == want.backend
        && got.model.as_bytes() == want.model
        && got.encoding_count() == want.encodings.len()
        && Some(got.encoding_len) == want.encodings.first().map(|encoding| encoding.len())
        && got
            .values
            .iter()
            .zip(want.encodings.iter().flat_map(|encoding| encoding.iter()))
            .all(|(&raw, value)| u32::from_le_bytes(raw) == value.to_bits())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes with the next id an append-only writer would store.
    fn enc(entries: &[EntryRef<'_>]) -> Result<Vec<u8>, Error> {
        crate::encode(crate::encode::next_id_for(entries), entries)
    }
    use crate::{MAX_ENCODING_LEN, MAX_ENCODINGS_PER_MODEL, MAX_ID, MAX_MODELS};

    fn entry<'a>(id: u32, encodings: &'a [&'a [f32]]) -> EntryRef<'a> {
        EntryRef {
            id,
            time: 0,
            metric: Metric::L2,
            label: b"label",
            backend: b"opencv_dnn_sface",
            model: b"sface.onnx",
            encodings,
        }
    }

    #[test]
    fn rejects_invalid_lists() {
        assert_eq!(enc(&[]), Err(Error::NoModels));
        let many = vec![entry(0, &[&[1.0]]); MAX_MODELS + 1];
        assert_eq!(enc(&many), Err(Error::TooManyModels));
        assert_eq!(
            enc(&[entry(1, &[&[1.0]]), entry(1, &[&[1.0]])]),
            Err(Error::IdNotAscending)
        );
        assert_eq!(
            enc(&[entry(MAX_ID + 1, &[&[1.0]])]),
            Err(Error::IdOutOfRange)
        );
    }

    #[test]
    fn rejects_invalid_encodings() {
        assert_eq!(enc(&[entry(0, &[])]), Err(Error::NoEncodings));
        let many: Vec<&[f32]> = vec![&[1.0]; MAX_ENCODINGS_PER_MODEL + 1];
        assert_eq!(enc(&[entry(0, &many)]), Err(Error::TooManyEncodings));
        assert_eq!(enc(&[entry(0, &[&[]])]), Err(Error::EncodingLength));
        let long = vec![0.0; MAX_ENCODING_LEN + 1];
        assert_eq!(enc(&[entry(0, &[&long])]), Err(Error::EncodingLength));
        assert_eq!(
            enc(&[entry(0, &[&[1.0], &[1.0, 2.0]])]),
            Err(Error::EncodingLengthMismatch)
        );
        assert_eq!(enc(&[entry(0, &[&[f32::NAN]])]), Err(Error::NonFiniteValue));
        assert_eq!(
            enc(&[entry(0, &[&[1.0, f32::INFINITY]])]),
            Err(Error::NonFiniteValue)
        );
    }

    #[test]
    fn rejects_invalid_text() {
        let mut bad = entry(0, &[&[1.0]]);
        bad.label = b"";
        assert_eq!(enc(&[bad]), Err(Error::InvalidLabel));
        bad.label = b"a\\b";
        assert_eq!(enc(&[bad]), Err(Error::InvalidLabel));
        bad.label = b"ok";
        bad.backend = b"";
        assert_eq!(enc(&[bad]), Err(Error::InvalidMetadata));
        bad.backend = b"opencv";
        bad.model = b"has space";
        assert_eq!(enc(&[bad]), Err(Error::InvalidMetadata));
    }

    #[test]
    fn rejects_files_over_the_size_limit() {
        let encoding = vec![0.5; MAX_ENCODING_LEN];
        let encodings: Vec<&[f32]> = vec![&encoding; MAX_ENCODINGS_PER_MODEL];
        let entries: Vec<EntryRef<'_>> = (0..9).map(|id| entry(id, &encodings)).collect();
        assert_eq!(
            enc(&entries[..7]).map(|bytes| bytes.len() <= MAX_FILE_BYTES),
            Ok(true)
        );
        assert_eq!(enc(&entries), Err(Error::TooLarge));
    }
}
