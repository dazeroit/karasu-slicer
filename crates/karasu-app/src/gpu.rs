//! GPU backend choice and deterministic adapter selection (TD-067: modern backends with an
//! OpenGL fallback).
//!
//! **The fallback is loaded only when needed (TD-100).** The automatic choice first asks an
//! instance of the modern backends alone whether this machine has an adapter for them; OpenGL
//! joins the backends only when it has none. An OpenGL driver loaded beside Vulkan was seen to
//! crash at exit (the AMD driver `atio6axx.dll`, 0xC0000409), though nothing drew with it.
//!
//! **Transparency (TD-069):** whether the chosen surface composes alpha is recorded, never used to
//! choose the adapter. On Windows, DX12 through Direct Composition would compose alpha, but the
//! frame Windows draws for the undecorated-window shadow (always requested by eframe 0.36) shows
//! through the transparent corners, so the default swap chain is kept. The splash is opaque since
//! TD-141; the smoke test still reports the surface's ability.
use eframe::{egui_wgpu, wgpu};
use std::sync::{Arc, Mutex};

/// Which graphics backends the application may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendChoice {
    /// Vulkan, DX12 or Metal; OpenGL only when the machine has no adapter for them (TD-100).
    Auto,
    /// Vulkan only.
    Vulkan,
    /// DirectX 12 only.
    Dx12,
    /// Metal only.
    Metal,
    /// OpenGL only (the fallback, also for testing it).
    Gl,
}

impl BackendChoice {
    /// Parse a `--gpu-backend` value.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "auto" => Self::Auto,
            "vulkan" => Self::Vulkan,
            "dx12" => Self::Dx12,
            "metal" => Self::Metal,
            "gl" | "opengl" => Self::Gl,
            _ => return None,
        })
    }

    /// The wgpu backends this choice enumerates, `modern` telling whether the machine has an
    /// adapter for a modern backend: the automatic choice adds OpenGL only when it has none.
    #[must_use]
    pub fn backends(self, modern: bool) -> wgpu::Backends {
        match self {
            Self::Auto if modern => wgpu::Backends::PRIMARY,
            Self::Auto => wgpu::Backends::PRIMARY | wgpu::Backends::GL,
            Self::Vulkan => wgpu::Backends::VULKAN,
            Self::Dx12 => wgpu::Backends::DX12,
            Self::Metal => wgpu::Backends::METAL,
            Self::Gl => wgpu::Backends::GL,
        }
    }
}

/// What the selector needs to know about one adapter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// Backend of the adapter.
    pub backend: wgpu::Backend,
    /// Device class.
    pub device_type: wgpu::DeviceType,
    /// Whether it can present to the window.
    pub presents: bool,
}

/// Index of the adapter to use: among those that can present, modern backends before OpenGL,
/// then discrete, integrated, virtual, other and software devices; ties keep the enumeration
/// order. `None` if no adapter can present.
#[must_use]
pub fn select(candidates: &[Candidate]) -> Option<usize> {
    let device_rank = |t: wgpu::DeviceType| match t {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Other => 3,
        wgpu::DeviceType::Cpu => 4,
    };
    candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| c.presents)
        .min_by_key(|(index, c)| {
            (
                u8::from(c.backend == wgpu::Backend::Gl),
                device_rank(c.device_type),
                *index,
            )
        })
        .map(|(index, _)| index)
}

/// The adapter actually chosen, for the status bar and the smoke test.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selected {
    /// Backend name.
    pub backend: String,
    /// Adapter name.
    pub adapter: String,
    /// Device class.
    pub device_type: String,
    /// Driver name and information.
    pub driver: String,
    /// Whether the window surface can compose alpha (reported; the splash is opaque, TD-141).
    pub transparent: bool,
}

/// Whether this machine has an adapter for a modern backend (Vulkan, DX12 or Metal), asked of an
/// instance of those backends alone so that no OpenGL driver is loaded. Unknown (the
/// enumeration not ready at once, which native backends are) counts as none: OpenGL stays
/// available.
fn modern_adapter_exists() -> bool {
    use std::{future::Future, task};
    let started = std::time::Instant::now();
    let mut descriptor =
        egui_wgpu::WgpuSetupCreateNew::without_display_handle().instance_descriptor;
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let mut adapters = std::pin::pin!(instance.enumerate_adapters(wgpu::Backends::PRIMARY));
    let mut context = task::Context::from_waker(task::Waker::noop());
    let found = match adapters.as_mut().poll(&mut context) {
        task::Poll::Ready(adapters) => !adapters.is_empty(),
        task::Poll::Pending => false,
    };
    log::info!(
        target: "session",
        "modern graphics adapters {}, probed in {} ms",
        if found { "found" } else { "not found" },
        started.elapsed().as_millis()
    );
    found
}

