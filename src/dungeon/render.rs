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

use crate::dungeon::icon_assets;
use crate::dungeon::map::DungeonMap;
use crate::secrets::scrub;
use std::collections::HashSet;

/// Render `map` to a self-contained HTML document.
///
/// Room and resource names are untrusted strings (an attacker-controlled
/// Azure tenant could name a resource `<script>...`) and MUST always be
/// HTML/SVG-escaped in the output so a hostile name can never inject markup
/// or script into the page.
pub fn render_html(map: &DungeonMap) -> String {
    const CELL: i32 = 150;
    const ROOM_SIZE: i32 = 116;
    const WALL: i32 = 4;
    const ICON_SIZE: i32 = 20;

    let mut svg_rooms = String::new();
    let mut svg_corridors = String::new();
    let mut svg_icon_defs = String::new();
    let mut room_max_x = 0;
    let mut room_max_y = 0;

    // Each icon key's SVG shape only needs to be embedded once as a shared
    // <symbol> definition; every resource instance then just references it
    // via <use>, so a subscription with hundreds of storage accounts still
    // ships one copy of the storage-account icon artwork.
    let mut defined_icons: HashSet<&'static str> = HashSet::new();

    for room in &map.rooms {
        room_max_x = room_max_x.max(room.x);
        room_max_y = room_max_y.max(room.y);
        let px = room.x * CELL;
        let py = room.y * CELL;

        let mut resource_icons = String::new();
        for (i, res) in room.resources.iter().enumerate() {
            let key = icon_assets::canonical_key(&res.icon);
            if defined_icons.insert(key) {
                svg_icon_defs.push_str(&format!(
                    "<symbol id=\"icon-{key}\" viewBox=\"0 0 24 24\">{inner}</symbol>",
                    key = key,
                    inner = icon_assets::inner_markup(key),
                ));
            }

            let ix = px + WALL + 8 + (i as i32 % 4) * (ICON_SIZE + 6);
            let iy = py + WALL + 30 + (i as i32 / 4) * (ICON_SIZE + 6);
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
            w = ROOM_SIZE,
            h = ROOM_SIZE,
            tx = px + 8,
            ty = py + 18,
            name = escape_html(&scrub(&room.name)),
            icons = resource_icons,
        ));
    }

    for edge in &map.edges {
        if let (Some(a), Some(b)) = (map.room(&edge.from), map.room(&edge.to)) {
            svg_corridors.push_str(&corridor_path(
                a.x * CELL,
                a.y * CELL,
                b.x * CELL,
                b.y * CELL,
                ROOM_SIZE,
            ));
        }
    }

    let width = (room_max_x + 1) * CELL + 40;
    let height = (room_max_y + 1) * CELL + 40;

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
    );
    scrub(&html)
}

/// Build an orthogonal (right-angle) corridor `<path>` between two rooms'
/// walls, plus a `<door>` marker at each end where the corridor meets the
/// room — the "L-shaped hallway with doors" look of a classic tabletop
/// dungeon map, replacing a bare diagonal `<line>` between room centers.
fn corridor_path(ax: i32, ay: i32, bx: i32, by: i32, room_size: i32) -> String {
    let a_cx = ax + room_size / 2;
    let a_cy = ay + room_size / 2;
    let b_cx = bx + room_size / 2;
    let b_cy = by + room_size / 2;

    // Exit A's wall on the side facing B, and enter B's wall on the side
    // facing A, then join the two points with one right-angle bend.
    let (exit_x, exit_y) = if b_cx >= a_cx {
        (ax + room_size, a_cy)
    } else {
        (ax, a_cy)
    };
    let (entry_x, entry_y) = if a_cx >= b_cx {
        (bx + room_size, b_cy)
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
