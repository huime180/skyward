//! The manifest inside an A/B update package's `payload.bin`: the partitions
//! it holds, how large each is, and the steps that produce it. Reading the
//! manifest is what lets a package be written partition by partition without
//! the snapshots `update_engine` would keep.

use anyhow::{ensure, Result};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// The four bytes every payload begins with.
pub const MAGIC: &[u8; 4] = b"CrAU";

/// The blocks a payload counts in, when its manifest names none.
pub const BLOCK: u64 = 4096;

/// A run of blocks within a partition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Extent {
    pub start_block: u64,
    pub num_blocks: u64,
}

impl Extent {
    /// Where the run starts among the partition's bytes.
    pub fn at(&self, block: u64) -> u64 {
        self.start_block * block
    }

    /// How many bytes the run holds.
    pub fn len(&self, block: u64) -> u64 {
        self.num_blocks * block
    }
}

/// What the payload's own header says: the format it is in, how long its
/// manifest is, and how long the signature over that manifest is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub version: u64,
    pub manifest_size: u64,
    pub metadata_signature_size: u32,
}

impl Header {
    /// Where the manifest starts among the payload's bytes.
    pub fn manifest_at(&self) -> usize {
        match self.version {
            2.. => 4 + 8 + 8 + 4,
            _ => 4 + 8 + 8,
        }
    }
}

/// The header that opens `bytes`.
pub fn header(bytes: &[u8]) -> Result<Header> {
    ensure!(bytes.starts_with(MAGIC), "the payload does not begin with CrAU");
    let version = u64::from_be_bytes(bytes[4..12].try_into().expect("eight bytes"));
    let manifest_size = u64::from_be_bytes(bytes[12..20].try_into().expect("eight bytes"));
    let signature = match version {
        2.. => u32::from_be_bytes(bytes[20..24].try_into().expect("four bytes")),
        _ => 0,
    };
    Ok(Header { version, manifest_size, metadata_signature_size: signature })
}

/// How one of a partition's steps produces blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Replace,
    ReplaceBz,
    ReplaceXz,
    ReplaceBrotli,
    Zero,
    SourceCopy,
    SourceBsdiff,
    BrotliBsdiff,
    Puffdiff,
    Zstd,
    /// A step this code does not carry out.
    Unknown(i32),
}

impl Kind {
    fn of(value: i32) -> Kind {
        // The numbers update_metadata.proto gives each step.
        match value {
            0 => Kind::Replace,
            1 => Kind::ReplaceBz,
            4 => Kind::SourceCopy,
            5 => Kind::SourceBsdiff,
            6 => Kind::Zero,
            8 => Kind::ReplaceXz,
            10 => Kind::BrotliBsdiff,
            other => Kind::Unknown(other),
        }
    }

    /// Whether the step reads the payload's data at an offset, rather than
    /// making the blocks from what it finds.
    pub fn reads_data(&self) -> bool {
        matches!(self, Kind::Replace | Kind::ReplaceBz | Kind::ReplaceXz | Kind::ReplaceBrotli | Kind::Zstd)
    }

    /// Whether this code carries the step out.
    pub fn carried(&self) -> bool {
        matches!(self, Kind::Replace | Kind::Zero | Kind::ReplaceBz | Kind::ReplaceXz | Kind::ReplaceBrotli | Kind::Zstd)
    }

    /// What a manifest calls the step.
    pub fn name(&self) -> String {
        match self {
            Kind::Replace => "REPLACE",
            Kind::ReplaceBz => "REPLACE_BZ",
            Kind::ReplaceBrotli => "REPLACE_BROTLI",
            Kind::ReplaceXz => "REPLACE_XZ",
            Kind::Zero => "ZERO",
            Kind::SourceCopy => "SOURCE_COPY",
            Kind::SourceBsdiff => "SOURCE_BSDIFF",
            Kind::BrotliBsdiff => "BROTLI_BSDIFF",
            Kind::Puffdiff => "PUFFDIFF",
            Kind::Zstd => "REPLACE_ZSTD",
            Kind::Unknown(value) => return format!("type {value}"),
        }
        .to_string()
    }
}

/// One step of a partition, as the manifest lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
    pub kind: Kind,
    /// Where its data sits in the payload, for the steps that read any.
    pub data_offset: u64,
    pub data_length: u64,
    /// The runs of the partition it fills.
    pub dst: Vec<Extent>,
}

