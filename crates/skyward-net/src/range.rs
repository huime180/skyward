//! Reading a package on its server as it is needed, and taking one file out
//! of it: a workflow hands its build out as an archive of packages, and what
//! a phone needs of it is the one package for its architecture.

use anyhow::{bail, Context, Result};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// How much of the package is read at a time: one request carries a block,
/// and what falls in it is served from there.
const BLOCK: u64 = 1 << 20;

/// A package on its server, read where it is asked for.
pub struct Ranged {
    agent: ureq::Agent,
    url: String,
    len: u64,
    at: u64,
    block_at: u64,
    block: Vec<u8>,
}

impl Ranged {
    /// The package at `url`, whose length the server is asked for.
    pub fn open(agent: &ureq::Agent, url: &str) -> Result<Ranged> {
        let head = agent.head(url).call().with_context(|| format!("ask {url}"))?;
        let len = head
            .headers()
            .get(ureq::http::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .with_context(|| format!("{url} says no length"))?;
        Ok(Ranged { agent: agent.clone(), url: url.to_string(), len, at: 0, block_at: 0, block: Vec::new() })
    }

    /// Read the block holding `at`, so that what falls in it is served from
    /// there.
    fn load(&mut self, at: u64) -> Result<()> {
        let start = at / BLOCK * BLOCK;
        if start == self.block_at && !self.block.is_empty() {
            return Ok(());
        }
        let end = (start + BLOCK).min(self.len).saturating_sub(1);
        let mut response = self
            .agent
            .get(&self.url)
            .header("Range", format!("bytes={start}-{end}"))
            .call()
            .with_context(|| format!("ask {} for bytes {start}-{end}", self.url))?;
        match response.status().as_u16() {
            206 | 200 => {}
            status => bail!("{} answered {status}", self.url),
        }
        let mut block = Vec::new();
        response
            .body_mut()
            .with_config()
            .limit(u64::MAX)
            .reader()
            .read_to_end(&mut block)
            .with_context(|| format!("read {} at {start}", self.url))?;
        self.block = block;
        self.block_at = start;
        Ok(())
    }

    /// Fill `buf` with what the package holds from `at`, and say how much.
    fn fill(&mut self, at: u64, buf: &mut [u8]) -> Result<usize> {
        self.load(at)?;
        let from = (at - self.block_at) as usize;
        if from >= self.block.len() {
            bail!("{} sent less than was asked", self.url);
        }
        let n = buf.len().min(self.block.len() - from);
        buf[..n].copy_from_slice(&self.block[from..from + n]);
        Ok(n)
    }
}

impl Read for Ranged {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.at >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let at = self.at;
        let n = self.fill(at, buf).map_err(std::io::Error::other)?;
        self.at += n as u64;
        Ok(n)
    }
}

impl Seek for Ranged {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let at = match to {
            SeekFrom::Start(at) => at as i64,
            SeekFrom::Current(by) => self.at as i64 + by,
            SeekFrom::End(by) => self.len as i64 + by,
        };
        if at < 0 {
            return Err(std::io::Error::other("before the package"));
        }
        self.at = at as u64;
        Ok(self.at)
    }
}

/// Take the file of `url` whose name holds `word` into `out`, reading only
/// the parts of the package that file lies in. `Ok(false)` where the package
/// holds no such file.
pub fn take(agent: &ureq::Agent, url: &str, word: &str, out: &Path) -> Result<bool> {
    let mut remote = Ranged::open(agent, url)?;
    let mut zip = zip::ZipArchive::new(&mut remote).with_context(|| format!("read the directory of {url}"))?;
    let names: Vec<String> = zip.file_names().filter(|name| name.to_ascii_lowercase().ends_with(".apk")).map(str::to_string).collect();
    let Some(pick) = names.iter().find(|name| name.contains(word)).or_else(|| names.first()).cloned() else {
        return Ok(false);
    };
    let mut entry = zip.by_name(&pick).with_context(|| format!("open {pick} in {url}"))?;
    let mut file = std::fs::File::create(out).with_context(|| format!("create {}", out.display()))?;
    std::io::copy(&mut entry, &mut file).with_context(|| format!("write {}", out.display()))?;
    file.flush().ok();
    Ok(true)
}
