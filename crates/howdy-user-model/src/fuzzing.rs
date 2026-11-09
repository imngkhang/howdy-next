//! Properties shared by the `cargo fuzz` targets and the corpus replay test.

use crate::encode::next_id_for;
use crate::{
    CHECKSUM_LEN, COUNT_OFFSET, Entry, EntryRef, HEADER_LEN, Metric, checksum, decode, encode,
};

/// Decoding arbitrary bytes never panics, and every accepted file re-encodes to the same bytes.
///
/// The input is also decoded with its trailer replaced by a valid checksum, so mutations reach
/// the entry parser instead of stopping at the checksum.
///
/// # Panics
///
/// Panics when an accepted file does not re-encode canonically.
pub fn decode_bytes(data: &[u8]) {
    decode_canonically(data);
    if let Some(resealed) = reseal(data) {
        decode_canonically(&resealed);
    }
}

fn decode_canonically(data: &[u8]) {
    let Ok(models) = decode(data) else {
        return;
    };
    let encoded =
        encode_entries(models.next_id, &models.entries).expect("decoded entries re-encode");
    assert_eq!(encoded, data, "encoding is canonical");
}

/// `data` with its last four bytes replaced by the checksum of the rest, or `None` when it is too
/// short to hold a header and trailer.
fn reseal(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < HEADER_LEN + CHECKSUM_LEN {
        return None;
    }
    let mut sealed = data.to_vec();
    let end = sealed.len() - CHECKSUM_LEN;
    let sum = checksum(&sealed[..end]);
    sealed[end..].copy_from_slice(&sum.to_le_bytes());
    Some(sealed)
}

/// Entries built from arbitrary bytes either fail to encode or decode back unchanged.
///
/// # Panics
///
/// Panics when encoded entries do not decode back to themselves.
pub fn round_trip(data: &[u8]) {
    let mut input = Input(data);
    let count = 1 + usize::from(input.byte() % 4);
    let mut owned = Vec::with_capacity(count);
    let mut id = 0_u32;
    for _ in 0..count {
        id = id.saturating_add(u32::from(input.byte() % 3));
        let metric = Metric::from_code(input.byte() % 4).unwrap_or(Metric::Cosine);
        let encoding_count = 1 + usize::from(input.byte() % 3);
        let encoding_len = 1 + usize::from(input.byte() % 4);
        let label_len = usize::from(input.byte() % 8);
        let label = input.bytes(label_len);
        let encodings: Vec<Vec<f32>> = (0..encoding_count)
            .map(|_| (0..encoding_len).map(|_| input.float()).collect())
            .collect();
        owned.push((id, i64::from(input.byte()), metric, label, encodings));
    }
    let slices: Vec<Vec<&[f32]>> = owned
        .iter()
        .map(|(.., encodings)| encodings.iter().map(Vec::as_slice).collect())
        .collect();
    let refs: Vec<EntryRef<'_>> = owned
        .iter()
        .zip(&slices)
        .map(|((id, time, metric, label, _), encodings)| EntryRef {
            id: *id,
            time: *time,
            metric: *metric,
            label,
            backend: b"opencv_dnn_sface",
            model: b"sface.onnx",
            encodings,
        })
        .collect();
    // Stored next ids may run ahead of the last id after removals.
    let next_id = next_id_for(&refs).saturating_add(u32::from(input.byte() % 3));
    let encoded = match encode(next_id, &refs) {
        Ok(encoded) => encoded,
        Err(crate::Error::EncodeMismatch) => panic!("encoder self-check failed"),
        Err(_) => return,
    };
    let decoded = decode(&encoded).expect("encoded entries decode");
    assert_eq!(decoded.entries.len(), refs.len(), "entry count round-trips");
    assert_eq!(decoded.next_id, next_id, "next id round-trips");
    assert_eq!(
        encode_entries(decoded.next_id, &decoded.entries).expect("decoded entries re-encode"),
        encoded,
        "round trip is canonical"
    );
}

