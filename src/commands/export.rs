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

    // Build a minimal ZIP file by hand (no extra dependency needed).
    // We use the "stored" (no compression) method for simplicity and zero deps.
    let mut zip = ZipBuilder::new();
    for e in &entries {
        let blob = objects::read_object(&repo.pv_dir, &e.hash)?;
        zip.add_file(&e.path, &blob.data);
    }
    let bytes = zip.finish();
    fs::write(&output, &bytes)?;

    printer::ok(&format!(
        "Exported {} file(s) from {short} → {out}",
        entries.len(),
        short = &hash[..7],
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
    if spec.len() >= 4 && spec.chars().all(|c| c.is_ascii_hexdigit()) {
        let (dir, file_prefix) = spec.split_at(2);
        let dir_path = repo.pv_dir.join("objects").join(dir);
        if dir_path.exists() {
            for entry in fs::read_dir(&dir_path)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(file_prefix) {
                    return Ok(Some(format!("{dir}{name}")));
                }
            }
        }
    }
    Ok(None)
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

    fn add_file(&mut self, name: &str, content: &[u8]) {
        let crc = crc32(content);
        let offset = self.data.len() as u32;
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
    }

    fn finish(mut self) -> Vec<u8> {
        let central_start = self.data.len() as u32;
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
        let central_size = (self.data.len() as u32) - central_start;

        // End of central directory record
        self.data.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]); // signature
        self.data.extend_from_slice(&0u16.to_le_bytes()); // disk number
        self.data.extend_from_slice(&0u16.to_le_bytes()); // disk with central
        self.data.extend_from_slice(&(self.entries.len() as u16).to_le_bytes()); // entries on disk
        self.data.extend_from_slice(&(self.entries.len() as u16).to_le_bytes()); // total entries
        self.data.extend_from_slice(&central_size.to_le_bytes());
        self.data.extend_from_slice(&central_start.to_le_bytes());
        self.data.extend_from_slice(&0u16.to_le_bytes()); // comment length

        self.data
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
