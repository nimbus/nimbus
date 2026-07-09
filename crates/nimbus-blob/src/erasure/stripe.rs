use bytes::Bytes;
use nimbus_core::{Error, Result, StorageErrorKind};

pub(crate) fn drive_for(shard_index: usize, stripe_index: usize, total: usize) -> usize {
    (shard_index + stripe_index) % total
}

pub(crate) fn encode_stripe(
    stripe_bytes: &[u8],
    data_shards: usize,
    parity_shards: usize,
) -> Result<Vec<Bytes>> {
    if stripe_bytes.is_empty() {
        return Err(Error::InvalidInput(
            "empty stripes are represented by an empty manifest, not encoded".to_string(),
        ));
    }
    let shard_len = even_shard_len(stripe_bytes.len(), data_shards);
    let mut shards = vec![vec![0u8; shard_len]; data_shards];
    for (index, byte) in stripe_bytes.iter().enumerate() {
        let shard_index = index / shard_len;
        let offset = index % shard_len;
        shards[shard_index][offset] = *byte;
    }

    let parity =
        reed_solomon_simd::encode(data_shards, parity_shards, shards.iter().map(Vec::as_slice))
            .map_err(coding_error)?;

    let mut out = Vec::with_capacity(data_shards + parity_shards);
    out.extend(shards.into_iter().map(Bytes::from));
    out.extend(parity.into_iter().map(Bytes::from));
    Ok(out)
}

pub(crate) fn reassemble_stripe(shards: &[Bytes], true_len: usize) -> Result<Bytes> {
    let total_len = shards
        .iter()
        .try_fold(0usize, |acc, shard| acc.checked_add(shard.len()))
        .ok_or_else(|| Error::storage(StorageErrorKind::Corruption, "stripe length overflow"))?;
    if true_len > total_len {
        return Err(Error::storage(
            StorageErrorKind::Corruption,
            format!("stripe true length {true_len} exceeds decoded bytes {total_len}"),
        ));
    }

    let mut out = Vec::with_capacity(true_len);
    for shard in shards {
        let remaining = true_len.saturating_sub(out.len());
        if remaining == 0 {
            break;
        }
        let take = remaining.min(shard.len());
        out.extend_from_slice(&shard[..take]);
    }
    Ok(Bytes::from(out))
}

pub(crate) fn decode_stripe(
    data_shards: usize,
    parity_shards: usize,
    present: &[(usize, Bytes)],
) -> Result<Vec<Bytes>> {
    if present.len() < data_shards {
        return Err(Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "erasure stripe has {} healthy shards, need {data_shards}",
                present.len()
            ),
        ));
    }

    let total = data_shards + parity_shards;
    let mut seen = vec![false; total];
    let mut shard_len = None;
    let mut originals: Vec<(usize, Bytes)> = Vec::new();
    let mut recoveries: Vec<(usize, Bytes)> = Vec::new();

    for (index, bytes) in present {
        if *index >= total {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                format!("erasure shard index {index} out of bounds for {total} shards"),
            ));
        }
        if std::mem::replace(&mut seen[*index], true) {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                format!("duplicate erasure shard index {index}"),
            ));
        }
        if bytes.is_empty() || bytes.len() % 2 != 0 {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                format!("erasure shard {index} has invalid length {}", bytes.len()),
            ));
        }
        match shard_len {
            Some(expected) if expected != bytes.len() => {
                return Err(Error::storage(
                    StorageErrorKind::Corruption,
                    format!(
                        "erasure shard {index} length {} differs from {expected}",
                        bytes.len()
                    ),
                ));
            }
            Some(_) => {}
            None => shard_len = Some(bytes.len()),
        }

        if *index < data_shards {
            originals.push((*index, bytes.clone()));
        } else {
            recoveries.push((*index - data_shards, bytes.clone()));
        }
    }

    if originals.len() == data_shards {
        originals.sort_by_key(|(index, _)| *index);
        return Ok(originals.into_iter().map(|(_, bytes)| bytes).collect());
    }
    if originals.len() + recoveries.len() < data_shards {
        return Err(Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "erasure stripe has {} healthy shards, need {data_shards}",
                originals.len() + recoveries.len()
            ),
        ));
    }

    let restored = reed_solomon_simd::decode(
        data_shards,
        parity_shards,
        originals
            .iter()
            .map(|(index, bytes)| (*index, bytes.as_ref())),
        recoveries
            .iter()
            .map(|(index, bytes)| (*index, bytes.as_ref())),
    )
    .map_err(coding_error)?;

    let mut decoded = vec![None; data_shards];
    for (index, bytes) in originals {
        decoded[index] = Some(bytes);
    }
    for (index, bytes) in restored {
        if index < data_shards {
            decoded[index] = Some(Bytes::from(bytes));
        }
    }

    decoded
        .into_iter()
        .enumerate()
        .map(|(index, shard)| {
            shard.ok_or_else(|| {
                Error::storage(
                    StorageErrorKind::Corruption,
                    format!("erasure decoder did not restore data shard {index}"),
                )
            })
        })
        .collect()
}

fn even_shard_len(stripe_len: usize, data_shards: usize) -> usize {
    let ceil = stripe_len.div_ceil(data_shards);
    if ceil % 2 == 0 { ceil } else { ceil + 1 }
}

fn coding_error(error: reed_solomon_simd::Error) -> Error {
    Error::storage(
        StorageErrorKind::Corruption,
        format!("erasure coding failed: {error}"),
    )
}