/// Seed files for `fuzz/corpus/user_model`, by file name.
///
/// # Panics
///
/// Panics if a seed entry fails to encode, which would be a codec defect.
#[must_use]
pub fn seed_corpus() -> Vec<(&'static str, Vec<u8>)> {
    let entry = |id, label: &'static [u8], encodings: &'static [&'static [f32]]| EntryRef {
        id,
        time: 1_700_000_000,
        metric: Metric::Cosine,
        label,
        backend: b"opencv_dnn_sface",
        model: b"face_recognition_sface_2021dec.onnx",
        encodings,
    };
    let one = encode(1, &[entry(0, b"Model #0", &[&[0.25, -0.5, 1.0]])]).expect("seed encodes");
    let several = encode(
        7,
        &[
            entry(0, b"desk", &[&[0.1, 0.2], &[0.3, 0.4]]),
            entry(3, "Café 🙂".as_bytes(), &[&[-1.0, 2.0]]),
            entry(4, b"desk", &[&[0.0, -0.0]]),
        ],
    )
    .expect("seed encodes");
    let mut bad_checksum = one.clone();
    let last = bad_checksum.len() - 1;
    bad_checksum[last] ^= 0xFF;
    // Every other corrupt seed carries a valid checksum, so it reaches the rule it is named for.
    let content = &one[..one.len() - CHECKSUM_LEN];
    let mut truncated = content[..content.len() / 2].to_vec();
    truncated.extend_from_slice(&[0; CHECKSUM_LEN]);
    let mut trailing = content.to_vec();
    trailing.extend_from_slice(&[0; 1 + CHECKSUM_LEN]);
    let mut huge_count = one.clone();
    huge_count[COUNT_OFFSET..COUNT_OFFSET + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    let mut bad_next_id = one.clone();
    bad_next_id[COUNT_OFFSET + 2..HEADER_LEN].copy_from_slice(&0_u32.to_le_bytes());
    let mut bad_metric = one.clone();
    bad_metric[HEADER_LEN + 4 + 8] = 0;
    let sealed = |bytes: Vec<u8>| reseal(&bytes).expect("seed holds a header and trailer");
    vec![
        ("empty", Vec::new()),
        ("one_entry", one.clone()),
        ("several_entries", several),
        ("truncated", sealed(truncated)),
        ("bad_checksum", bad_checksum),
        ("trailing_byte", sealed(trailing)),
        ("huge_count", sealed(huge_count)),
        ("bad_metric", sealed(bad_metric)),
        ("bad_next_id", sealed(bad_next_id)),
    ]
}

fn encode_entries(next_id: u32, entries: &[Entry]) -> Result<Vec<u8>, crate::Error> {
    let slices: Vec<Vec<&[f32]>> = entries.iter().map(Entry::encodings).collect();
    let refs: Vec<EntryRef<'_>> = entries
        .iter()
        .zip(&slices)
        .map(|(entry, encodings)| entry.as_ref(encodings))
        .collect();
    encode(next_id, &refs)
}

/// Reads fuzz input front to back, yielding zeros once it runs out.
struct Input<'a>(&'a [u8]);

impl Input<'_> {
    fn byte(&mut self) -> u8 {
        let Some((&first, rest)) = self.0.split_first() else {
            return 0;
        };
        self.0 = rest;
        first
    }

    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.byte()).collect()
    }

    fn float(&mut self) -> f32 {
        f32::from_le_bytes([self.byte(), self.byte(), self.byte(), self.byte()])
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    fn corpus() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/user_model")
    }

    /// Replays the `cargo fuzz` seed corpus on stable, so `ctest` covers it without nightly.
    #[test]
    fn corpus_satisfies_fuzz_properties() {
        let mut replayed = 0;
        for entry in fs::read_dir(corpus()).expect("corpus directory exists") {
            let data = fs::read(entry.expect("corpus entry").path()).expect("corpus file");
            super::decode_bytes(&data);
            super::round_trip(&data);
            replayed += 1;
        }
        assert!(replayed > 0, "corpus is not empty");
    }

    /// The committed seeds match [`super::seed_corpus`], so a layout change cannot leave stale
    /// seeds behind.
    #[test]
    fn corpus_seeds_are_current() {
        for (name, bytes) in super::seed_corpus() {
            let stored = fs::read(corpus().join(name)).expect("seed file exists");
            assert_eq!(
                stored, bytes,
                "seed {name} is current; run write_seed_corpus"
            );
        }
    }

    /// Each corrupt seed fails on the rule it is named for, not on its checksum.
    #[test]
    fn corrupt_seeds_reach_their_rule() {
        use crate::Error;
        let expected = [
            ("empty", Error::Truncated),
            ("truncated", Error::Truncated),
            ("bad_checksum", Error::ChecksumMismatch),
            ("trailing_byte", Error::TrailingBytes),
            ("huge_count", Error::TooManyModels),
            ("bad_metric", Error::InvalidMetric),
            ("bad_next_id", Error::InvalidNextId),
        ];
        let seeds = super::seed_corpus();
        for (name, error) in expected {
            let (_, bytes) = seeds
                .iter()
                .find(|(seed, _)| *seed == name)
                .expect("seed exists");
            assert_eq!(crate::decode(bytes), Err(error), "seed {name}");
        }
    }

    /// Regenerates the committed seeds: `cargo test -p howdy-user-model -- --ignored`.
    #[test]
    #[ignore = "writes fuzz/corpus/user_model"]
    fn write_seed_corpus() {
        fs::create_dir_all(corpus()).expect("corpus directory");
        for (name, bytes) in super::seed_corpus() {
            fs::write(corpus().join(name), bytes).expect("seed written");
        }
    }
}