impl Operation {
    /// How many bytes the step writes.
    pub fn len(&self, block: u64) -> u64 {
        self.dst.iter().map(|extent| extent.len(block)).sum()
    }
}

/// A partition the payload writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Partition {
    pub name: String,
    /// How large the partition is once written, in bytes.
    pub size: u64,
    pub operations: Vec<Operation>,
}

impl Partition {
    /// Whether every step of the partition is one this code carries out.
    pub fn carried(&self) -> bool {
        self.operations.iter().all(|operation| operation.kind.carried())
    }
}

/// What the payload says to write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub block_size: u32,
    pub partitions: Vec<Partition>,
    /// Where the steps' data begins among the package's bytes: the payload's
    /// header, manifest and the signature over it come first.
    pub data_at: u64,
}

impl Manifest {
    /// The block size the steps count in.
    pub fn block(&self) -> u64 {
        match self.block_size {
            0 => BLOCK,
            size => u64::from(size),
        }
    }
}

/// The manifest of the payload that sits at `at` among `source`'s bytes and
/// runs for `size` of them, as a package's `payload.bin` does.
pub fn read(source: &mut (impl Read + Seek), at: u64, size: u64) -> Result<Manifest> {
    source.seek(SeekFrom::Start(at))?;
    let mut opening = [0u8; 24];
    source.read_exact(&mut opening[..20])?;
    let version = u64::from_be_bytes(opening[4..12].try_into().expect("eight bytes"));
    let held = match version {
        2.. => 24,
        _ => 20,
    };
    if held == 24 {
        source.read_exact(&mut opening[20..24])?;
    }
    let head = header(&opening[..held])?;
    ensure!(head.manifest_size <= size, "the manifest is longer than the payload");
    let mut manifest_bytes = vec![0u8; head.manifest_size as usize];
    source.seek(SeekFrom::Start(at + head.manifest_at() as u64))?;
    source.read_exact(&mut manifest_bytes)?;
    let mut read = manifest(&manifest_bytes)?;
    read.data_at = at + head.manifest_at() as u64 + head.manifest_size + u64::from(head.metadata_signature_size);
    Ok(read)
}

/// The steps of `partition` this code does not carry out, by the names the
/// manifest gives them, each said once.
fn unnamed_steps(partition: &Partition) -> String {
    let mut said: Vec<String> = Vec::new();
    for operation in &partition.operations {
        let name = operation.kind.name();
        if !operation.kind.carried() && !said.contains(&name) {
            said.push(name);
        }
    }
    said.join(" ")
}

/// How much of a step's data is read at a time.
const CHUNK: u64 = 1024 * 1024;

/// What a step's data comes to once it is unpacked, `want` bytes of it.
fn unpack(kind: Kind, data: &[u8], want: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(want);
    match kind {
        Kind::Replace => {
            out.extend_from_slice(data);
        }
        Kind::ReplaceBz => {
            let mut held = bzip2_rs::DecoderReader::new(data);
            std::io::Read::read_to_end(&mut held, &mut out)?;
        }
        Kind::ReplaceXz => {
            lzma_rs::xz_decompress(&mut std::io::Cursor::new(data), &mut out)?;
        }
        Kind::ReplaceBrotli => {
            let mut held = brotli::Decompressor::new(data, 4096);
            std::io::Read::read_to_end(&mut held, &mut out)?;
        }
        Kind::Zstd => {
            let mut held = ruzstd::decoding::StreamingDecoder::new(std::io::Cursor::new(data))
                .map_err(|e| anyhow::anyhow!("unpack zstd: {e}"))?;
            std::io::Read::read_to_end(&mut held, &mut out)?;
        }
        other => anyhow::bail!("a step of {} is not carried out here", other.name()),
    }
    ensure!(out.len() == want, "a step of {} unpacks to {} bytes, not {want}", kind.name(), out.len());
    Ok(out)
}

/// Writes what `partition` holds into `out`, reading the steps' data from
/// `source` — the package, whose `payload.bin` starts at `at` — a chunk at a
/// time, so a package of ten gigabytes never sits in memory.
pub fn apply_partition(source: &mut (impl Read + Seek), at: u64, block: u64, partition: &Partition, out: &mut (impl Write + Seek)) -> Result<()> {
    apply_while(source, at, block, partition, out, &mut |_, _| {})
}