/// The eframe wgpu configuration for `choice`, recording the chosen adapter into `chosen`.
#[must_use]
pub fn configuration(
    choice: BackendChoice,
    chosen: Arc<Mutex<Option<Selected>>>,
) -> egui_wgpu::WgpuConfiguration {
    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    let modern = choice == BackendChoice::Auto && modern_adapter_exists();
    let backends = choice.backends(modern);
    setup.instance_descriptor.backends = backends;
    setup.native_adapter_selector = Some(Arc::new(move |adapters, surface| {
        let candidates: Vec<Candidate> = adapters
            .iter()
            .map(|adapter| {
                let info = adapter.get_info();
                Candidate {
                    backend: info.backend,
                    device_type: info.device_type,
                    presents: surface.is_none_or(|s| adapter.is_surface_supported(s)),
                }
            })
            .collect();
        let index = select(&candidates).ok_or_else(|| {
            // Without OpenGL among the backends, it can still be asked for.
            if backends.contains(wgpu::Backends::GL) {
                "no graphics adapter can present to the window".to_owned()
            } else {
                "no graphics adapter can present to the window; try --gpu-backend gl".to_owned()
            }
        })?;
        let adapter = adapters[index].clone();
        let info = adapter.get_info();
        let transparent = surface.is_some_and(|s| composes_alpha(s, &adapter));
        log::info!(
            target: "session",
            "graphics adapter {} ({:?}, {:?}), driver {} {}",
            info.name,
            info.device_type,
            info.backend,
            info.driver,
            info.driver_info
        );
        if let Ok(mut slot) = chosen.lock() {
            *slot = Some(Selected {
                backend: format!("{:?}", info.backend),
                adapter: info.name.clone(),
                device_type: format!("{:?}", info.device_type),
                driver: format!("{} {}", info.driver, info.driver_info)
                    .trim()
                    .to_owned(),
                transparent,
            });
        }
        Ok(adapter)
    }));
    egui_wgpu::WgpuConfiguration {
        wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(setup),
        ..egui_wgpu::WgpuConfiguration::default()
    }
}

/// Whether `adapter` can present to `surface` with alpha composition.
fn composes_alpha(surface: &wgpu::Surface<'_>, adapter: &wgpu::Adapter) -> bool {
    let modes = surface.get_capabilities(adapter).alpha_modes;
    modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        || modes.contains(&wgpu::CompositeAlphaMode::PostMultiplied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::{Backend, DeviceType};

    fn candidate(backend: Backend, device_type: DeviceType, presents: bool) -> Candidate {
        Candidate {
            backend,
            device_type,
            presents,
        }
    }

    #[test]
    fn modern_backends_and_real_gpus_come_first_and_gl_is_the_fallback() {
        let list = [
            candidate(Backend::Gl, DeviceType::DiscreteGpu, true),
            candidate(Backend::Vulkan, DeviceType::Cpu, true),
            candidate(Backend::Vulkan, DeviceType::IntegratedGpu, true),
            candidate(Backend::Vulkan, DeviceType::DiscreteGpu, false),
        ];
        // The discrete Vulkan GPU cannot present; the integrated one wins over software.
        assert_eq!(select(&list), Some(2));
        // Without a presenting modern adapter, OpenGL is used.
        assert_eq!(select(&[list[0].clone(), list[3].clone()]), Some(0));
        assert_eq!(select(&[list[3].clone()]), None);
        assert_eq!(select(&[]), None);
        // Ties keep the enumeration order.
        let twins = [
            candidate(Backend::Dx12, DeviceType::DiscreteGpu, true),
            candidate(Backend::Vulkan, DeviceType::DiscreteGpu, true),
        ];
        assert_eq!(select(&twins), Some(0));
    }

    #[test]
    fn backend_choices_parse_and_map() {
        assert_eq!(BackendChoice::parse("gl"), Some(BackendChoice::Gl));
        assert_eq!(BackendChoice::parse("opengl"), Some(BackendChoice::Gl));
        assert_eq!(BackendChoice::parse("vulkan"), Some(BackendChoice::Vulkan));
        assert_eq!(BackendChoice::parse("webgpu"), None);
        // OpenGL joins the automatic choice only without a modern adapter (TD-100).
        assert_eq!(BackendChoice::Auto.backends(true), wgpu::Backends::PRIMARY);
        assert!(BackendChoice::Auto
            .backends(false)
            .contains(wgpu::Backends::GL | wgpu::Backends::VULKAN));
        assert_eq!(BackendChoice::Gl.backends(true), wgpu::Backends::GL);
        assert_eq!(
            BackendChoice::Vulkan.backends(false),
            wgpu::Backends::VULKAN
        );
    }
}
