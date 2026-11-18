//! Whether the image a phone starts from already carries root, and which
//! manager put it there: a patched boot image holds the manager's own files,
//! and their names are what says so.

use crate::apk::Kind;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// What a patched image holds, by the manager that put it there.
const MARKS: [(Kind, [&[u8]; 3]); 3] = [
    (Kind::Magisk, [b"MAGISK", b"magiskinit", b".backup/.magisk"]),
    (Kind::KernelSu, [b"KernelSU", b"kernelsu.ko", b"ksud"]),
    (Kind::Apatch, [b"APatch", b"kpatch", b"kptools"]),
];

/// The manager whose files the image at `image` holds, if any.
pub fn of_image(image: &Path) -> Option<Kind> {
    let mut file = File::open(image).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    for (kind, marks) in MARKS {
        if marks.iter().any(|mark| bytes.windows(mark.len()).any(|window| window == *mark)) {
            return Some(kind);
        }
    }
    None
}

/// The manager the image of `partition` in `slot` holds, if any: the one the
/// phone would start with.
pub fn of_partition(dir: &Path, partition: &str, slot: skyward_vab::Slot) -> Option<Kind> {
    let device = dir.join(format!("{partition}{}", slot.suffix()));
    of_image(&device)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn an_image_with_a_manager_in_it_says_which_one() {
        let dir = tempfile::tempdir().unwrap();
        let at = dir.path().join("boot.img");
        let mut bytes = vec![0u8; 4096];
        bytes[2048..2048 + 12].copy_from_slice(b"KernelSU-ko\0");
        fs::write(&at, &bytes).unwrap();
        assert_eq!(of_image(&at), Some(Kind::KernelSu));
    }

    #[test]
    fn a_stock_image_holds_none_of_them() {
        let dir = tempfile::tempdir().unwrap();
        let at = dir.path().join("boot.img");
        fs::write(&at, vec![0u8; 4096]).unwrap();
        assert_eq!(of_image(&at), None);
    }

    #[test]
    fn magisk_says_it_is_magisk() {
        let dir = tempfile::tempdir().unwrap();
        let at = dir.path().join("init_boot.img");
        let mut bytes = vec![0u8; 4096];
        bytes[1024..1030].copy_from_slice(b"MAGISK");
        fs::write(&at, &bytes).unwrap();
        assert_eq!(of_image(&at), Some(Kind::Magisk));
    }
}
