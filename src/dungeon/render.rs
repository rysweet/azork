//! The native, offline, deterministic dungeon renderer.
//!
//! Produces a single self-contained HTML document (inline SVG + a small
//! amount of vanilla JS — no build step, no CDN fetch) laying rooms out as
//! walled rectilinear chambers on a parchment-and-grid background, joined
//! by orthogonal corridors with doors, each holding its resources' Azure
//! architecture icons. Layout is a pure function of the
//! [`crate::dungeon::DungeonMap`], so the same subscription always produces
//! the same document. See `docs/DUNGEON-CRAWLER.md#rendering` and
//! `docs/DUNGEON-CRAWLER.md#why-a-self-designed-renderer-tool-evaluation`
//! for the tabletop-dungeon-map styling this renderer targets and why it is
//! self-designed rather than driving a third-party map tool.

use crate::dungeon::decorations;
use crate::dungeon::icon_assets;
use crate::dungeon::map::{DungeonMap, Room};
use crate::secrets::scrub;
use std::collections::{HashMap, HashSet};

/// Fixed icon size (px). Rooms grow to fit their resource count rather than
/// icons shrinking to fit a fixed room, so every icon always renders at a
/// legible, consistent size regardless of how many resources share a room.
const ICON_SIZE: i32 = 20;
/// Gap (px) between adjacent icons within a room's icon grid.
const ICON_GAP: i32 = 6;
/// Wall stroke thickness (px), matched to the `.room-wall` stroke-width.
const WALL: i32 = 4;
/// Left/right/bottom padding (px) inside a room's walls around its icon
/// grid.
const ROOM_PADDING: i32 = 8;
/// Vertical space (px) reserved at the top of a room for its label, above
/// the icon grid.
const ROOM_HEADER: i32 = 30;
/// Smallest room footprint (px) regardless of resource count, so a room
/// with zero or few resources still reads as a proper dungeon chamber
/// rather than a cramped sliver.
const ROOM_MIN_SIZE: i32 = 116;
/// Fixed gap (px) added around the single largest room's footprint to
/// derive the uniform grid cell spacing (see [`grid_cell_size`]). Kept
/// generous enough that corridors have visible open space alongside walls
/// even when the largest room in the map is much bigger than
/// [`ROOM_MIN_SIZE`].
const CORRIDOR_GAP: i32 = 80;

/// A room's rendered pixel footprint, computed purely from its resource
/// count so it can never silently overflow (unlike a fixed-size room with
/// an unbounded icon grid).
struct RoomLayout {
    width: i32,
    height: i32,
    cols: i32,
}

/// Compute the icon grid shape and pixel footprint for a room holding
/// `resource_count` resources. The grid is as close to square as possible
/// (`cols = ceil(sqrt(n))`), and the resulting width/height are clamped to
/// [`ROOM_MIN_SIZE`] so small rooms keep their original, familiar size.
fn room_layout(resource_count: usize) -> RoomLayout {
    let n = resource_count.max(1) as i32;
    let cols = (n as f64).sqrt().ceil() as i32;
    let cols = cols.max(1);
    let rows = ((n + cols - 1) / cols).max(1);

    let content_w = cols * ICON_SIZE + (cols - 1).max(0) * ICON_GAP;
    let content_h = rows * ICON_SIZE + (rows - 1).max(0) * ICON_GAP;

    let width = (WALL * 2 + ROOM_PADDING * 2 + content_w).max(ROOM_MIN_SIZE);
    let height = (WALL * 2 + ROOM_HEADER + ROOM_PADDING + content_h).max(ROOM_MIN_SIZE);

    RoomLayout {
        width,
        height,
        cols,
    }
}

