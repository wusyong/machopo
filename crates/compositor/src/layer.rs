use std::sync::Arc;

use base::AtomicSequenceNumber;
use compositor_paint::DisplayItemList;

use gfx::{Color, ImageData, PointF, RectF, SizeF};
use protocol::viz::SurfaceId;

use crate::{ClipNodeId, TransformNodeId};

/// `cc::Layer` id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayerId(pub u32);

impl LayerId {
    /// An id no other layer in the process has had (cc's
    /// `g_next_layer_id`): a layer of another document is never taken for
    /// one of this one's.
    pub fn next() -> Self {
        static IDS: AtomicSequenceNumber<u32> = AtomicSequenceNumber::<u32>::starting_at(1);
        Self(IDS.get_next())
    }
}

/// `cc::PictureLayer`: a layer whose content is a recorded display list.
///
/// Main-thread side. The display list is shared (`Arc`) with the impl side on
/// commit instead of copied, like `cc::RecordingSource` handing its
/// `DisplayItemList` to the `RasterSource`.
#[derive(Debug, Clone)]
pub struct PictureLayer {
    pub id: LayerId,
    /// Layer size in CSS pixels.
    pub bounds: SizeF,
    pub display_list: Arc<DisplayItemList>,
    /// When true, tiles are cleared to `background_color` before raster and
    /// blending can be skipped.
    pub contents_opaque: bool,
    pub background_color: Color,
    /// What this display list changed of the last one the layer had, in the
    /// layer's space, CSS px (cc's `PictureLayer` invalidation, from
    /// `SetNeedsDisplayRect`): only the tiles these touch are rastered
    /// again. `None`: all of it (`SetNeedsDisplay`), as for a new layer.
    /// Only read when the display list is a new one.
    pub invalidation: Option<Arc<[RectF]>>,
    /// Where the layer is: its nodes in the host's property trees.
    pub properties: LayerProperties,
}

/// `cc::TextureLayer`: a layer whose content is an image the embedder
/// draws itself, a canvas's bitmap (Blink's foreign layer of a composited
/// canvas). The impl side puts it on the GPU once per snapshot and draws it
/// as one texture quad, scaled into its bounds.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureLayer {
    pub id: LayerId,
    /// Layer size in CSS px: the canvas's content box. The snapshot is
    /// scaled into it, bilinearly (Blink's `kLow` filter quality).
    pub bounds: SizeF,
    /// What it shows: decoded pixels (`ImageData::decoded`). A new one
    /// (another id) is put on the GPU again; the same one isn't.
    pub snapshot: ImageData,
    /// An opaque canvas's (`alpha: false`, Blink's
    /// `SetContentsOpaque(is_opaque_)`): every pixel opaque.
    pub contents_opaque: bool,
    pub properties: LayerProperties,
}

/// A layer of the document, in paint order with the others (`cc::Layer`'s
/// kinds that paint makes).
#[derive(Debug, Clone)]
pub enum Layer {
    Picture(PictureLayer),
    Texture(TextureLayer),
}

impl Layer {
    pub fn id(&self) -> LayerId {
        match self {
            Self::Picture(layer) => layer.id,
            Self::Texture(layer) => layer.id,
        }
    }

    pub fn bounds(&self) -> SizeF {
        match self {
            Self::Picture(layer) => layer.bounds,
            Self::Texture(layer) => layer.bounds,
        }
    }

    pub fn properties(&self) -> LayerProperties {
        match self {
            Self::Picture(layer) => layer.properties,
            Self::Texture(layer) => layer.properties,
        }
    }

    /// The picture layer it is, if it is one.
    pub fn as_picture(&self) -> Option<&PictureLayer> {
        match self {
            Self::Picture(layer) => Some(layer),
            Self::Texture(_) => None,
        }
    }
}

impl From<PictureLayer> for Layer {
    fn from(layer: PictureLayer) -> Self {
        Self::Picture(layer)
    }
}

impl From<TextureLayer> for Layer {
    fn from(layer: TextureLayer) -> Self {
        Self::Texture(layer)
    }
}

/// A layer's place in the property trees (cc's transform and clip tree
/// indices on a `Layer`), and where in its transform node's space its
/// content's top left corner is. The default is the document's origin,
/// unclipped.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayerProperties {
    pub transform: TransformNodeId,
    pub clip: ClipNodeId,
    pub origin: PointF,
    /// Whether the layer is fixed to the viewport (`position: fixed`, RFC
    /// 0013's decision 16): it is drawn where it is however far the
    /// viewport is scrolled, so the impl side puts the scroll offset back
    /// into its origin. `cc` says the same with a transform node above the
    /// viewport's scroll.
    pub fixed: bool,
    /// Whether the layer is drawn over the surface layers, which are
    /// otherwise over every other layer: a browser's popup over its web
    /// views (Chromium's browser layer tree orders a popup's layer with
    /// the web contents' as siblings).
    pub over_surfaces: bool,
}

/// `cc::SurfaceLayer`: shows another client's surface, as the browser's
/// compositor shows the tab's. Draws as one `SurfaceDrawQuad`; Viz puts the
/// surface's own quads there.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceLayer {
    pub id: LayerId,
    /// Its top left corner in the viewport, CSS px. It doesn't move with
    /// the root layer's scroll offset.
    pub position: PointF,
    pub bounds: SizeF,
    pub surface_id: SurfaceId,
    /// Shown until the surface has a frame.
    pub background_color: Color,
}
