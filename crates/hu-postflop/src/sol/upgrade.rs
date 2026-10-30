use super::*;
use hu_engine::NodeKind;

/// Upgrade an indexed legacy P1 artifact without rewriting its strategy or value chunks.
pub fn upgrade_sol(input: &Path, output: &Path) -> Result<(), SolError> {
    if output.try_exists()? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "output already exists",
        )
        .into());
    }
    let mut source = File::open(input)?;
    let file_len = source.metadata()?.len();
    if file_len < 10 {
        return Err(SolError::Truncated {
            expected: HEADER_LEN,
            actual: file_len as usize,
        });
    }
    let mut identity = [0u8; 10];
    source.read_exact(&mut identity)?;
    if &identity[..8] != MAGIC {
        return Err(SolError::BadMagic);
    }
    let version = u16::from_le_bytes([identity[8], identity[9]]);
    match version {
        4..=6 => {}
        7..=9 => return Err(SolError::AlreadyCurrent),
        _ => {
            return Err(SolError::BadVersion {
                found: version,
                expected: FORMAT_VERSION,
            });
        }
    }
    if file_len < HEADER_LEN as u64 {
        return Err(SolError::Truncated {
            expected: HEADER_LEN,
            actual: file_len as usize,
        });
    }
    source.seek(SeekFrom::Start(0))?;
    let mut bytes = [0u8; HEADER_LEN];
    source.read_exact(&mut bytes)?;
    let mut header = parse_header(&bytes);
    let old_index_offset = (HEADER_LEN as u64)
        .checked_add(header.metadata_len)
        .ok_or(SolError::Malformed("invalid frame offsets"))?;
    let old_chunks = old_index_offset
        .checked_add(header.index_len)
        .filter(|&n| n <= header.directory_offset)
        .ok_or(SolError::Malformed("invalid frame offsets"))?;
    if header.directory_offset.checked_add(header.directory_len) != Some(file_len) {
        return Err(SolError::Malformed(
            "directory frame does not end the file (truncated or extended)",
        ));
    }
    let metadata_frame = read_exact_at(&mut source, HEADER_LEN as u64, header.metadata_len)?;
    if *blake3::hash(&metadata_frame).as_bytes() != header.metadata_checksum {
        return Err(SolError::Checksum("metadata"));
    }
    let decoded = decompress(
        &metadata_frame,
        MAX_METADATA_BYTES,
        "metadata frame does not decompress",
    )?;
    // HEAD stores the config as TOML, not a serialized PostflopConfig. Its
    // SolMetadata and ProfileReport postcard layouts are unchanged.
    let metadata: SolMetadata = if version == 4 {
        postcard::from_bytes(&decoded)?
    } else {
        let (m, _): (SolMetadata, crate::run::ProfileReport) = postcard::from_bytes(&decoded)?;
        m
    };
    let hash = config_hash(metadata.config_toml.as_bytes());
    if hash != header.config_hash {
        return Err(SolError::HashMismatch {
            header_hash: config_hash_hex(&header.config_hash),
            computed_hash: config_hash_hex(&hash),
        });
    }
    if metadata.meta.iterations != header.iteration {
        return Err(SolError::Malformed(
            "header iteration differs from the metadata",
        ));
    }
    crate::prepare::require_artifact_config(&metadata.config_toml)
        .map_err(|e| SolError::Config(e.to_string()))?;
    let prepared = crate::prepare::prepare_artifact(
        &metadata.config_toml,
        Path::new("embedded.toml"),
        &metadata.meta.storage,
    )
    .map_err(|e| SolError::Config(e.to_string()))?;
    let index_frame = read_exact_at(&mut source, old_index_offset, header.index_len)?;
    if *blake3::hash(&index_frame).as_bytes() != header.index_checksum {
        return Err(SolError::Checksum("index"));
    }
    let mut decoder = zstd::stream::read::Decoder::with_buffer(index_frame.as_slice())
        .map_err(|_| SolError::Malformed("index frame does not decompress"))?
        .single_frame();
    let mut game = crate::index::read_index(&mut decoder, &prepared.config, metadata.tree_nodes)?;
    crate::index::upgrade_labels(&mut game)?;
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), ZSTD_LEVEL)?;
    crate::index::write_index(&mut encoder, &game.tree, &game.index)?;
    let new_index = encoder.finish()?;
    let directory_frame =
        read_exact_at(&mut source, header.directory_offset, header.directory_len)?;
    if *blake3::hash(&directory_frame).as_bytes() != header.directory_checksum {
        return Err(SolError::Checksum("directory"));
    }
    let decoded = decompress(
        &directory_frame,
        MAX_DIRECTORY_BYTES,
        "directory frame does not decompress",
    )?;
    let mut directory: Directory = if version == 6 {
        let (mut d, c): (Directory, Vec<ChunkEntry>) = postcard::from_bytes(&decoded)?;
        d.constraints = c;
        d
    } else {
        postcard::from_bytes(&decoded)?
    };
    let new_chunks = old_index_offset
        .checked_add(new_index.len() as u64)
        .ok_or(SolError::Malformed("invalid frame offsets"))?;
    for entry in directory
        .strategy
        .iter_mut()
        .chain(&mut directory.values)
        .chain(&mut directory.constraints)
    {
        entry.offset = entry
            .offset
            .checked_sub(old_chunks)
            .and_then(|offset| new_chunks.checked_add(offset))
            .ok_or(SolError::Malformed("inconsistent chunk directory"))?;
    }
    let encoded = if version == 6 {
        postcard::to_allocvec(&(&directory, &directory.constraints))?
    } else {
        postcard::to_allocvec(&directory)?
    };
    let new_directory = zstd::bulk::compress(&encoded, ZSTD_LEVEL)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let out = temp.as_file_mut();
    out.write_all(&[0; HEADER_LEN])?;
    out.write_all(&metadata_frame)?;
    out.write_all(&new_index)?;
    source.seek(SeekFrom::Start(old_chunks))?;
    let chunk_len = header.directory_offset - old_chunks;
    if std::io::copy(&mut source.take(chunk_len), out)? != chunk_len {
        return Err(SolError::Malformed("truncated chunks"));
    }
    out.write_all(&new_directory)?;
    header.version += 3;
    header.index_len = new_index.len() as u64;
    header.index_checksum = *blake3::hash(&new_index).as_bytes();
    header.directory_offset = new_chunks
        .checked_add(chunk_len)
        .ok_or(SolError::Malformed("invalid frame offsets"))?;
    header.directory_len = new_directory.len() as u64;
    header.directory_checksum = *blake3::hash(&new_directory).as_bytes();
    out.seek(SeekFrom::Start(0))?;
    out.write_all(&build_header(&header))?;
    out.sync_all()?;
    let mut reader = SolReader::open(temp.path())?;
    let checked = reader.read_index(|stream| {
        crate::index::read_index(stream, &prepared.config, metadata.tree_nodes)
    })?;
    let view = checked.view();
    for (id, node) in checked.tree.nodes.iter().enumerate() {
        if node.kind == NodeKind::Action
            && view.node_by_history(&view.history(id as u32)) != Some(id as u32)
        {
            return Err(SolError::Malformed(
                "upgraded node history does not resolve",
            ));
        }
    }
    temp.persist_noclobber(output)
        .map_err(|e| SolError::Io(e.error))?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}
