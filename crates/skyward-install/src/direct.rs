//! Writing an update package's partitions straight into a slot, without the
//! snapshots `update_engine` keeps: what the payload holds, partition by
//! partition, is written where the slot's copies of them are.

use crate::payload::{Manifest, Partition};
use anyhow::{ensure, Result};
use skyward_vab::Slot;
use std::path::{Path, PathBuf};

/// One partition to write, and the device it goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub name: String,
    pub device: PathBuf,
    /// How many bytes of it the payload fills.
    pub size: u64,
    /// Where the partition sits among those the manifest lists.
    pub index: usize,
}

/// The partitions of `manifest` that can be written into `slot`, and where
/// each goes among the links in `by_name`. A partition the phone does not
/// have in that slot is left out, as is one written by a step this code does
/// not carry out.
pub fn plan(manifest: &Manifest, by_name: &Path, slot: Slot) -> Result<Vec<Planned>> {
    ensure!(!manifest.partitions.is_empty(), "the package names no partitions to write");
    let mut planned = Vec::new();
    for (index, partition) in manifest.partitions.iter().enumerate() {
        let Some(device) = where_(by_name, partition, slot) else {
            continue;
        };
        if !partition.carried() {
            continue;
        }
        ensure!(partition.size > 0, "{} has no size in the package", partition.name);
        planned.push(Planned { name: partition.name.clone(), device, size: partition.size, index });
    }
    ensure!(!planned.is_empty(), "the package holds no partition this phone writes the way this code does");
    Ok(planned)
}

/// The device the partition lives in, among the links in `by_name`, once the
/// slot it belongs to is named.
fn where_(by_name: &Path, partition: &Partition, slot: Slot) -> Option<PathBuf> {
    let device = by_name.join(format!("{}{}", partition.name, slot.suffix()));
    device.exists().then_some(device)
}

/// What the payload does not carry: the partitions this code leaves as they
/// are, said for the page.
pub fn left_out(manifest: &Manifest) -> Vec<String> {
    manifest.partitions.iter().filter(|partition| !partition.carried()).map(|partition| partition.name.clone()).collect()
}

/// Writes the partitions the package holds into `slot`, reading them from
/// `package` where `update` says its payload is. `progress` is told each
/// partition's name and how many came before it, and the names of those the
/// payload does not write come back.
#[cfg(target_os = "android")]
pub fn write(
    package: &Path,
    update: &crate::package::Update,
    by_name: &Path,
    slot: Slot,
    progress: &mut dyn FnMut(&str, usize, usize),
) -> Result<Vec<String>> {
    use anyhow::Context;
    use std::fs::OpenOptions;

    let mut source = std::fs::File::open(package).with_context(|| format!("open {}", package.display()))?;
    let manifest = crate::payload::read(&mut source, update.payload_offset, update.payload_size).context("read the package's manifest")?;
    let block = manifest.block();
    let planned = plan(&manifest, by_name, slot)?;
    let total = planned.len();
    for (at, one) in planned.iter().enumerate() {
        progress(&one.name, at, total);
        let mut device = OpenOptions::new().write(true).open(&one.device).with_context(|| format!("open {}", one.device.display()))?;
        crate::image::make_writable(&device)?;
        let partition = &manifest.partitions[one.index];
        crate::payload::apply_partition(&mut source, manifest.data_at, block, partition, &mut device)
            .with_context(|| format!("write {}", one.name))?;
    }
    Ok(left_out(&manifest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::{Extent, Kind, Operation};
    use std::fs;

    fn manifest_of(partitions: Vec<Partition>) -> Manifest {
        Manifest { block_size: 4096, partitions, data_at: 0 }
    }

    fn partition(name: &str, size: u64, kind: Kind) -> Partition {
        Partition {
            name: name.into(),
            size,
            operations: vec![Operation { kind, data_offset: 0, data_length: 0, dst: vec![Extent { start_block: 0, num_blocks: size / 4096 }] }],
        }
    }

    #[test]
    fn only_the_partitions_the_slot_has_are_planned() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("boot_a"), vec![0u8; 8192]).unwrap();
        fs::write(dir.path().join("system_a"), vec![0u8; 8192]).unwrap();
        let manifest = manifest_of(vec![
            partition("boot", 8192, Kind::Replace),
            partition("system", 8192, Kind::Zero),
            partition("vendor", 8192, Kind::Zero),
        ]);
        let planned = plan(&manifest, dir.path(), Slot::A).unwrap();
        let names: Vec<&str> = planned.iter().map(|one| one.name.as_str()).collect();
        assert_eq!(names, ["boot", "system"]);
        assert!(planned[0].device.ends_with("boot_a"));
    }

    #[test]
    fn a_partition_written_by_an_unsupported_step_is_left_out() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("system_b"), vec![0u8; 8192]).unwrap();
        let manifest = manifest_of(vec![partition("system", 8192, Kind::SourceBsdiff)]);
        assert_eq!(left_out(&manifest), ["system"]);
        assert!(plan(&manifest, dir.path(), Slot::B).is_err());
    }

    #[test]
    fn a_package_with_nothing_to_write_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = manifest_of(Vec::new());
        assert!(plan(&manifest, dir.path(), Slot::A).is_err());
    }
}
