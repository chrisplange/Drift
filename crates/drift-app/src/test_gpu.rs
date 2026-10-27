//! The graphics adapter the UI tests render with, found once per test run.
//!
//! Without an adapter a UI test skips, and says so on the console once per run. With
//! `DRIFT_REQUIRE_GPU=1` (set in CI) it fails instead, so a runner without one can't pass the
//! UI and smoke tests by skipping them. `DRIFT_GPU_FALLBACK=1` limits the tests to software
//! adapters (WARP on Windows, lavapipe on Linux). The adapter used is printed on the console,
//! past the test harness's capture, so it shows in CI logs.
//!
//! The browser demo's test (`tests/demo.rs`) includes this file too.

use std::io::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use eframe::egui_wgpu::{self, wgpu};

/// UI tests skipped in this run for want of an adapter.
static SKIPPED: AtomicUsize = AtomicUsize::new(0);

fn env_on(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// What the UI tests render with: egui_kittest's own setup, which picks a software adapter
/// first; with `DRIFT_GPU_FALLBACK=1`, only a software one.
pub fn setup() -> egui_wgpu::WgpuSetup {
    let mut setup = egui_kittest::wgpu::default_wgpu_setup();
    if env_on("DRIFT_GPU_FALLBACK")
        && let egui_wgpu::WgpuSetup::CreateNew(new) = &mut setup
        && let Some(pick) = new.native_adapter_selector.clone()
    {
        new.native_adapter_selector = Some(Arc::new(move |adapters, surface| {
            let software: Vec<wgpu::Adapter> = adapters
                .iter()
                .filter(|a| a.get_info().device_type == wgpu::DeviceType::Cpu)
                .cloned()
                .collect();
            pick(&software, surface)
        }));
    }
    setup
}

/// The adapter [`setup`] picks on this machine, looked up once and printed.
fn adapter() -> Option<&'static wgpu::AdapterInfo> {
    static ADAPTER: OnceLock<Option<wgpu::AdapterInfo>> = OnceLock::new();
    ADAPTER
        .get_or_init(|| {
            let found = lookup();
            if let Some(info) = &found {
                let _ = writeln!(
                    std::io::stderr(),
                    "UI tests render on {} ({:?}, {:?})",
                    info.name,
                    info.backend,
                    info.device_type
                );
            }
            found
        })
        .as_ref()
}

fn lookup() -> Option<wgpu::AdapterInfo> {
    let setup = setup();
    let instance = pollster::block_on(setup.new_instance());
    let egui_wgpu::WgpuSetup::CreateNew(new) = setup else {
        return None;
    };
    let adapters =
        pollster::block_on(instance.enumerate_adapters(new.instance_descriptor.backends));
    let pick = new.native_adapter_selector?;
    pick(&adapters, None).ok().map(|a| a.get_info())
}

/// Whether a UI test can run here: there's an adapter. Without one it skips, said once per
/// run and counted (or fails, with `DRIFT_REQUIRE_GPU=1`).
pub fn available() -> bool {
    let found = gate(adapter, env_on("DRIFT_REQUIRE_GPU")).is_some();
    if !found {
        let n = SKIPPED.fetch_add(1, Ordering::Relaxed) + 1;
        if n == 1 {
            let _ = writeln!(
                std::io::stderr(),
                "No graphics adapter: UI tests are SKIPPED, not run (DRIFT_REQUIRE_GPU=1 fails \
                 them instead)"
            );
        }
        let test = std::thread::current().name().unwrap_or("?").to_owned();
        eprintln!("skipped UI test {test}: no graphics adapter ({n} skipped so far)");
    }
    found
}

/// The require-GPU rule, with the adapter lookup passed in: what it found, or a panic when
/// nothing was found and `require`.
fn gate<T>(lookup: impl FnOnce() -> Option<T>, require: bool) -> Option<T> {
    let found = lookup();
    assert!(
        found.is_some() || !require,
        "DRIFT_REQUIRE_GPU is set but no graphics adapter was found, so the UI tests can't \
         run. Install a software one (lavapipe on Linux; WARP comes with Windows) rather \
         than unsetting the variable."
    );
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "DRIFT_REQUIRE_GPU is set but no graphics adapter")]
    fn require_gpu_fails_without_an_adapter() {
        gate(|| None::<()>, true);
    }

    #[test]
    fn without_require_gpu_a_missing_adapter_skips() {
        assert_eq!(gate(|| None::<()>, false), None);
    }

    #[test]
    fn an_adapter_runs_the_test_either_way() {
        assert_eq!(gate(|| Some(1), true), Some(1));
        assert_eq!(gate(|| Some(1), false), Some(1));
    }
}