/// The same, telling how far it has gone as it works: the bytes written and
/// how many the partition comes to in all.
pub fn apply_while(
    source: &mut (impl Read + Seek),
    at: u64,
    block: u64,
    partition: &Partition,
    out: &mut (impl Write + Seek),
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    let all: u64 = partition.operations.iter().map(|operation| operation.len(block)).sum();
    let mut written = 0u64;
    ensure!(
        partition.carried(),
        "{} is written by steps this code does not carry out ({})",
        partition.name,
        unnamed_steps(partition)
    );
    for operation in &partition.operations {
        let wants = operation.len(block);
        match operation.kind {
            Kind::Replace => {
                ensure!(wants == operation.data_length, "{} reads {} bytes and writes {wants}", partition.name, operation.data_length);
                source.seek(SeekFrom::Start(at + operation.data_offset))?;
                let mut left = operation.data_length;
                for run in &operation.dst {
                    out.seek(SeekFrom::Start(run.at(block)))?;
                    let mut len = run.len(block);
                    while len > 0 {
                        let step = len.min(CHUNK);
                        let mut held = vec![0u8; step as usize];
                        source.read_exact(&mut held)?;
                        out.write_all(&held)?;
                        len -= step;
                        left = left.saturating_sub(step);
                        written += step;
                        progress(written, all);
                    }
                }
                ensure!(left == 0, "{} reads more than its runs hold", partition.name);
            }
            Kind::Zero => {
                for run in &operation.dst {
                    out.seek(SeekFrom::Start(run.at(block)))?;
                    let mut len = run.len(block);
                    while len > 0 {
                        let step = len.min(CHUNK);
                        out.write_all(&vec![0u8; step as usize])?;
                        len -= step;
                        written += step;
                        progress(written, all);
                    }
                }
            }
            // A run packed by one of the four ways the manifest names is
            // unpacked whole, then written across the runs it fills.
            Kind::ReplaceBz | Kind::ReplaceXz | Kind::ReplaceBrotli | Kind::Zstd => {
                source.seek(SeekFrom::Start(at + operation.data_offset))?;
                let mut packed = vec![0u8; usize::try_from(operation.data_length).unwrap_or(usize::MAX)];
                source.read_exact(&mut packed)?;
                let data = unpack(operation.kind, &packed, wants as usize)?;
                let mut from = 0;
                for run in &operation.dst {
                    out.seek(SeekFrom::Start(run.at(block)))?;
                    let len = run.len(block) as usize;
                    out.write_all(&data[from..from + len])?;
                    from += len;
                    written += len as u64;
                    progress(written, all);
                }
            }
            _ => anyhow::bail!("{} is written by steps this code does not carry out ({})", partition.name, unnamed_steps(partition)),
        }
    }
    Ok(())
}

/// Writes the image `partition` holds to the file at `out`, reading it from
/// `source` where the payload sits at `at`. The partition's own size is what
/// the file comes to.
pub fn extract(source: &mut (impl Read + Seek), at: u64, block: u64, partition: &Partition, out: &Path) -> Result<()> {
    extract_while(source, at, block, partition, out, &mut |_, _| {})
}

