//! `viz::SurfaceAggregator`: flattens the surface tree into one frame.
//!
//! The root surface (the browser's own frame in a window, the tab's in
//! headless mode) embeds others with `SurfaceDrawQuad`s. Aggregation copies
//! the root frame's quads and puts each embedded surface's quads where its
//! `SurfaceDrawQuad` was: moved to the quad's origin, clipped to it, with
//! the surface's background behind them. Resource ids are only unique per
//! client, so each one is renumbered (`DisplayResourceProvider`'s child to
//! parent mapping), the same way for as long as the client uses it, so a
//! renderer can keep its upload.

use std::collections::{HashMap, HashSet};

use base::SequenceNumber;
use gfx::{Color, PointF, RRectF, RectF, Size};
use protocol::viz::{
    CompositorFrame, CompositorRenderPass, DrawQuad, FrameSinkId, LocalSurfaceId, Material,
    RenderPassId, ResourceId, SharedQuadState, TransferableResource,
};

/// Latest frame per frame sink (`viz::SurfaceManager`, much simplified).
#[derive(Default)]
pub struct SurfaceManager {
    surfaces: HashMap<FrameSinkId, (LocalSurfaceId, CompositorFrame)>,
}

impl SurfaceManager {
    /// Stores `frame` unless a frame for a newer `LocalSurfaceId` is already
    /// held. Returns whether the frame was accepted.
    pub fn submit(
        &mut self,
        sink: FrameSinkId,
        local_surface_id: LocalSurfaceId,
        frame: CompositorFrame,
    ) -> bool {
        if let Some((current, _)) = self.surfaces.get(&sink)
            && local_surface_id < *current
        {
            return false;
        }
        self.surfaces.insert(sink, (local_surface_id, frame));
        true
    }

    pub fn latest(&self, sink: FrameSinkId) -> Option<&CompositorFrame> {
        self.surfaces.get(&sink).map(|(_, f)| f)
    }

    pub fn remove(&mut self, sink: FrameSinkId) {
        self.surfaces.remove(&sink);
    }
}

/// `viz::AggregatedFrame`: what a `DirectRenderer` draws.
pub struct AggregatedFrame<'a> {
    /// Root last, as in `CompositorFrame`. One pass for now: no client
    /// makes more.
    pub render_pass_list: Vec<CompositorRenderPass>,
    /// By the aggregated ids the quads use.
    pub resources: HashMap<ResourceId, &'a TransferableResource>,
    pub background_color: Color,
    /// Size the root client produced the frame for, physical pixels.
    pub size_in_pixels: Size,
    /// Every frame sink a `SurfaceDrawQuad` pointed at, with a frame or not:
    /// a new frame from one of them needs a new draw.
    pub embedded: HashSet<FrameSinkId>,
}

/// The aggregator's state across frames: how resource ids are renumbered.
pub struct SurfaceAggregator {
    resource_ids: HashMap<(FrameSinkId, ResourceId), ResourceId>,
    /// From 1, never handed out twice: the renderer keeps what it uploaded
    /// under these.
    aggregated_ids: SequenceNumber<u64>,
}

impl Default for SurfaceAggregator {
    fn default() -> Self {
        Self {
            resource_ids: HashMap::new(),
            aggregated_ids: SequenceNumber::starting_at(1),
        }
    }
}

/// What one aggregation builds.
struct Output<'a> {
    quads: Vec<DrawQuad>,
    shared_quad_states: Vec<SharedQuadState>,
    resources: HashMap<ResourceId, &'a TransferableResource>,
    embedded: HashSet<FrameSinkId>,
    /// The ids used this time; the rest are dropped after.
    used_ids: HashMap<(FrameSinkId, ResourceId), ResourceId>,
}

