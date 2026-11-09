# Draft: a DX12 surface can't be configured for a new device after its device is lost

<!-- A draft for gfx-rs/wgpu, written while doing RFC 0056 (2026-10-18). Not sent: for review first (CLAUDE.md: no issues upstream without review). -->

## Summary

On the DX12 backend, after the device a surface was configured with is lost (or destroyed), configuring the same `Surface` for a new device on the same adapter fails with a validation error, `Invalid surface`. wgpu-hal tries to resize the old swap chain, which belongs to the removed device. The same sequence works on Vulkan.

## Environment

- wgpu 30.0.1 (wgpu-core 30.0.1, wgpu-hal 30.0.1)
- Windows 11 (10.0.26200), DX12 backend
- AMD Radeon RX 7900 XTX (also seen with the device destroyed by `Device::destroy`, which is how we test a GPU reset)

## Steps

1. Create a `Surface` for a window, request an adapter compatible with it, request device A, and `surface.configure(&device_a, &config)`. Present a few frames.
2. Lose device A: `device_a.destroy()` (a driver reset does the same).
3. Request device B from the same adapter (or a new adapter for the same GPU).
4. `surface.configure(&device_b, &config)`.

## What happens

```text
wgpu_hal::dx12: ResizeBuffers failed: The GPU device instance has been suspended. Use GetDeviceRemovedReason ... (DXGI_ERROR_DEVICE_REMOVED; the message is localized, this is its English text)
wgpu_core::device::resource: surface configuration failed: ...
wgpu error: Validation Error

Caused by:
  In Surface::configure
    Invalid surface
```

(Before the configure, `Present` on the old swap chain also fails with `DXGI_ERROR_DEVICE_REMOVED`.)

## Why

- `wgpu-core/src/device/resource.rs` (around line 5365, `Device::configure_surface`): when the surface is configured again, it takes the old `presentation` (`surface.presentation.lock().take()`) but doesn't call the HAL's `unconfigure` for the device that configured it.
- `wgpu-hal/src/dx12/mod.rs` (around line 1503, `Surface::configure`): if the surface still has a swap chain (`self.swap_chain.write().take()`), it calls `ResizeBuffers` on it, whatever device it was made with. The swap chain was made on device A's queue, which has been removed, so `ResizeBuffers` fails.
- The HAL's `unconfigure` only runs when the `Surface` is dropped (`wgpu-core/src/instance.rs`, around line 725).

## What we expected

Configuring a surface for a different device than the one it was last configured for unconfigures it from the old one first (releasing a DX12 swap chain, so the new device can make its own), as dropping the surface does. Or, if moving a surface to another device isn't supported, that this is documented, and the error says so.

## Our workaround

When the device is lost, on DX12 only:

1. Let go of everything on the old device first (our renderer's context, so that nothing keeps the swap chain's buffers alive). If anything still holds them, creating the new swap chain fails with `E_ACCESSDENIED` (0x80070005): DXGI allows one swap chain per window.
2. Drop the old `Surface` (which unconfigures it and releases its swap chain), and create a new `Surface` for the window. With winit, the window handle is only available on the thread that created the window, so the new surface is made there and handed to the thread that renders.
3. Configure the new surface for the new device.

On Vulkan (and Metal) we keep the same surface: configuring it for the new device works.