/// The same, telling how far it has gone as it works: the bytes written and
/// how many there are in all.
pub fn extract_while(
    source: &mut (impl Read + Seek),
    at: u64,
    block: u64,
    partition: &Partition,
    out: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<()> {
    let mut file = std::fs::File::create(out).map_err(|e| anyhow::anyhow!("create {}: {e}", out.display()))?;
    let written = apply_while(source, at, block, partition, &mut file, progress);
    if let Err(e) = written {
        // A file that came to nothing is taken away, so an empty image is
        // never taken for a whole one.
        drop(file);
        let _ = std::fs::remove_file(out);
        return Err(e);
    }
    Ok(())
}

/// The manifest `manifest` holds.
pub fn manifest(manifest: &[u8]) -> Result<Manifest> {
    let mut block_size = 0;
    let mut partitions = Vec::new();
    for (field, value) in fields(manifest)? {
        match field {
            // 3 is the block size the steps count in; 13 is the partition list.
            3 => block_size = u32::try_from(number(value)?).unwrap_or(0),
            13 => partitions.push(partition(value)?),
            _ => {}
        }
    }
    Ok(Manifest { block_size, partitions, data_at: 0 })
}

fn partition(bytes: &[u8]) -> Result<Partition> {
    let mut name = String::new();
    let mut size = 0;
    let mut operations = Vec::new();
    for (field, value) in fields(bytes)? {
        match field {
            1 => name = String::from_utf8_lossy(value).into_owned(),
            // The partition as it is written, a message of its own: its
            // size comes first, then the hash it is checked against.
            7 => {
                for (inner, held) in fields(value)? {
                    if inner == 1 {
                        size = number(held)?;
                    }
                }
            }
            8 => operations.push(operation(value)?),
            _ => {}
        }
    }
    ensure!(!name.is_empty(), "a partition of the manifest has no name");
    Ok(Partition { name, size, operations })
}

fn operation(bytes: &[u8]) -> Result<Operation> {
    let mut kind = Kind::Replace;
    let mut data_offset = 0;
    let mut data_length = 0;
    let mut dst = Vec::new();
    for (field, value) in fields(bytes)? {
        match field {
            1 => kind = Kind::of(i32::try_from(number(value)?).unwrap_or(-1)),
            2 => data_offset = number(value)?,
            3 => data_length = number(value)?,
            6 => dst.push(extent(value)?),
            _ => {}
        }
    }
    Ok(Operation { kind, data_offset, data_length, dst })
}

fn extent(bytes: &[u8]) -> Result<Extent> {
    let mut start_block = 0;
    let mut num_blocks = 0;
    for (field, value) in fields(bytes)? {
        match field {
            1 => start_block = number(value)?,
            2 => num_blocks = number(value)?,
            _ => {}
        }
    }
    Ok(Extent { start_block, num_blocks })
}

/// Writes what `partition` holds into `out`, taking each step's data from
/// `payload` — the bytes of `payload.bin`, whose start sits at `payload_at`
/// in the file the offsets count from.
/// The `size` field of a partition's description.
fn number(bytes: &[u8]) -> Result<u64> {
    let mut at = 0;
    let value = varint(bytes, &mut at)?;
    Ok(value)
}

/// The fields of a message, as the numbers they carry and the bytes of each.
fn fields(bytes: &[u8]) -> Result<Vec<(u64, &[u8])>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let key = varint(bytes, &mut at)?;
        let (field, wire) = (key >> 3, key & 7);
        match wire {
            // A number, which only the fields this code reads use for their
            // own length or size: kept as its own bytes.
            0 => {
                let start = at;
                varint(bytes, &mut at)?;
                out.push((field, &bytes[start..at]));
            }
            2 => {
                let len = usize::try_from(varint(bytes, &mut at)?).unwrap_or(usize::MAX);
                let end = at.checked_add(len).filter(|end| *end <= bytes.len());
                let end = end.ok_or_else(|| anyhow::anyhow!("a field of the manifest runs past its end"))?;
                out.push((field, &bytes[at..end]));
                at = end;
            }
            // Fixed-width numbers are not among the fields read here; skip
            // them so a newer manifest still parses.
            1 => at += 8,
            5 => at += 4,
            other => anyhow::bail!("a field of the manifest has wire type {other}"),
        }
    }
    Ok(out)
}

