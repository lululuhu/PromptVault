//! `pv export <ref> -o <file>` — export a commit's tree as a zip archive.
//!
//! Useful for sharing a snapshot of all prompts at a given version.

use std::fs;
use std::path::PathBuf;

use anyhow::Result;

use crate::core::objects;
use crate::core::refs;
use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run(spec: &str, output: PathBuf) -> Result<()> {
    let repo = Repo::find()?;
    let hash = resolve_commit(&repo, spec)?
        .ok_or_else(|| anyhow::anyhow!("unknown commit: {spec}"))?;

    let commit = objects::read_commit(&repo.pv_dir, &hash)?;
    let entries = objects::read_tree(&repo.pv_dir, &commit.tree)?;

    // ZIP format limits (without ZIP64 extensions, which we don't implement):
    //   - max 65,535 entries (u16)
    //   - max 4 GB per file and total archive (u32 offsets/sizes)
    const MAX_ENTRIES: usize = 65_535;
    const MAX_FILE_SIZE: usize = u32::MAX as usize; // ~4 GiB

    if entries.len() > MAX_ENTRIES {
        anyhow::bail!(
            "too many files to export: {} (ZIP limit is {MAX_ENTRIES}); \
             consider exporting fewer prompts",
            entries.len()
        );
    }

    let mut zip = ZipBuilder::new();
    for e in &entries {
        let blob = objects::read_object(&repo.pv_dir, &e.hash)?;
        if blob.data.len() > MAX_FILE_SIZE {
            anyhow::bail!(
                "file too large for ZIP format: {} is {} bytes (limit is ~4 GiB)",
                e.path,
                blob.data.len()
            );
        }
        zip.add_file(&e.path, &blob.data)?;
    }
    let bytes = zip.finish()?;
    if bytes.len() > MAX_FILE_SIZE {
        anyhow::bail!(
            "resulting ZIP would exceed the 4 GiB format limit ({} bytes); \
             export fewer or smaller prompts",
            bytes.len()
        );
    }
    fs::write(&output, &bytes)?;

    printer::ok(&format!(
        "Exported {} file(s) from {short} → {out}",
        entries.len(),
        short = crate::core::safe::short_hash(&hash),
        out = output.display()
    ));
    Ok(())
}

fn resolve_commit(repo: &Repo, spec: &str) -> Result<Option<String>> {
    if let Some(h) = refs::resolve_tag(&repo.pv_dir, spec)? {
        return Ok(Some(h));
    }
    if let Some(h) = refs::resolve_branch(&repo.pv_dir, spec)? {
        return Ok(Some(h));
    }
    if spec == "HEAD" {
        return repo.head_commit();
    }
    crate::core::safe::resolve_hash_prefix(&repo.pv_dir.join("objects"), spec)
}

// ---- Minimal ZIP writer (stored, no compression) -------------------------

struct ZipEntry {
    name: String,
    offset: u32,
    crc32: u32,
    size: u32,
}

struct ZipBuilder {
    data: Vec<u8>,
    entries: Vec<ZipEntry>,
}

impl ZipBuilder {
    fn new() -> Self {
        Self { data: Vec::new(), entries: Vec::new() }
    }

    fn add_file(&mut self, name: &str, content: &[u8]) -> Result<()> {
        let crc = crc32(content);
        let offset = self.data.len();
        if offset > u32::MAX as usize {
            anyhow::bail!("ZIP offset overflow: archive exceeds 4 GiB limit");
        }
        let offset = offset as u32;
        let size = content.len() as u32;

        // Local file header
        self.data.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]); // signature
        self.data.extend_from_slice(&20u16.to_le_bytes()); // version needed
        self.data.extend_from_slice(&0u16.to_le_bytes()); // flags
        self.data.extend_from_slice(&0u16.to_le_bytes()); // method = stored
        self.data.extend_from_slice(&0u16.to_le_bytes()); // mod time
        self.data.extend_from_slice(&0u16.to_le_bytes()); // mod date
        self.data.extend_from_slice(&crc.to_le_bytes());
        self.data.extend_from_slice(&size.to_le_bytes()); // compressed size
        self.data.extend_from_slice(&size.to_le_bytes()); // uncompressed size
        self.data.extend_from_slice(&(name.len() as u16).to_le_bytes());
        self.data.extend_from_slice(&0u16.to_le_bytes()); // extra field length
        self.data.extend_from_slice(name.as_bytes());
        self.data.extend_from_slice(content);

        self.entries.push(ZipEntry {
            name: name.to_string(),
            offset,
            crc32: crc,
            size,
        });
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<u8>> {
        let central_start = self.data.len();
        if central_start > u32::MAX as usize {
            anyhow::bail!("ZIP central directory offset exceeds 4 GiB limit");
        }
        let central_start = central_start as u32;
        for e in &self.entries {
            // Central directory file header
            self.data.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]); // signature
            self.data.extend_from_slice(&20u16.to_le_bytes()); // version made by
            self.data.extend_from_slice(&20u16.to_le_bytes()); // version needed
            self.data.extend_from_slice(&0u16.to_le_bytes()); // flags
            self.data.extend_from_slice(&0u16.to_le_bytes()); // method = stored
            self.data.extend_from_slice(&0u16.to_le_bytes()); // mod time
            self.data.extend_from_slice(&0u16.to_le_bytes()); // mod date
            self.data.extend_from_slice(&e.crc32.to_le_bytes());
            self.data.extend_from_slice(&e.size.to_le_bytes()); // compressed
            self.data.extend_from_slice(&e.size.to_le_bytes()); // uncompressed
            self.data.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            self.data.extend_from_slice(&0u16.to_le_bytes()); // extra
            self.data.extend_from_slice(&0u16.to_le_bytes()); // comment
            self.data.extend_from_slice(&0u16.to_le_bytes()); // disk number
            self.data.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            self.data.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            self.data.extend_from_slice(&e.offset.to_le_bytes()); // local header offset
            self.data.extend_from_slice(e.name.as_bytes());
        }
        let central_size = (self.data.len() as u32).saturating_sub(central_start);

        // End of central directory record
        self.data.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]); // signature
        self.data.extend_from_slice(&0u16.to_le_bytes()); // disk number
        self.data.extend_from_slice(&0u16.to_le_bytes()); // disk with central
        self.data.extend_from_slice(&(self.entries.len() as u16).to_le_bytes()); // entries on disk
        self.data.extend_from_slice(&(self.entries.len() as u16).to_le_bytes()); // total entries
        self.data.extend_from_slice(&central_size.to_le_bytes());
        self.data.extend_from_slice(&central_start.to_le_bytes());
        self.data.extend_from_slice(&0u16.to_le_bytes()); // comment length

        Ok(self.data)
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            if c & 1 == 1 {
                c = 0xedb88320 ^ (c >> 1);
            } else {
                c >>= 1;
            }
        }
        table[i as usize] = c;
    }
    let mut crc = 0xffffffffu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffffffff
}
