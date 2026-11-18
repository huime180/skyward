//! The root pages' own vocabulary: where a manager's patcher comes from,
//! where the image a patch writes comes from, and which of the ways each was
//! taken by. The pages name them; the patch runs with them.

use crate::lang::Lang;

/// What the images taken out of a package are on show for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageMode {
    /// One of them is the image the patch writes.
    Patch,
    /// One of them is flashed on its own.
    Flash,
}

/// Which of a root page's files is being picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootPick {
    /// The manager a patch runs with.
    Manager,
    /// The image a patch writes.
    Image,
}

/// Where the patcher a root patch runs comes from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RootSource {
    /// The manager the phone has, in its folder on the data partition.
    #[default]
    Installed,
    /// An APK the user picked.
    Apk(String),
}

/// Where the image a patch writes comes from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RootImage {
    /// The image the partition holds now.
    #[default]
    Partition,
    /// An image file the user picked.
    File(String),
}

/// The way to a manager the page took, which its row shows a check for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ManagerWay {
    /// No way taken yet.
    #[default]
    None,
    /// The manager the phone has, under the data partition.
    LastUsed,
    /// An app whose package carries a manager's library.
    Installed,
    /// A package on the phone.
    Storage,
    /// A package downloaded from the manager's own repository.
    Download,
}

impl ManagerWay {
    /// What the page calls it, in the bar that starts the patch.
    pub fn name(self, l: Lang) -> &'static str {
        match self {
            ManagerWay::None => l.pick(["未选择", "未選擇", "none"]),
            ManagerWay::LastUsed => l.pick(["上次使用的管理器", "上次使用的管理器", "The manager used last time"]),
            ManagerWay::Installed => l.pick(["从已安装管理器中选择", "從已安裝管理器中選擇", "An installed manager"]),
            ManagerWay::Storage => l.pick(["从内部存储选择", "從內部儲存空間選擇", "From internal storage"]),
            ManagerWay::Download => l.pick(["网络下载", "網路下載", "Downloaded"]),
        }
    }
}

/// The way to an image the page took.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageWay {
    /// No way taken yet.
    #[default]
    None,
    /// The image the partition holds now, for the partition chosen.
    Direct,
    /// An image file the user picked.
    File,
    /// An image out of a system package.
    Package,
}

impl ImageWay {
    /// What the page calls it, in the bar that starts the patch.
    pub fn name(self, l: Lang) -> &'static str {
        match self {
            ImageWay::None => l.pick(["未选择", "未選擇", "none"]),
            ImageWay::Direct => l.pick(["本机镜像", "本機鏡像", "This phone's image"]),
            ImageWay::File => l.pick(["选择镜像文件进行修补", "選擇映像檔進行修補", "An image file"]),
            ImageWay::Package => l.pick(["从完整系统包解析", "從完整系統包解析", "A system package"]),
        }
    }
}

/// The partitions an image of the phone's own is written to, in the order a
/// page lists them: those the phone has.
pub fn root_partitions() -> Vec<&'static str> {
    let slot = skyward_vab::current_slot().ok().flatten();
    let by_name = std::path::Path::new(skyward_root::BY_NAME);
    ["boot", "init_boot"]
        .into_iter()
        .filter(|name| match slot {
            Some(slot) => by_name.join(format!("{name}{}", slot.suffix())).exists(),
            None => true,
        })
        .collect()
}

