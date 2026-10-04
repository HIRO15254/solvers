//! Owned postcard decoding over individually verified checkpoint chunks.
//!
//! Only the current compressed/decoded chunk and the largest field crossing a
//! chunk boundary are staged. The returned solver state still needs memory.

use serde::de::DeserializeOwned;

use super::*;

pub(super) struct PayloadReader<'a> {
    file: &'a mut File,
    chunks: &'a [ChunkEntry],
    header: &'a MultiwayCheckpointHeader,
    next_chunk: usize,
    compressed: Vec<u8>,
    decoded: Vec<u8>,
    cursor: usize,
    consumed: u64,
    scratch: Vec<u8>,
    hasher: blake3::Hasher,
    failure: Option<CheckpointError>,
}

impl<'a> PayloadReader<'a> {
    pub(super) fn new(
        file: &'a mut File,
        chunks: &'a [ChunkEntry],
        header: &'a MultiwayCheckpointHeader,
    ) -> Self {
        Self {
            file,
            chunks,
            header,
            next_chunk: 0,
            compressed: Vec::new(),
            decoded: Vec::new(),
            cursor: 0,
            consumed: 0,
            scratch: Vec::new(),
            hasher: blake3::Hasher::new(),
            failure: None,
        }
    }

    fn next(&mut self) -> Result<(), CheckpointError> {
        let index = self.next_chunk;
        let chunk = &self.chunks[index];
        let compressed_len = chunk.compressed_len as usize;
        self.compressed.clear();
        try_reserve_u8(&mut self.compressed, compressed_len)?;
        self.compressed.resize(compressed_len, 0);
        self.file.read_exact(&mut self.compressed)?;
        self.hasher.update(&self.compressed);
        if *blake3::hash(&self.compressed).as_bytes() != chunk.checksum {
            return Err(CheckpointError::ChunkChecksumMismatch {
                index: index as u32,
            });
        }
        self.decoded.clear();
        // One extra byte detects decompression beyond the declared bound.
        let read_limit = chunk.uncompressed_len + 1;
        try_reserve_u8(&mut self.decoded, read_limit as usize)?;
        zstd::stream::read::Decoder::new(self.compressed.as_slice())?
            .take(read_limit)
            .read_to_end(&mut self.decoded)?;
        if self.decoded.len() as u64 != chunk.uncompressed_len {
            return Err(CheckpointError::ChunkDecodedLengthMismatch {
                index: index as u32,
                declared: chunk.uncompressed_len,
                actual: self.decoded.len() as u64,
            });
        }
        self.cursor = 0;
        self.next_chunk += 1;
        Ok(())
    }

    fn ensure_available(&mut self) -> postcard::Result<()> {
        if self.failure.is_some() {
            return Err(postcard::Error::DeserializeUnexpectedEnd);
        }
        if self.cursor == self.decoded.len() {
            if self.next_chunk == self.chunks.len() {
                return Err(postcard::Error::DeserializeUnexpectedEnd);
            }
            if let Err(error) = self.next() {
                self.failure = Some(error);
                return Err(postcard::Error::DeserializeUnexpectedEnd);
            }
        }
        Ok(())
    }

    /// Always finish integrity checks, including when postcard stops early.
    /// As with the old slice decoder, trailing *decoded* bytes are ignored,
    /// but their framing, checksums, and decompressed lengths are validated.
    pub(super) fn finish(mut self) -> Result<(), CheckpointError> {
        if let Some(error) = self.failure.take() {
            return Err(error);
        }
        while self.next_chunk < self.chunks.len() {
            self.next()?;
        }
        if *self.hasher.finalize().as_bytes() != self.header.payload_checksum {
            return Err(CheckpointError::ChecksumMismatch);
        }
        Ok(())
    }
}

// All checkpoint DTOs own their strings/vectors. A temporary borrow suffices
// for postcard's owned string/byte/floating-point visitors; borrowed visitors
// are deliberately unsupported so this adapter needs no unsafe lifetimes.
struct OwnedPayload<'a, 'b>(&'a mut PayloadReader<'b>);

