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

    /// The managers the phone has, as the pages that list them name them.
    pub fn root_apps(&mut self, apps: Vec<skyward_root::installed::App>) {
        self.state.root_apps = Some(apps);
        self.refresh();
    }


    /// The package a page chose becomes the one the next page patches with.
    pub fn root_package_chosen(&mut self, path: String) {
        self.root_manager_chosen(ManagerWay::Download, RootSource::Apk(path));
    }

    /// What the last patch came to.
    pub fn root_ended(&mut self, said: String) {
        self.state.root_said = Some(said);
        self.refresh();
    }

    /// An image chosen becomes the one a patch writes, and the page it was
    /// chosen on gives way to the root page.
    pub(crate) fn root_image_chosen(&mut self, way: ImageWay, image: RootImage) {
        self.state.root_image_way = way;
        self.state.root_image = image;
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

}

impl App {
    /// Apply device capabilities before the first frame is drawn.
    pub fn configure_device(&mut self, cfg: &skyward_config::DeviceConfig) {
        self.state.tablet = cfg.device.tablet;
        self.resize(self.full_width, self.h);
        self.state.supports_vibration = cfg.device.supports_vibration.unwrap_or(cfg.haptics.is_some());
        self.state.tablet = cfg.device.tablet;
        self.state.sensors = cfg.orientation.is_some();
        self.refresh();
    }
}

impl App {
    /// Read the modules the phone's root manager has, and keep them for the
    /// page that lists them.
    pub(crate) fn read_modules(&mut self) {
        let dir = std::path::Path::new(skyward_root::modules::MODULES);
        self.state.modules = skyward_root::modules::list(dir).ok();
    }
}

