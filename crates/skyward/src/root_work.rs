//! The work the root pages ask for, on threads of its own: downloading a
//! manager's package, and patching a boot image with one. Both leave their
//! outcome where the pages' loop reads it.

use anyhow::{ensure, Context, Result};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use skyward_net::http::Servers;
use skyward_ui::app::{RootImage, RootSource};

/// What a manager's package is: where it is downloaded from, and what the
/// file it lands in is called.
struct ManagerPlan {
    name: String,
    url: String,
}

/// How patching a boot image ended: the partition it wrote, and where the
/// image it held went.
static PATCHED: Mutex<Option<Result<String>>> = Mutex::new(None);

/// How downloading a manager's package ended: where the package is.
static DOWNLOADED: Mutex<Option<Result<String>>> = Mutex::new(None);

/// What patching ended with, once it has.
pub fn patched() -> Option<Result<String>> {
    take(&PATCHED)
}

/// What downloading a manager's package ended with, once it has.
pub fn downloaded() -> Option<Result<String>> {
    take(&DOWNLOADED)
}

fn take(held: &Mutex<Option<Result<String>>>) -> Option<Result<String>> {
    held.lock().ok().and_then(|mut slot| slot.take())
}

/// Patch the image `image` names with the manager `source` names, and put it
/// into `partition`, or into the one the phone's root manager keeps.
pub fn patch_root(partition: Option<&'static str>, source: RootSource, image: RootImage) {
    let done = patch_root_in(partition, &source, &image);
    match &done {
        Ok(said) => {
            let _ = skyward_sys::kmsg(&format!("root: {said}"));
        }
        Err(e) => {
            let _ = skyward_sys::kmsg(&format!("root: patching failed: {e:#}"));
        }
    }
    if let Ok(mut held) = PATCHED.lock() {
        *held = Some(done);
    }
}

fn patch_root_in(partition: Option<&'static str>, source: &RootSource, image: &RootImage) -> Result<String> {
    let slot = skyward_vab::current_slot()?.context("not an A/B phone")?;
    let work = Path::new("/tmp/root");
    std::fs::create_dir_all(work).with_context(|| format!("create {}", work.display()))?;
    let by_name = Path::new(skyward_root::BY_NAME);
    let partition = partition.unwrap_or_else(|| skyward_root::target(by_name, slot));
    // What the partition holds now is kept aside before anything is written
    // to it, whichever way the image to write is come by.
    let stock = work.join("stock.img");
    skyward_root::read(by_name, partition, slot, &stock)?;
    let kept = keep_aside(&stock, partition, slot, work)?;
    let _ = skyward_sys::kmsg(&format!("root: the image {partition}{} held is at {}", slot.suffix(), kept.display()));
    let base = match image {
        RootImage::Partition => &stock,
        RootImage::File(at) => Path::new(at),
    };
    let manager = manager_from(source, work)?;
    let patched = work.join("patched.img");
    let _ = skyward_sys::kmsg(&format!("root: {} patches {partition}{} from {}", manager.name(), slot.suffix(), base.display()));
    let options = skyward_root::Options::default();
    match &manager {
        skyward_root::Manager::Magisk(magisk) => magisk.patch(base, &patched, &options)?,
        skyward_root::Manager::KernelSu(ksu) => ksu.patch(base, &patched)?,
        skyward_root::Manager::Apatch(apatch) => apatch.patch(base, &patched, &options)?,
    }
    skyward_root::write(&patched, by_name, partition, slot)?;
    // Read the partition again: what is on it has to be what was written, or
    // the phone does not start.
    let back = work.join("written.img");
    skyward_root::read(by_name, partition, slot, &back)?;
    let want = std::fs::metadata(&patched).with_context(|| format!("measure {}", patched.display()))?.len();
    let got = std::fs::metadata(&back).with_context(|| format!("measure {}", back.display()))?.len();
    ensure!(want == got, "{partition}{} reads back as {got} bytes, not the {want} written", slot.suffix());
    Ok(format!("{partition}{} — {}", slot.suffix(), kept.display()))
}

