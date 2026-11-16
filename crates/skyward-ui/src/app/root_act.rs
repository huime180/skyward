//! What the root pages do once a control is touched: where a manager's
//! patcher came from, which image the patch writes and where, and the work
//! that runs for them.

use super::*;

impl App {
    /// A manager chosen becomes the one the patch runs, and the page it was
    /// chosen on gives way to the root page.
    pub fn root_manager_chosen(&mut self, way: ManagerWay, source: RootSource) {
        self.state.root_manager_way = way;
        self.state.root_source = source;
        self.state.root_said = None;
        self.back_to_root();
    }

    /// The page a chosen package comes back to.
    pub(crate) fn back_to_root(&mut self) {
        while self.stack.len() > 1 && self.screen() != Screen::Root {
            self.stack.pop();
        }
        self.settle_page();
        self.refresh();
    }

    /// Starts taking the partition at `name` out of the package on show, to
    /// internal storage, on a thread of its own.
    pub(crate) fn package_take(&mut self, name: &str) {
        let Some((path, offset, size, block)) = self.state.package_file.clone() else {
            self.state.package_said = Some("no package on show".into());
            return;
        };
        let name = name.to_string();
        if let Ok(mut held) = TAKING.lock() {
            *held = Some((name.clone(), 0, 1));
        }
        std::thread::spawn(move || {
            let taken = (|| -> anyhow::Result<String> {
                let mut file = std::fs::File::open(&path)?;
                let manifest = skyward_install::payload::read(&mut file, offset, size)?;
                let partition = manifest
                    .partitions
                    .iter()
                    .find(|one| one.name == name)
                    .ok_or_else(|| anyhow::anyhow!("{name} is not in this package"))?;
                let dir = std::path::Path::new("/sdcard/image");
                std::fs::create_dir_all(dir)?;
                let out = format!("/sdcard/image/{name}.img");
                skyward_install::payload::extract_while(&mut file, manifest.data_at, block, partition, std::path::Path::new(&out), &mut |done, all| {
                    if let Ok(mut held) = TAKING.lock() {
                        *held = Some((name.clone(), done, all));
                    }
                })?;
                Ok(out)
            })();
            if let Ok(mut held) = TAKING.lock() {
                *held = None;
            }
            if let Ok(mut held) = TAKEN.lock() {
                *held = Some(taken.map_err(|e| format!("{e:#}")));
            }
        });
    }

    /// The image taken out of a package, once it is out.
    pub(crate) fn package_taken(&mut self, taken: Result<String, String>) {
        match taken {
            Ok(path) => {
                self.state.package_said = Some(path.clone());
                match self.state.package_mode {
                    Some(PackageMode::Flash) => self.state.pending_flash = Some(path),
                    _ => self.state.root_image = RootImage::File(path),
                }
            }
            Err(why) => self.state.package_said = Some(why),
        }
    }
}
