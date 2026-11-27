//! How much of the GPU's memory a renderer's tiles may take, and Viz's
//! limits on every client's shared images: the browser's to say, as it
//! knows the screen and the machine and the renderer (in its sandbox)
//! doesn't.

use gfx::Size;
use protocol::viz::{DEFAULT_TILE_MEMORY_LIMIT, SHARED_IMAGES_PER_CLIENT, SharedImageLimits};

/// The screen Chromium measured 1152 MB on (2056 x 1329 at a scale of 2),
/// in pixels: the budget grows with a screen's pixels at that rate.
const LARGE_RESOLUTION: u64 = 2056 * 1329 * 2 * 2;
const LARGE_RESOLUTION_MB: u64 = 1152;
const MIB: u64 = 1 << 20;

/// The tile memory a renderer's compositor may keep (Blink's
/// `GetGpuMemoryPolicy` on desktops): 1152 MB for a 2056 x 1329 screen at
/// a scale of 2, as many more or fewer as the `screen`'s physical pixels
/// are, but no more than a quarter of the machine's `physical_memory`
/// (when it is known), and else no less than 512 MB.
pub fn tile_memory_limit(screen: Size, physical_memory: Option<u64>) -> u64 {
    let pixels = u64::from(screen.width).saturating_mul(u64::from(screen.height));
    let mb = LARGE_RESOLUTION_MB
        .saturating_mul(pixels)
        .checked_div(LARGE_RESOLUTION)
        .unwrap_or(0);
    let least = DEFAULT_TILE_MEMORY_LIMIT / MIB;
    // As Chromium: the machine's quarter first, and only then the least
    // (a machine of less than 2 GB is given its quarter).
    let mb = match physical_memory.map(|bytes| bytes / 4 / MIB) {
        Some(quarter) if mb > quarter => quarter,
        _ => mb.max(least),
    };
    mb.saturating_mul(MIB)
}

/// Viz's limits on clients' shared images, for renderers told
/// `tile_memory_limit`: twice it a client (the browser's own UI and a
/// renderer's tiles that go as others come), [`SHARED_IMAGES_PER_CLIENT`]
/// images a client, and half the machine's `physical_memory` together
/// (none when it isn't known).
pub fn shared_image_limits(
    tile_memory_limit: u64,
    physical_memory: Option<u64>,
) -> SharedImageLimits {
    SharedImageLimits {
        bytes_per_client: tile_memory_limit.saturating_mul(2),
        images_per_client: SHARED_IMAGES_PER_CLIENT,
        bytes: physical_memory.map_or(u64::MAX, |bytes| bytes / 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    /// 1152 MB for 2056 x 1329 at 2 (the screen Chromium measured on), as
    /// many more or fewer as there are pixels: 874 for 1920 x 1080 at 2.
    #[test]
    fn the_budget_grows_with_the_screen() {
        let memory = Some(64 * GIB);
        assert_eq!(tile_memory_limit(Size::new(3840, 2160), memory), 874 * MIB);
        assert_eq!(tile_memory_limit(Size::new(4112, 2658), memory), 1152 * MIB);
        // Twice the pixels, twice the memory.
        assert_eq!(tile_memory_limit(Size::new(8224, 2658), memory), 2304 * MIB);
        // A small screen, or none, gets the least.
        assert_eq!(tile_memory_limit(Size::new(800, 600), memory), 512 * MIB);
        assert_eq!(tile_memory_limit(Size::new(0, 0), memory), 512 * MIB);
    }

    /// No more than a quarter of the machine's memory, even below the
    /// least (Chromium's order: the quarter first); with none known, no cap.
    #[test]
    fn the_budget_is_a_quarter_of_the_machine_at_most() {
        let huge = Size::new(16384, 16384);
        assert_eq!(tile_memory_limit(huge, Some(8 * GIB)), 2 * GIB);
        assert_eq!(
            tile_memory_limit(Size::new(8224, 2658), Some(8 * GIB)),
            2 * GIB
        );
        assert_eq!(tile_memory_limit(Size::new(8224, 2658), Some(4 * GIB)), GIB);
        assert_eq!(
            tile_memory_limit(Size::new(8224, 2658), Some(GIB)),
            256 * MIB
        );
        // A small screen on a small machine: the least, though more than
        // the quarter (Chromium caps only what is past the quarter).
        assert_eq!(tile_memory_limit(Size::new(800, 600), Some(GIB)), 512 * MIB);
        // With no memory known, nothing caps it.
        assert_eq!(tile_memory_limit(huge, None), 28_293 * MIB);
        assert_eq!(
            tile_memory_limit(Size::new(u32::MAX, u32::MAX), None) % MIB,
            0
        );
    }

    #[test]
    fn vizs_limits_follow_the_budget_and_the_machine() {
        let limits = shared_image_limits(512 * MIB, Some(64 * GIB));
        assert_eq!(
            limits,
            SharedImageLimits {
                bytes_per_client: GIB,
                images_per_client: SHARED_IMAGES_PER_CLIENT,
                bytes: 32 * GIB,
            }
        );
        assert_eq!(shared_image_limits(512 * MIB, None).bytes, u64::MAX);
        assert_eq!(
            shared_image_limits(u64::MAX, None).bytes_per_client,
            u64::MAX
        );
    }
}
