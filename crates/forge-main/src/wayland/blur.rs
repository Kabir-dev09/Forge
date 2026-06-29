use forge_core::config_registry::{BlurConfig, BlurMethod};
use forge_core::geometry::Size;
use wayland_client::protocol::{wl_compositor::WlCompositor, wl_surface::WlSurface};
use wayland_client::QueueHandle;
use wayland_protocols_plasma::blur::client::{
    org_kde_kwin_blur::OrgKdeKwinBlur, org_kde_kwin_blur_manager::OrgKdeKwinBlurManager,
};

use super::connection::WaylandState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlurBackendStatus {
    Disabled,
    Kde,
    External,
    Unsupported,
}

#[derive(Default)]
pub struct BlurState {
    kde_blur: Option<OrgKdeKwinBlur>,
    active_size: Option<Size>,
    active_method: Option<BlurMethod>,
}

impl BlurState {
    pub fn apply(
        &mut self,
        surface: &WlSurface,
        compositor: &WlCompositor,
        kde_manager: Option<&OrgKdeKwinBlurManager>,
        qh: &QueueHandle<WaylandState>,
        size: Size,
        config: &BlurConfig,
    ) -> BlurBackendStatus {
        if !config.enabled || config.method == BlurMethod::Off {
            self.disable(surface, kde_manager);
            return BlurBackendStatus::Disabled;
        }

        match config.method {
            BlurMethod::Auto | BlurMethod::Kde => {
                if let Some(manager) = kde_manager {
                    self.apply_kde(surface, compositor, manager, qh, size, config.method);
                    return BlurBackendStatus::Kde;
                }

                self.disable(surface, kde_manager);
                if config.method == BlurMethod::Kde {
                    BlurBackendStatus::Unsupported
                } else {
                    BlurBackendStatus::External
                }
            }
            BlurMethod::External => {
                self.disable(surface, kde_manager);
                BlurBackendStatus::External
            }
            BlurMethod::Off => BlurBackendStatus::Disabled,
        }
    }

    pub fn disable(&mut self, surface: &WlSurface, kde_manager: Option<&OrgKdeKwinBlurManager>) {
        if self.kde_blur.take().is_some() {
            if let Some(manager) = kde_manager {
                manager.unset(surface);
            }
            surface.commit();
        }
        self.active_size = None;
        self.active_method = None;
    }

    fn apply_kde(
        &mut self,
        surface: &WlSurface,
        compositor: &WlCompositor,
        manager: &OrgKdeKwinBlurManager,
        qh: &QueueHandle<WaylandState>,
        size: Size,
        method: BlurMethod,
    ) {
        if size.width == 0 || size.height == 0 {
            return;
        }

        let blur = self
            .kde_blur
            .get_or_insert_with(|| manager.create(surface, qh, ()));

        if self.active_size == Some(size) && self.active_method == Some(method) {
            return;
        }

        let region = compositor.create_region(qh, ());
        region.add(0, 0, size.width as i32, size.height as i32);
        blur.set_region(Some(&region));
        region.destroy();
        blur.commit();
        surface.commit();

        self.active_size = Some(size);
        self.active_method = Some(method);
    }
}
