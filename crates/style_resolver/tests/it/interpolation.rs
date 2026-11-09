//! Each longhand's animation type.

use css::interpolate::AnimationType;
use css::properties::LonghandId;

fn id(name: &str) -> LonghandId {
    LonghandId::from_name(name).expect("a longhand")
}

#[test]
fn animation_types() {
    let types = [
        ("display", AnimationType::NotAnimatable),
        ("transition-duration", AnimationType::NotAnimatable),
        ("transition-property", AnimationType::NotAnimatable),
        ("position", AnimationType::Discrete),
        ("font-family", AnimationType::Discrete),
        ("border-top-style", AnimationType::Discrete),
        ("background-image", AnimationType::Discrete),
        ("color", AnimationType::ByComputedValue),
        ("width", AnimationType::ByComputedValue),
        ("opacity", AnimationType::ByComputedValue),
        ("transform", AnimationType::ByComputedValue),
        ("transform-origin", AnimationType::ByComputedValue),
        ("transform-box", AnimationType::Discrete),
        ("object-position", AnimationType::ByComputedValue),
        ("-webkit-box-flex", AnimationType::Discrete),
        ("-webkit-line-clamp", AnimationType::Discrete),
        ("contain", AnimationType::NotAnimatable),
        ("clip", AnimationType::ByComputedValue),
        ("clip-path", AnimationType::ByComputedValue),
        ("clip-rule", AnimationType::Discrete),
        ("tab-size", AnimationType::ByComputedValue),
        ("object-fit", AnimationType::Discrete),
        ("visibility", AnimationType::ByComputedValue),
        ("box-shadow", AnimationType::ByComputedValue),
        ("text-size-adjust", AnimationType::ByComputedValue),
        (
            "-webkit-tap-highlight-color",
            AnimationType::ByComputedValue,
        ),
        ("caret-color", AnimationType::ByComputedValue),
        ("user-select", AnimationType::Discrete),
        ("resize", AnimationType::Discrete),
        // A flow-relative longhand is its physical one's.
        ("margin-inline-start", AnimationType::ByComputedValue),
        ("border-block-start-style", AnimationType::Discrete),
    ];
    for (name, expected) in types {
        assert_eq!(id(name).animation_type(), expected, "{name}");
    }
}