impl<'de, 'a: 'de, 'b: 'de> postcard::de_flavors::Flavor<'de> for OwnedPayload<'a, 'b> {
    type Remainder = ();
    type Source = ();

    #[inline]
    fn pop(&mut self) -> postcard::Result<u8> {
        self.0.ensure_available()?;
        let value = self.0.decoded[self.0.cursor];
        self.0.cursor += 1;
        self.0.consumed += 1;
        Ok(value)
    }

    fn try_take_n(&mut self, _count: usize) -> postcard::Result<&'de [u8]> {
        Err(postcard::Error::DeserializeBadEncoding)
    }

    fn size_hint(&self) -> Option<usize> {
        usize::try_from(self.0.header.uncompressed_len - self.0.consumed).ok()
    }

    #[inline]
    fn try_take_n_temp<'s>(&'s mut self, count: usize) -> postcard::Result<&'s [u8]>
    where
        'de: 's,
    {
        if count == 0 {
            return Ok(&[]);
        }
        // Reject impossible field lengths before any scratch reservation.
        if count as u64 > self.0.header.uncompressed_len - self.0.consumed {
            return Err(postcard::Error::DeserializeUnexpectedEnd);
        }
        self.0.ensure_available()?;
        if count <= self.0.decoded.len() - self.0.cursor {
            let start = self.0.cursor;
            self.0.cursor += count;
            self.0.consumed += count as u64;
            return Ok(&self.0.decoded[start..start + count]);
        }
        self.0.scratch.clear();
        if let Err(error) = try_reserve_u8(&mut self.0.scratch, count) {
            self.0.failure = Some(error);
            return Err(postcard::Error::DeserializeUnexpectedEnd);
        }
        while self.0.scratch.len() < count {
            self.0.ensure_available()?;
            let take = (count - self.0.scratch.len()).min(self.0.decoded.len() - self.0.cursor);
            self.0
                .scratch
                .extend_from_slice(&self.0.decoded[self.0.cursor..self.0.cursor + take]);
            self.0.cursor += take;
            self.0.consumed += take as u64;
        }
        Ok(&self.0.scratch)
    }

    fn finalize(self) -> postcard::Result<()> {
        Ok(())
    }
}

pub(super) fn decode_owned<T: DeserializeOwned>(
    reader: &mut PayloadReader<'_>,
) -> Result<T, CheckpointError> {
    let mut decoder = postcard::Deserializer::from_flavor(OwnedPayload(reader));
    Ok(T::deserialize(&mut decoder)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding_many_small_fields_does_not_retain_the_whole_payload() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bounded.mwckpt");
        let expected = vec![0.125f32; CHUNK_UNCOMPRESSED_BYTES];
        let raw = postcard::to_allocvec(&expected).unwrap();
        super::super::tests::write_checkpoint_with_version(&raw, 6, [0; 32], [0; 32], 0, &path);
        let mut file = File::open(&path).unwrap();
        let mut encoded = [0; HEADER_LEN];
        file.read_exact(&mut encoded).unwrap();
        let header = decode_header(&encoded).unwrap();
        let mut table = vec![0; header.chunk_count as usize * CHUNK_ENTRY_LEN];
        file.read_exact(&mut table).unwrap();
        let chunks = decode_chunk_table(&table);
        let mut reader = PayloadReader::new(&mut file, &chunks, &header);
        let actual: Vec<f32> = decode_owned(&mut reader).unwrap();
        assert_eq!(actual, expected);
        assert!(reader.decoded.capacity() <= CHUNK_UNCOMPRESSED_BYTES + 1);
        assert!(
            reader.compressed.capacity()
                <= zstd::zstd_safe::compress_bound(CHUNK_UNCOMPRESSED_BYTES)
        );
        assert!(reader.scratch.capacity() <= size_of::<f32>());
        assert_eq!(reader.consumed, header.uncompressed_len);
        reader.finish().unwrap();
    }
}