/// Render `map` to a self-contained HTML document.
///
/// Room and resource names are untrusted strings (an attacker-controlled
/// Azure tenant could name a resource `<script>...`) and MUST always be
/// HTML/SVG-escaped in the output so a hostile name can never inject markup
/// or script into the page.
pub fn render_html(map: &DungeonMap) -> String {
    // Rough per-room/per-edge output size estimates so the accumulator
    // strings grow once up front instead of reallocating/copying on every
    // push for subscriptions with hundreds of rooms or corridors.
    let mut svg_rooms = String::with_capacity(map.rooms.len() * 256);
    let mut svg_corridors = String::with_capacity(map.edges.len() * 256);
    let mut svg_icon_defs = String::new();
    let mut room_max_x = 0;
    let mut room_max_y = 0;

    // Each icon key's SVG shape only needs to be embedded once as a shared
    // <symbol> definition; every resource instance then just references it
    // via <use>, so a subscription with hundreds of storage accounts still
    // ships one copy of the storage-account icon artwork.
    let mut defined_icons: HashSet<&'static str> = HashSet::new();

    // Index rooms by id once so corridor lookups below are O(1) each
    // instead of an O(rooms) linear scan per edge (map.room() does a linear
    // find, which would make this loop O(edges * rooms) for large maps).
    let mut rooms_by_id: HashMap<&str, &Room> = HashMap::with_capacity(map.rooms.len());
    for room in &map.rooms {
        rooms_by_id.insert(room.id.as_str(), room);
    }

    // Every room's own footprint is a pure function of its resource count,
    // computed once up front (and reused below for both icon placement and
    // corridor endpoints) rather than assuming a shared fixed size.
    let mut layouts: HashMap<&str, RoomLayout> = HashMap::with_capacity(map.rooms.len());
    let mut max_room_dim = ROOM_MIN_SIZE;
    for room in &map.rooms {
        let layout = room_layout(room.resources.len());
        max_room_dim = max_room_dim.max(layout.width).max(layout.height);
        layouts.insert(room.id.as_str(), layout);
    }

    // A single uniform grid cell, sized off the *largest* room's footprint
    // plus a fixed gap, guarantees no room/corridor can ever collide no
    // matter how large one room's icon grid grows relative to its
    // neighbors (unlike a fixed cell size, which silently overflows once a
    // room exceeds it).
    let cell = max_room_dim + CORRIDOR_GAP;

    for room in &map.rooms {
        room_max_x = room_max_x.max(room.x);
        room_max_y = room_max_y.max(room.y);
        let px = room.x * cell + decorations::MAP_MARGIN;
        let py = room.y * cell + decorations::MAP_MARGIN;
        let layout = &layouts[room.id.as_str()];

        let mut resource_icons = String::new();
        for (i, res) in room.resources.iter().enumerate() {
            let key = icon_assets::canonical_key(&res.icon);
            if defined_icons.insert(key) {
                svg_icon_defs.push_str(&format!(
                    "<symbol id=\"icon-{key}\" viewBox=\"{view_box}\">{inner}</symbol>",
                    key = key,
                    view_box = icon_assets::view_box(key),
                    inner = icon_assets::inner_markup(key),
                ));
            }

            let ix = px + WALL + ROOM_PADDING + (i as i32 % layout.cols) * (ICON_SIZE + ICON_GAP);
            let iy = py + WALL + ROOM_HEADER + (i as i32 / layout.cols) * (ICON_SIZE + ICON_GAP);
            resource_icons.push_str(&format!(
                "<g class=\"resource\" data-resource-id=\"{id}\">\
                 <title>{name} ({kind})</title>\
                 <use href=\"#icon-{key}\" x=\"{ix}\" y=\"{iy}\" width=\"{size}\" height=\"{size}\" \
                 class=\"icon icon-{key}\" data-icon=\"{key}\"/></g>",
                id = escape_html(&scrub(&res.id)),
                name = escape_html(&scrub(&res.name)),
                kind = escape_html(&scrub(&res.kind)),
                key = key,
                ix = ix,
                iy = iy,
                size = ICON_SIZE,
            ));
        }

        // A "walled chamber": an outer wall stroke plus a slightly inset
        // floor fill, evoking a hand-drawn dungeon room rather than a flat
        // node-graph box.
        svg_rooms.push_str(&format!(
            "<g class=\"room\" data-room-id=\"{id}\">\
             <rect x=\"{px}\" y=\"{py}\" width=\"{w}\" height=\"{h}\" rx=\"3\" class=\"room-floor\"/>\
             <rect x=\"{px}\" y=\"{py}\" width=\"{w}\" height=\"{h}\" rx=\"3\" class=\"room-wall\"/>\
             <text x=\"{tx}\" y=\"{ty}\" class=\"room-label\">{name}</text>\
             {icons}</g>",
            id = escape_html(&scrub(&room.id)),
            px = px,
            py = py,
            w = layout.width,
            h = layout.height,
            tx = px + 8,
            ty = py + 18,
            name = escape_html(&scrub(&room.name)),
            icons = resource_icons,
        ));
    }

    for edge in &map.edges {
        if let (Some(a), Some(b)) = (
            rooms_by_id.get(edge.from.as_str()),
            rooms_by_id.get(edge.to.as_str()),
        ) {
            let a_layout = &layouts[a.id.as_str()];
            let b_layout = &layouts[b.id.as_str()];
            svg_corridors.push_str(&corridor_path(
                a.x * cell + decorations::MAP_MARGIN,
                a.y * cell + decorations::MAP_MARGIN,
                a_layout.width,
                a_layout.height,
                b.x * cell + decorations::MAP_MARGIN,
                b.y * cell + decorations::MAP_MARGIN,
                b_layout.width,
                b_layout.height,
            ));
        }
    }

    let width = (room_max_x + 1) * cell + decorations::MAP_MARGIN * 2;
    let height = (room_max_y + 1) * cell + decorations::MAP_MARGIN * 2;
    let svg_decorations = decorations::build(
        width,
        height,
        &icon_assets::inner_markup("mystery-chest"),
        &icon_assets::view_box("mystery-chest"),
    );

    let partial_banner = if map.partial {
        "<div class=\"partial-banner\">⚠ Partial map — enumeration was cancelled or incomplete; not every room in the subscription is shown.</div>"
    } else {
        ""
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>AzZork Dungeon Crawler — {subscription}</title>
<style>
  body {{ background: #2b2013; color: #3a2c18; font-family: 'Georgia', 'Courier New', serif; margin: 0; padding: 16px; }}
  h1 {{ font-size: 1.1em; color: #e8dcc4; }}
  .partial-banner {{ background: #5a3a1a; color: #ffe9b3; padding: 8px; margin-bottom: 12px; border: 1px solid #a97; }}
  svg {{ background: #e8dcbd; border: 3px solid #4a3418; }}
  .parchment {{ fill: #e8dcbd; }}
  .grid-line {{ stroke: #cbb98a; stroke-width: 1; }}
  .room-floor {{ fill: #f2e8cc; }}
  .room-wall {{ fill: none; stroke: #3a2c18; stroke-width: 5; }}
  .room-label {{ fill: #3a2c18; font-size: 12px; font-weight: bold; }}
  .corridor {{ fill: none; stroke: #3a2c18; stroke-width: 10; stroke-linecap: square; }}
  .corridor-fill {{ fill: none; stroke: #d8c896; stroke-width: 6; stroke-linecap: square; }}
  .door {{ fill: #6b4226; stroke: #2a1c10; stroke-width: 1; }}
  .icon {{ color: #3a2c18; }}
  .decoration {{ pointer-events: none; }}
  .map-border-outer {{ fill: none; stroke: #4a3418; stroke-width: 4; }}
  .map-border-inner {{ fill: none; stroke: #4a3418; stroke-width: 1; }}
  .torch-post {{ fill: #4a3418; }}
  .torch-flame {{ fill: #e8912b; stroke: #a9520a; stroke-width: 1; }}
  #detail {{ margin-top: 12px; padding: 10px; border: 2px solid #4a3418; display: none; background: #1c1712; color: #e8dcc4; }}
  #detail a {{ color: #9fd3ff; }}
  #detail code {{ display: block; background: #0c0a08; padding: 4px; margin: 2px 0; }}
</style>
</head>
<body>
<h1>Dungeon map — subscription {subscription}</h1>
{partial_banner}
<svg width="{width}" height="{height}">
<defs>
<pattern id="grid" width="20" height="20" patternUnits="userSpaceOnUse">
<path d="M 20 0 L 0 0 0 20" class="grid-line" fill="none"/>
</pattern>
{icon_defs}
</defs>
<rect x="0" y="0" width="{width}" height="{height}" class="parchment"/>
<rect x="0" y="0" width="{width}" height="{height}" fill="url(#grid)"/>
{corridors}
{rooms}
{decorations}
</svg>
<div id="detail"></div>
<script>
(function() {{
  var detail = document.getElementById('detail');
  document.querySelectorAll('.resource').forEach(function(el) {{
    el.addEventListener('click', function(evt) {{
      evt.stopPropagation();
      var id = el.getAttribute('data-resource-id');
      fetch('/api/v1/resources/' + id).then(function(r) {{ return r.json(); }}).then(function(data) {{
        detail.innerHTML = '';
        var h = document.createElement('strong');
        h.textContent = data.name;
        detail.appendChild(h);
        var t = document.createElement('div');
        t.textContent = 'Type: ' + data.kind;
        detail.appendChild(t);
        var link = document.createElement('a');
        link.href = data.portal_url;
        link.target = '_blank';
        link.textContent = 'Open in Azure Portal';
        detail.appendChild(document.createElement('br'));
        detail.appendChild(link);
        (data.suggested_commands || []).forEach(function(cmd) {{
          var code = document.createElement('code');
          code.textContent = cmd;
          detail.appendChild(code);
        }});
        detail.style.display = 'block';
      }});
    }});
  }});
}})();
</script>
</body>
</html>
"#,
        subscription = escape_html(&scrub(&map.subscription)),
        partial_banner = partial_banner,
        width = width,
        height = height,
        icon_defs = svg_icon_defs,
        corridors = svg_corridors,
        rooms = svg_rooms,
        decorations = svg_decorations,
    );
    scrub(&html)
}

/// Build an orthogonal (right-angle) corridor `<path>` between two rooms'
/// walls, plus a `<door>` marker at each end where the corridor meets the
/// room — the "L-shaped hallway with doors" look of a classic tabletop
/// dungeon map, replacing a bare diagonal `<line>` between room centers.
/// Each room's own computed width/height is used (rather than a shared
/// constant), so a corridor always meets a room's *actual* wall regardless
/// of how that room's adaptive size differs from its neighbor's.
#[allow(clippy::too_many_arguments)]
fn corridor_path(
    ax: i32,
    ay: i32,
    a_w: i32,
    a_h: i32,
    bx: i32,
    by: i32,
    b_w: i32,
    b_h: i32,
) -> String {
    let a_cx = ax + a_w / 2;
    let a_cy = ay + a_h / 2;
    let b_cx = bx + b_w / 2;
    let b_cy = by + b_h / 2;

    // Exit A's wall on the side facing B, and enter B's wall on the side
    // facing A, then join the two points with one right-angle bend.
    let (exit_x, exit_y) = if b_cx >= a_cx {
        (ax + a_w, a_cy)
    } else {
        (ax, a_cy)
    };
    let (entry_x, entry_y) = if a_cx >= b_cx {
        (bx + b_w, b_cy)
    } else {
        (bx, b_cy)
    };
    let mid_x = (exit_x + entry_x) / 2;

    let path = format!(
        "<path class=\"corridor\" d=\"M {ex} {ey} L {mx} {ey} L {mx} {fy} L {fx} {fy}\"/>\
         <path class=\"corridor-fill\" d=\"M {ex} {ey} L {mx} {ey} L {mx} {fy} L {fx} {fy}\"/>",
        ex = exit_x,
        ey = exit_y,
        mx = mid_x,
        fy = entry_y,
        fx = entry_x,
    );

    // Door glyphs: small filled rects straddling each wall opening.
    let door_a = format!(
        "<rect x=\"{x}\" y=\"{y}\" width=\"6\" height=\"12\" class=\"door\"/>",
        x = exit_x - 3,
        y = exit_y - 6
    );
    let door_b = format!(
        "<rect x=\"{x}\" y=\"{y}\" width=\"6\" height=\"12\" class=\"door\"/>",
        x = entry_x - 3,
        y = entry_y - 6
    );

    format!("{path}{door_a}{door_b}")
}

/// Escape a string for safe inclusion in HTML/SVG text content or
/// double-quoted attribute values.
pub fn escape_html(s: &str) -> String {
    // The overwhelming majority of resource/room names contain no
    // characters that need escaping; skip the char-by-char rebuild (and its
    // allocation) entirely in that common case.
    if !s.contains(['&', '<', '>', '"', '\'']) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}
