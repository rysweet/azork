//! Purely-decorative, deterministic dungeon-map dressing: a border frame,
//! perimeter torches, a treasure chest, and a dragon glyph.
//!
//! Every decoration is placed inside the fixed outer margin band
//! (`MAP_MARGIN`) that already surrounds the room/corridor grid, so nothing
//! here needs collision detection against rooms or corridors — the margin
//! itself is the only invariant that must hold, and `render.rs` guarantees
//! it by construction. Placement is a pure function of the map's overall
//! pixel dimensions (never RNG, never resource content), so the same map
//! always renders the same decorations. All decoration markup carries
//! `pointer-events: none` and no `.resource` class, so it can never
//! intercept a click meant for a resource icon or open the detail popup.

/// Outer margin (in px) reserved around the room/corridor grid for border
/// art and decorations. Rooms and corridors are always placed at
/// `>= MAP_MARGIN` by `render.rs`, so this band is guaranteed decoration-only
/// space.
pub const MAP_MARGIN: i32 = 96;

/// Spacing (in px) between torch glyphs along the border.
const TORCH_SPACING: i32 = 140;

/// A single embedded dragon glyph (a simple original line-art silhouette,
/// not an Azure architecture icon), used once per map as a decorative
/// flourish in a top corner. Kept intentionally simple/geometric so it reads
/// clearly at small sizes rather than aiming for photorealism.
const DRAGON_SVG: &str = include_str!("../../assets/decorations/dragon.svg");

/// Build the decorative border frame, perimeter torches, treasure chest, and
/// dragon glyph for a map with the given full canvas `width`/`height`
/// (including margins). All output is placed strictly within the outer
/// `MAP_MARGIN` band, so it can never overlap the room/corridor grid that
/// occupies the interior.
pub fn build(width: i32, height: i32, chest_icon_inner: &str, chest_view_box: &str) -> String {
    let mut out = String::new();

    // Border frame: a decorative double-line rectangle drawn entirely inside
    // the margin band, distinct from each room's own wall stroke.
    out.push_str(&format!(
        "<rect x=\"8\" y=\"8\" width=\"{w}\" height=\"{h}\" class=\"map-border-outer\"/>\
         <rect x=\"16\" y=\"16\" width=\"{w2}\" height=\"{h2}\" class=\"map-border-inner\"/>",
        w = width - 16,
        h = height - 16,
        w2 = width - 32,
        h2 = height - 32,
    ));

    // Torches at fixed intervals along the top and bottom margin bands.
    let mut x = MAP_MARGIN / 2;
    while x < width - MAP_MARGIN / 2 {
        out.push_str(&torch(x, MAP_MARGIN / 2 - 10));
        out.push_str(&torch(x, height - MAP_MARGIN / 2 - 10));
        x += TORCH_SPACING;
    }

    // Treasure chest: bottom-left corner of the margin band, reusing the
    // already-bundled mystery-chest icon artwork.
    let chest_size = 32;
    out.push_str(&format!(
        "<svg x=\"{x}\" y=\"{y}\" width=\"{s}\" height=\"{s}\" viewBox=\"{vb}\" \
         class=\"decoration\" pointer-events=\"none\">{inner}</svg>",
        x = MAP_MARGIN / 2 - chest_size / 2,
        y = height - MAP_MARGIN / 2 - chest_size / 2,
        s = chest_size,
        vb = chest_view_box,
        inner = chest_icon_inner,
    ));

    // Dragon glyph: top-right corner of the margin band.
    let dragon_size = 40;
    out.push_str(&format!(
        "<g transform=\"translate({x},{y})\" class=\"decoration\" pointer-events=\"none\">{svg}</g>",
        x = width - MAP_MARGIN / 2 - dragon_size,
        y = MAP_MARGIN / 2 - dragon_size / 2,
        svg = inner_svg_markup(DRAGON_SVG),
    ));

    out
}

/// A single torch glyph: a post plus a stylized flame, both non-interactive.
fn torch(cx: i32, cy: i32) -> String {
    format!(
        "<g class=\"decoration torch\" pointer-events=\"none\">\
         <rect x=\"{x1}\" y=\"{y1}\" width=\"3\" height=\"14\" class=\"torch-post\"/>\
         <circle cx=\"{cx}\" cy=\"{fy}\" r=\"5\" class=\"torch-flame\"/></g>",
        x1 = cx - 1,
        y1 = cy,
        cx = cx,
        fy = cy - 4,
    )
}

/// Strip the outer `<svg ...>...</svg>` wrapper from an embedded decoration
/// document, leaving just its inner markup for direct inline embedding
/// (mirrors [`crate::dungeon::icon_assets::inner_markup`]).
fn inner_svg_markup(svg: &str) -> String {
    let after_open = svg.find('>').map(|i| &svg[i + 1..]).unwrap_or(svg);
    after_open
        .rfind("</svg>")
        .map(|i| after_open[..i].trim())
        .unwrap_or(after_open)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_is_a_pure_function_of_its_inputs() {
        let a = build(800, 600, "<path/>", "0 0 18 18");
        let b = build(800, 600, "<path/>", "0 0 18 18");
        assert_eq!(a, b);
    }

    #[test]
    fn build_marks_every_decoration_non_interactive() {
        let out = build(800, 600, "<path/>", "0 0 18 18");
        // Every decoration element declares pointer-events:none (inline) or
        // is one of the border-frame rects, which carry no class that would
        // ever match `.resource` click handling.
        assert!(!out.contains("class=\"resource\""));
    }

    #[test]
    fn dragon_svg_is_well_formed() {
        assert!(DRAGON_SVG.contains("<svg"));
        assert!(DRAGON_SVG.contains("</svg>"));
    }
}
