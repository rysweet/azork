//! The native, offline, deterministic dungeon renderer.
//!
//! Produces a single self-contained HTML document (inline SVG + a small
//! amount of vanilla JS — no build step, no CDN fetch) laying rooms out on a
//! grid with corridor walls and resource icons. Layout is a pure function of
//! the [`crate::dungeon::DungeonMap`], so the same subscription always
//! produces the same document. See
//! `docs/DUNGEON-CRAWLER.md#rendering`.

use crate::dungeon::map::DungeonMap;
use std::collections::HashMap;

/// Render `map` to a self-contained HTML document.
///
/// Room and resource names are untrusted strings (an attacker-controlled
/// Azure tenant could name a resource `<script>...`) and MUST always be
/// HTML/SVG-escaped in the output so a hostile name can never inject markup
/// or script into the page.
pub fn render_html(map: &DungeonMap) -> String {
    const CELL: i32 = 140;
    const ROOM_SIZE: i32 = 110;

    let mut svg_rooms = String::new();
    let mut svg_edges = String::new();
    let mut room_max_x = 0;
    let mut room_max_y = 0;

    // Many resources across a subscription share the same icon key (e.g.
    // every storage account); cache each key's short glyph the first time
    // it's computed instead of re-deriving it per resource.
    let mut glyph_cache: HashMap<&str, String> = HashMap::new();

    for room in &map.rooms {
        room_max_x = room_max_x.max(room.x);
        room_max_y = room_max_y.max(room.y);
        let px = room.x * CELL;
        let py = room.y * CELL;

        let mut resource_icons = String::new();
        for (i, res) in room.resources.iter().enumerate() {
            let ix = px + 10 + (i as i32 % 4) * 24;
            let iy = py + 34 + (i as i32 / 4) * 24;
            let icon_short = glyph_cache
                .entry(res.icon.as_str())
                .or_insert_with(|| short_icon_glyph(&res.icon));
            resource_icons.push_str(&format!(
                "<g class=\"resource\" data-resource-id=\"{id}\"><title>{name} ({kind})</title>\
                 <rect x=\"{ix}\" y=\"{iy}\" width=\"20\" height=\"20\" rx=\"3\" class=\"icon icon-{icon}\"/>\
                 <text x=\"{tx}\" y=\"{ty}\" class=\"icon-label\">{icon_short}</text></g>",
                id = escape_html(&res.id),
                name = escape_html(&res.name),
                kind = escape_html(&res.kind),
                ix = ix,
                iy = iy,
                tx = ix + 2,
                ty = iy + 14,
                icon = escape_html(&res.icon),
                icon_short = escape_html(icon_short),
            ));
        }

        svg_rooms.push_str(&format!(
            "<g class=\"room\" data-room-id=\"{id}\">\
             <rect x=\"{px}\" y=\"{py}\" width=\"{w}\" height=\"{h}\" rx=\"6\" class=\"room-wall\"/>\
             <text x=\"{tx}\" y=\"{ty}\" class=\"room-label\">{name}</text>\
             {icons}</g>",
            id = escape_html(&room.id),
            px = px,
            py = py,
            w = ROOM_SIZE,
            h = ROOM_SIZE,
            tx = px + 8,
            ty = py + 18,
            name = escape_html(&room.name),
            icons = resource_icons,
        ));
    }

    for edge in &map.edges {
        if let (Some(a), Some(b)) = (map.room(&edge.from), map.room(&edge.to)) {
            let (ax, ay) = (a.x * CELL + ROOM_SIZE / 2, a.y * CELL + ROOM_SIZE / 2);
            let (bx, by) = (b.x * CELL + ROOM_SIZE / 2, b.y * CELL + ROOM_SIZE / 2);
            svg_edges.push_str(&format!(
                "<line x1=\"{ax}\" y1=\"{ay}\" x2=\"{bx}\" y2=\"{by}\" class=\"corridor\"/>"
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

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>AzZork Dungeon Crawler — {subscription}</title>
<style>
  body {{ background: #14110f; color: #e8dcc4; font-family: 'Courier New', monospace; margin: 0; padding: 16px; }}
  h1 {{ font-size: 1.1em; }}
  .partial-banner {{ background: #5a3a1a; color: #ffe9b3; padding: 8px; margin-bottom: 12px; border: 1px solid #a97; }}
  svg {{ background: #1c1712; border: 2px solid #6b4f2a; }}
  .room-wall {{ fill: #2a2116; stroke: #a97c3f; stroke-width: 3; }}
  .room-label {{ fill: #e8dcc4; font-size: 12px; }}
  .corridor {{ stroke: #6b4f2a; stroke-width: 4; stroke-dasharray: 2 3; }}
  .icon {{ fill: #7fae4a; stroke: #33220f; }}
  .icon-label {{ fill: #14110f; font-size: 8px; }}
  #detail {{ margin-top: 12px; padding: 10px; border: 1px solid #6b4f2a; display: none; }}
  #detail a {{ color: #9fd3ff; }}
  #detail code {{ display: block; background: #0c0a08; padding: 4px; margin: 2px 0; }}
</style>
</head>
<body>
<h1>Dungeon map — subscription {subscription}</h1>
{partial_banner}
<svg width="{width}" height="{height}">
{edges}
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
        var html = '<strong></strong><br>Type: <span></span><br>';
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
        subscription = escape_html(&map.subscription),
        partial_banner = partial_banner,
        width = width,
        height = height,
        edges = svg_edges,
        rooms = svg_rooms,
    )
}

/// A short (<=2 char) glyph shown inline on a resource's icon tile; purely
/// cosmetic, derived from the icon key so it's stable and offline.
fn short_icon_glyph(icon: &str) -> String {
    icon.split('-')
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
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