fn manager_from(source: &RootSource, work: &Path) -> Result<skyward_root::Manager> {
    let manager = match source {
        RootSource::Installed => skyward_root::detect().context("no root manager on the phone")?,
        RootSource::Apk(at) => {
            // What a workflow hands out is an archive of packages: the
            // package inside is the one to patch with.
            let inner = work.join("incoming");
            let apk = &skyward_root::apk::expand(Path::new(at), &inner)?;
            let into = work.join("apk");
            match skyward_root::apk::kind(apk)? {
                skyward_root::apk::Kind::Magisk => {
                    skyward_root::apk::unpack(apk, &into, skyward_root::apk::Kind::Magisk)?;
                    skyward_root::Manager::Magisk(skyward_root::Magisk { dir: into, version: None })
                }
                skyward_root::apk::Kind::KernelSu => {
                    skyward_root::apk::unpack(apk, &into, skyward_root::apk::Kind::KernelSu)?;
                    skyward_root::Manager::KernelSu(skyward_root::KernelSu { ksud: into.join("ksud"), version: None })
                }
                skyward_root::apk::Kind::Apatch => {
                    skyward_root::apk::unpack(apk, &into, skyward_root::apk::Kind::Apatch)?;
                    skyward_root::Manager::Apatch(skyward_root::Apatch { dir: into, version: None })
                }
            }
        }
    };
    Ok(manager)
}

fn keep_aside(stock: &Path, partition: &str, slot: skyward_vab::Slot, work: &Path) -> Result<std::path::PathBuf> {
    let at = Path::new("/sdcard/Download").join(format!("stock-{partition}{}.img", slot.suffix()));
    if std::fs::copy(stock, &at).is_ok() {
        return Ok(at);
    }
    let at = work.join(format!("stock-{partition}{}.img", slot.suffix()));
    std::fs::copy(stock, &at).with_context(|| format!("keep {}", stock.display()))?;
    Ok(at)
}

/// Download the package of the manager at `at` of the ones the page offers,
/// the way `proxy` names, and put the package itself where the page keeps
/// what it downloaded.
pub fn download_manager(servers: Servers, at: usize, proxy: usize) {
    let done = (|| -> Result<String> {
        let stop = AtomicBool::new(false);
        let plan = manager_plan(&servers, at, proxy, &stop)?;
        let dir = Path::new(skyward_root::releases::MANAGERS);
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        let into = dir.join(&plan.name);
        let agent = skyward_net::http::agent(servers.clone());
        skyward_net::download::fetch(&agent, &plan.url, &into, &skyward_net::download::Sum::Md5(String::new()), &stop, &mut |_| {})?;
        // A workflow hands its build out as an archive of packages: the one
        // for this phone is taken out of it, and kept where the archive was.
        let apk = skyward_root::apk::expand(&into, dir)?;
        if apk != into {
            std::fs::rename(&apk, &into)?;
        }
        Ok(into.display().to_string())
    })();
    match &done {
        Ok(said) => {
            let _ = skyward_sys::kmsg(&format!("root: {said}"));
        }
        Err(e) => {
            let _ = skyward_sys::kmsg(&format!("root: download failed: {e:#}"));
        }
    }
    if let Ok(mut held) = DOWNLOADED.lock() {
        *held = Some(done);
    }
}

fn manager_plan(servers: &Servers, at: usize, proxy: usize, stop: &AtomicBool) -> Result<ManagerPlan> {
    use skyward_root::releases::{self, Tier, Variant};
    let all = Variant::all();
    let (variant, tier) = *all.get(at).context("the list holds no such manager")?;
    let agent = skyward_net::http::agent(servers.clone());
    let work = Path::new("/tmp/root");
    std::fs::create_dir_all(work).with_context(|| format!("create {}", work.display()))?;
    let fetch = |url: &str, path: &Path| -> Result<()> {
        skyward_net::download::fetch(&agent, url, path, &skyward_net::download::Sum::Md5(String::new()), stop, &mut |_| {})
    };
    let mirror = releases::MIRRORS.get(proxy).map(|(_, url)| *url).unwrap_or("");
    match (tier, releases::has_releases(variant)) {
        (Tier::Stable, true) => {
            let listing = work.join("latest.json");
            fetch(&releases::through(mirror, &releases::latest(variant)), &listing)?;
            let json = std::fs::read_to_string(&listing).with_context(|| format!("read {}", listing.display()))?;
            let one = format!("[{json}]");
            let url = releases::apk_in(&one, Tier::Stable).context("the newest release carries no package")?;
            Ok(ManagerPlan { name: releases::file_name(&url), url: releases::through(mirror, &url) })
        }
        _ => Ok(ManagerPlan {
            name: "manager.apk".to_string(),
            url: match tier {
                Tier::Dev => releases::nightly_via(variant, proxy),
                Tier::Stable => releases::through(mirror, &releases::nightly(variant)),
            },
        }),
    }
}