/// A base-128 number, as protobuf writes them.
fn varint(bytes: &[u8], at: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for shift in 0..10 {
        let byte = *bytes.get(*at).ok_or_else(|| anyhow::anyhow!("a number of the manifest runs past its end"))?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << (shift * 7);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    anyhow::bail!("a number of the manifest is too long")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn varint_bytes(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
        out
    }

    fn key(field: u64, wire: u64) -> Vec<u8> {
        varint_bytes((field << 3) | wire)
    }

    fn len_field(field: u64, value: &[u8]) -> Vec<u8> {
        let mut out = key(field, 2);
        out.extend(varint_bytes(value.len() as u64));
        out.extend_from_slice(value);
        out
    }

    fn num_field(field: u64, value: u64) -> Vec<u8> {
        let mut out = key(field, 0);
        out.extend(varint_bytes(value));
        out
    }

    #[test]
    fn a_header_says_where_its_manifest_is() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend(2u64.to_be_bytes());
        bytes.extend(64u64.to_be_bytes());
        bytes.extend(32u32.to_be_bytes());
        let head = header(&bytes).unwrap();
        assert_eq!(head, Header { version: 2, manifest_size: 64, metadata_signature_size: 32 });
        assert_eq!(head.manifest_at(), 24);
        assert!(header(b"nope").is_err());
    }

    #[test]
    fn the_manifest_names_the_partitions_and_their_steps() {
        // One partition, `boot`, whose image is 4 MiB and made of a zeroed
        // run and one replace step that reads the payload at 4096.
        let mut operation = num_field(1, 0);
        operation.extend(num_field(2, 4096));
        operation.extend(num_field(3, 2048));
        let info = num_field(1, 4 * 1024 * 1024);
        let mut partition = len_field(1, b"boot");
        partition.extend(len_field(7, &info));
        partition.extend(len_field(8, &operation));
        let mut zeroed = num_field(1, 6);
        zeroed.extend(num_field(3, 1024));
        partition.extend(len_field(8, &zeroed));
        let mut all = num_field(3, 4096);
        all.extend(len_field(13, &partition));

        let parsed = manifest(&all).unwrap();
        assert_eq!(parsed.block(), 4096);
        let parts = parsed.partitions;
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "boot");
        assert_eq!(parts[0].size, 4 * 1024 * 1024);
        assert_eq!(parts[0].operations[0].kind, Kind::Replace);
        assert_eq!(parts[0].operations[0].data_offset, 4096);
        assert_eq!(parts[0].operations[0].data_length, 2048);
        assert_eq!(parts[0].operations[1].kind, Kind::Zero);
        assert!(parts[0].operations[0].kind.reads_data());
        assert!(!parts[0].operations[1].kind.reads_data());
    }

    fn manifest_of(partition: &[u8]) -> Vec<u8> {
        let mut all = num_field(3, 4096);
        all.extend(len_field(13, partition));
        all
    }

    fn partition_of(name: &str, size: u64, operations: &[Vec<u8>]) -> Vec<u8> {
        let mut partition = len_field(1, name.as_bytes());
        partition.extend(len_field(7, &num_field(1, size)));
        for operation in operations {
            partition.extend(len_field(8, operation));
        }
        partition
    }

    fn zero(start: u64, blocks: u64) -> Vec<u8> {
        // Six is ZERO, as the manifest numbers the steps.
        let mut operation = num_field(1, 6);
        let mut run = num_field(1, start);
        run.extend(num_field(2, blocks));
        operation.extend(len_field(6, &run));
        operation
    }

    #[test]
    fn a_payload_reads_its_manifest_from_where_it_sits() {
        let partition = partition_of("vendor", 8192, &[zero(0, 2)]);
        let manifest_bytes = manifest_of(&partition);
        let mut payload = MAGIC.to_vec();
        payload.extend(2u64.to_be_bytes());
        payload.extend((manifest_bytes.len() as u64).to_be_bytes());
        payload.extend(0u32.to_be_bytes());
        payload.extend_from_slice(&manifest_bytes);
        // The payload sits after a hundred bytes of something else in the
        // file, as it does after a package's other files.
        let mut file = vec![0xAAu8; 100];
        file.extend_from_slice(&payload);
        let mut source = Cursor::new(file);
        let read_back = read(&mut source, 100, payload.len() as u64).unwrap();
        assert_eq!(read_back.partitions.len(), 1);
        assert_eq!(read_back.partitions[0].name, "vendor");
        assert_eq!(read_back.partitions[0].size, 8192);
    }

    #[test]
    fn a_zero_step_zeroes_the_blocks_it_names() {
        let partition = Partition {
            name: "boot".into(),
            size: 2 * 4096,
            operations: vec![Operation { kind: Kind::Zero, data_offset: 0, data_length: 0, dst: vec![Extent { start_block: 0, num_blocks: 2 }] }],
        };
        let mut source = Cursor::new(Vec::new());
        let mut out = Cursor::new(vec![7u8; 2 * 4096]);
        apply_partition(&mut source, 0, 4096, &partition, &mut out).unwrap();
        assert!(out.into_inner().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn a_step_of_an_unknown_kind_is_kept_as_it_is() {
        let unknown = Operation { kind: Kind::Unknown(77), data_offset: 0, data_length: 0, dst: Vec::new() };
        assert_eq!(Operation { kind: Kind::of(77), data_offset: 0, data_length: 0, dst: Vec::new() }, unknown);
        assert!(!unknown.kind.reads_data());
        assert!(!unknown.kind.carried());
    }
}