impl SurfaceAggregator {
    /// `SurfaceAggregator::Aggregate`: the root sink's latest frame, with
    /// the surfaces it embeds (and theirs) put in. `None` until the root
    /// has a frame.
    pub fn aggregate<'a>(
        &mut self,
        manager: &'a SurfaceManager,
        root: FrameSinkId,
    ) -> Option<AggregatedFrame<'a>> {
        let frame = manager.latest(root)?;
        let root_pass = frame.root_pass()?;
        let mut out = Output {
            quads: Vec::new(),
            shared_quad_states: Vec::new(),
            resources: HashMap::new(),
            embedded: HashSet::new(),
            used_ids: HashMap::new(),
        };
        let mut path = vec![root];
        self.copy_frame(
            manager,
            root,
            frame,
            PointF::new(0.0, 0.0),
            None,
            None,
            1.0,
            &mut path,
            &mut out,
        );
        // Ids nothing uses any more are forgotten, as a renderer drops
        // their uploads.
        self.resource_ids = out.used_ids;
        Some(AggregatedFrame {
            render_pass_list: vec![CompositorRenderPass {
                id: RenderPassId(1),
                output_rect: root_pass.output_rect,
                damage_rect: root_pass.output_rect,
                shared_quad_state_list: out.shared_quad_states,
                quad_list: out.quads,
            }],
            resources: out.resources,
            background_color: frame.metadata.root_background_color,
            size_in_pixels: frame.size_in_pixels(),
            embedded: out.embedded,
        })
    }

    /// Appends `frame`'s root pass, from `sink`, moved by `offset`, clipped
    /// to `clip` and faded by `opacity` (in the aggregated frame's space).
    /// `path`: the sinks being copied, to stop a surface embedding itself.
    #[allow(clippy::too_many_arguments)]
    fn copy_frame<'a>(
        &mut self,
        manager: &'a SurfaceManager,
        sink: FrameSinkId,
        frame: &'a CompositorFrame,
        offset: PointF,
        clip: Option<RectF>,
        rounded: Option<RRectF>,
        opacity: f32,
        path: &mut Vec<FrameSinkId>,
        out: &mut Output<'a>,
    ) {
        let Some(pass) = frame.root_pass() else {
            return;
        };
        let resources: HashMap<ResourceId, &'a TransferableResource> =
            frame.resource_list.iter().map(|r| (r.id, r)).collect();
        let moved = |r: RectF| RectF::new(r.x + offset.x, r.y + offset.y, r.width, r.height);
        // This pass's shared quad states, as added to the output.
        let mut states: HashMap<usize, usize> = HashMap::new();
        for quad in &pass.quad_list {
            let state = pass
                .shared_quad_state_list
                .get(quad.shared_quad_state)
                .copied()
                .unwrap_or_default();
            let quad_clip = intersect(clip, state.clip_rect.map(moved));
            // One rounded clip per quad: its own, else the embedder's.
            let quad_rounded = state
                .rounded_clip
                .map(|r| r.offset(offset.x, offset.y))
                .or(rounded);
            let quad_opacity = opacity * state.opacity;
            let rect = moved(quad.rect);
            match quad.material {
                Material::Surface {
                    surface_id,
                    default_background_color,
                } => {
                    let child_sink = surface_id.frame_sink_id;
                    out.embedded.insert(child_sink);
                    let surface_clip = intersect(quad_clip, Some(rect));
                    let child = manager
                        .latest(child_sink)
                        .filter(|_| !path.contains(&child_sink));
                    // Without a frame (or embedding itself), the default
                    // background.
                    let background = child.map_or(default_background_color, |child| {
                        child.metadata.root_background_color
                    });
                    if let Some(child) = child {
                        path.push(child_sink);
                        self.copy_frame(
                            manager,
                            child_sink,
                            child,
                            PointF::new(rect.x, rect.y),
                            surface_clip,
                            quad_rounded,
                            quad_opacity,
                            path,
                            out,
                        );
                        path.pop();
                    }
                    // Behind the surface's quads (the list is front to back).
                    let index = push_state(out, surface_clip, quad_rounded, quad_opacity);
                    out.quads.push(DrawQuad {
                        rect,
                        shared_quad_state: index,
                        material: Material::SolidColor { color: background },
                    });
                }
                Material::Texture {
                    resource_id,
                    tex_coord_rect,
                    nearest_neighbor,
                } => {
                    let Some(&resource) = resources.get(&resource_id) else {
                        tracing::warn!(?sink, ?resource_id, "quad references a missing resource");
                        continue;
                    };
                    let id = self.aggregated_id(sink, resource_id, out);
                    out.resources.insert(id, resource);
                    let index = *states
                        .entry(quad.shared_quad_state)
                        .or_insert_with(|| push_state(out, quad_clip, quad_rounded, quad_opacity));
                    out.quads.push(DrawQuad {
                        rect,
                        shared_quad_state: index,
                        material: Material::Texture {
                            resource_id: id,
                            tex_coord_rect,
                            nearest_neighbor,
                        },
                    });
                }
                Material::SolidColor { color } => {
                    let index = *states
                        .entry(quad.shared_quad_state)
                        .or_insert_with(|| push_state(out, quad_clip, quad_rounded, quad_opacity));
                    out.quads.push(DrawQuad {
                        rect,
                        shared_quad_state: index,
                        material: Material::SolidColor { color },
                    });
                }
            }
        }
    }

    /// The aggregated id for `sink`'s `id`: the one it had last time, or a
    /// new one.
    fn aggregated_id(
        &mut self,
        sink: FrameSinkId,
        id: ResourceId,
        out: &mut Output<'_>,
    ) -> ResourceId {
        let key = (sink, id);
        let aggregated = match self.resource_ids.get(&key) {
            Some(&aggregated) => aggregated,
            None => ResourceId(self.aggregated_ids.get_next()),
        };
        out.used_ids.insert(key, aggregated);
        aggregated
    }
}

fn push_state(
    out: &mut Output<'_>,
    clip_rect: Option<RectF>,
    rounded_clip: Option<RRectF>,
    opacity: f32,
) -> usize {
    let index = out.shared_quad_states.len();
    out.shared_quad_states.push(SharedQuadState {
        clip_rect,
        rounded_clip,
        opacity,
    });
    index
}

fn intersect(a: Option<RectF>, b: Option<RectF>) -> Option<RectF> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.intersect(&b)),
        (a, None) => a,
        (None, b) => b,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use protocol::viz::{CompositorFrameMetadata, ResourceFormat, SurfaceId};

    use super::*;

    const BROWSER: FrameSinkId = FrameSinkId {
        client_id: 0,
        sink_id: 1,
    };
    const TAB: FrameSinkId = FrameSinkId {
        client_id: 1,
        sink_id: 1,
    };
    const RED: Color = Color::rgba(255, 0, 0, 255);
    const GREEN: Color = Color::rgba(0, 128, 0, 255);

    fn resource(id: u64) -> TransferableResource {
        TransferableResource {
            id: ResourceId(id),
            size: Size::new(4, 4),
            format: ResourceFormat::Rgba8Premul,
            pixels: Arc::from(vec![0; 64]),
            shared_image: None,
        }
    }

    fn texture(id: u64, rect: RectF) -> DrawQuad {
        DrawQuad {
            rect,
            shared_quad_state: 0,
            material: Material::Texture {
                resource_id: ResourceId(id),
                tex_coord_rect: RectF::new(0.0, 0.0, 4.0, 4.0),
                nearest_neighbor: true,
            },
        }
    }

    fn bilinear(mut quad: DrawQuad) -> DrawQuad {
        if let Material::Texture {
            nearest_neighbor, ..
        } = &mut quad.material
        {
            *nearest_neighbor = false;
        }
        quad
    }

    fn solid(color: Color, rect: RectF) -> DrawQuad {
        DrawQuad {
            rect,
            shared_quad_state: 0,
            material: Material::SolidColor { color },
        }
    }

    fn surface(sink: FrameSinkId, rect: RectF) -> DrawQuad {
        DrawQuad {
            rect,
            shared_quad_state: 0,
            material: Material::Surface {
                surface_id: SurfaceId {
                    frame_sink_id: sink,
                    local_surface_id: LocalSurfaceId::default(),
                },
                default_background_color: Color::WHITE,
            },
        }
    }

    fn frame(
        quads: Vec<DrawQuad>,
        resources: Vec<TransferableResource>,
        background: Color,
    ) -> CompositorFrame {
        CompositorFrame {
            metadata: CompositorFrameMetadata {
                device_scale_factor: 1.0,
                frame_token: 1,
                root_background_color: background,
            },
            resource_list: resources,
            render_pass_list: vec![CompositorRenderPass {
                id: RenderPassId(1),
                output_rect: RectF::new(0.0, 0.0, 8.0, 8.0),
                damage_rect: RectF::new(0.0, 0.0, 8.0, 8.0),
                shared_quad_state_list: vec![SharedQuadState::default()],
                quad_list: quads,
            }],
        }
    }

    fn submit(manager: &mut SurfaceManager, sink: FrameSinkId, frame: CompositorFrame) {
        manager.submit(sink, LocalSurfaceId::default(), frame);
    }

    /// The aggregated quads: rect, clip, and what they draw.
    fn quads(frame: &AggregatedFrame<'_>) -> Vec<(RectF, Option<RectF>, Material)> {
        let pass = &frame.render_pass_list[0];
        pass.quad_list
            .iter()
            .map(|q| {
                let state = pass.shared_quad_state_list[q.shared_quad_state];
                (q.rect, state.clip_rect, q.material)
            })
            .collect()
    }

    fn texture_id(material: Material) -> ResourceId {
        match material {
            Material::Texture { resource_id, .. } => resource_id,
            other => panic!("not a texture: {other:?}"),
        }
    }

    #[test]
    fn an_embedded_surface_goes_where_its_quad_is() {
        let mut manager = SurfaceManager::default();
        // The browser: a toolbar on top, the tab under it. Both use
        // resource id 1.
        submit(
            &mut manager,
            BROWSER,
            frame(
                vec![
                    texture(1, RectF::new(0.0, 0.0, 8.0, 2.0)),
                    surface(TAB, RectF::new(0.0, 2.0, 8.0, 6.0)),
                ],
                vec![resource(1)],
                Color::WHITE,
            ),
        );
        // The tab's frame is taller than the room it gets.
        submit(
            &mut manager,
            TAB,
            frame(
                vec![
                    solid(RED, RectF::new(1.0, 1.0, 2.0, 2.0)),
                    // Sampled bilinearly (a canvas's layer).
                    bilinear(texture(1, RectF::new(0.0, 0.0, 8.0, 8.0))),
                ],
                vec![resource(1)],
                GREEN,
            ),
        );
        let mut aggregator = SurfaceAggregator::default();
        let frame = aggregator.aggregate(&manager, BROWSER).unwrap();
        let q = quads(&frame);
        assert_eq!(q.len(), 4);
        // The toolbar first (front), then the tab's quads moved down by 2
        // and clipped to its quad, then the tab's background behind them.
        let tab_rect = Some(RectF::new(0.0, 2.0, 8.0, 6.0));
        assert_eq!((q[0].0, q[0].1), (RectF::new(0.0, 0.0, 8.0, 2.0), None));
        assert_eq!(
            q[1],
            (
                RectF::new(1.0, 3.0, 2.0, 2.0),
                tab_rect,
                Material::SolidColor { color: RED }
            )
        );
        assert_eq!((q[2].0, q[2].1), (RectF::new(0.0, 2.0, 8.0, 8.0), tab_rect));
        assert_eq!(
            q[3],
            (
                RectF::new(0.0, 2.0, 8.0, 6.0),
                tab_rect,
                Material::SolidColor { color: GREEN }
            )
        );
        // The two clients' resource 1 are two resources now.
        let (browser_id, tab_id) = (texture_id(q[0].2), texture_id(q[2].2));
        assert_ne!(browser_id, tab_id);
        assert!(std::ptr::eq(
            frame.resources[&browser_id],
            &manager.latest(BROWSER).unwrap().resource_list[0]
        ));
        assert!(std::ptr::eq(
            frame.resources[&tab_id],
            &manager.latest(TAB).unwrap().resource_list[0]
        ));
        assert_eq!(frame.embedded, HashSet::from([TAB]));
        // Each sampled as its client asked.
        let nearest = |material| matches!(material, Material::Texture { nearest_neighbor, .. } if nearest_neighbor);
        assert!(nearest(q[0].2));
        assert!(!nearest(q[2].2));
    }

    #[test]
    fn an_embedded_surfaces_quads_keep_one_rounded_clip() {
        use gfx::{RRectF, SizeF};
        let mut manager = SurfaceManager::default();
        // The browser's tab quad is cut to rounded corners.
        let tab_corners = RRectF::uniform(RectF::new(0.0, 2.0, 8.0, 6.0), 2.0);
        let mut browser = frame(
            vec![surface(TAB, RectF::new(0.0, 2.0, 8.0, 6.0))],
            vec![],
            Color::WHITE,
        );
        browser.render_pass_list[0].shared_quad_state_list[0].rounded_clip = Some(tab_corners);
        submit(&mut manager, BROWSER, browser);
        // In the tab, one quad has rounded corners of its own (a rounded
        // scroller), the other none.
        let own = RRectF::new(RectF::new(1.0, 1.0, 4.0, 4.0), [SizeF::new(1.0, 1.0); 4]);
        let mut tab = frame(
            vec![
                DrawQuad {
                    shared_quad_state: 1,
                    ..solid(RED, RectF::new(1.0, 1.0, 4.0, 4.0))
                },
                solid(GREEN, RectF::new(0.0, 0.0, 8.0, 8.0)),
            ],
            vec![],
            GREEN,
        );
        tab.render_pass_list[0]
            .shared_quad_state_list
            .push(SharedQuadState {
                rounded_clip: Some(own),
                ..SharedQuadState::default()
            });
        submit(&mut manager, TAB, tab);
        let mut aggregator = SurfaceAggregator::default();
        let frame = aggregator.aggregate(&manager, BROWSER).unwrap();
        let pass = &frame.render_pass_list[0];
        let rounded: Vec<Option<RRectF>> = pass
            .quad_list
            .iter()
            .map(|q| pass.shared_quad_state_list[q.shared_quad_state].rounded_clip)
            .collect();
        // Its own, moved down with the tab; the tab's for the others and
        // the tab's background.
        assert_eq!(
            rounded,
            [
                Some(own.offset(0.0, 2.0)),
                Some(tab_corners),
                Some(tab_corners)
            ]
        );
    }

    #[test]
    fn a_surface_without_a_frame_shows_its_default_background() {
        let mut manager = SurfaceManager::default();
        submit(
            &mut manager,
            BROWSER,
            frame(
                vec![surface(TAB, RectF::new(0.0, 2.0, 8.0, 6.0))],
                vec![],
                RED,
            ),
        );
        let mut aggregator = SurfaceAggregator::default();
        let frame = aggregator.aggregate(&manager, BROWSER).unwrap();
        assert_eq!(
            quads(&frame),
            [(
                RectF::new(0.0, 2.0, 8.0, 6.0),
                Some(RectF::new(0.0, 2.0, 8.0, 6.0)),
                Material::SolidColor {
                    color: Color::WHITE
                }
            )]
        );
        // Still embedded: its first frame needs a draw.
        assert!(frame.embedded.contains(&TAB));
    }

    #[test]
    fn aggregated_ids_last_as_long_as_the_resource_is_used() {
        let mut manager = SurfaceManager::default();
        let tab_frame = |ids: &[u64]| {
            frame(
                ids.iter()
                    .map(|&id| texture(id, RectF::new(0.0, 0.0, 4.0, 4.0)))
                    .collect(),
                ids.iter().map(|&id| resource(id)).collect(),
                GREEN,
            )
        };
        submit(
            &mut manager,
            BROWSER,
            frame(
                vec![surface(TAB, RectF::new(0.0, 0.0, 8.0, 8.0))],
                vec![],
                Color::WHITE,
            ),
        );
        let mut aggregator = SurfaceAggregator::default();
        let ids = |aggregator: &mut SurfaceAggregator, manager: &SurfaceManager| {
            let frame = aggregator.aggregate(manager, BROWSER).unwrap();
            quads(&frame)
                .iter()
                .filter(|q| matches!(q.2, Material::Texture { .. }))
                .map(|q| texture_id(q.2))
                .collect::<Vec<_>>()
        };
        submit(&mut manager, TAB, tab_frame(&[5, 6]));
        let first = ids(&mut aggregator, &manager);
        // 6 stays, 5 goes, 7 is new.
        submit(&mut manager, TAB, tab_frame(&[6, 7]));
        let second = ids(&mut aggregator, &manager);
        assert_eq!(second[0], first[1]);
        assert!(!first.contains(&second[1]));
        // 5 went, so it gets a new id if it comes back.
        submit(&mut manager, TAB, tab_frame(&[5]));
        let third = ids(&mut aggregator, &manager);
        assert!(!first.contains(&third[0]) && !second.contains(&third[0]));
    }

    #[test]
    fn a_surface_embedding_itself_stops() {
        let mut manager = SurfaceManager::default();
        submit(
            &mut manager,
            TAB,
            frame(
                vec![surface(TAB, RectF::new(0.0, 0.0, 8.0, 8.0))],
                vec![],
                GREEN,
            ),
        );
        let mut aggregator = SurfaceAggregator::default();
        let frame = aggregator.aggregate(&manager, TAB).unwrap();
        // Only the default background where it would go again.
        assert_eq!(quads(&frame).len(), 1);
    }
}
