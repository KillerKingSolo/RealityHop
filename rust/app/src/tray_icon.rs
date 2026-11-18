//! The tray icon: the center shows the overall state, the outer ring one segment per node (green working, amber slow,
//! red dead, grey not checked yet). Drawn in memory, the same design as 1.7.x's tray_icon_file.

use tiny_skia::{Color, FillRule, LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};

/// The size the icon is drawn at; Windows and macOS scale it down.
pub const SIZE: u32 = 64;

// The color of a state or ring segment
fn color(state: &str, center: bool) -> (u8, u8, u8) {
    match state {
        "ok" => (95, 211, 141),
        "warn" => (245, 165, 36),
        "bad" => (255, 77, 79),
        _ if center => (120, 120, 130),
        _ => (70, 70, 80),
    }
}

// A paint of one color
fn paint((r, g, b): (u8, u8, u8)) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(r, g, b, 255));
    p.anti_alias = true;
    p
}

// The icon as straight (not premultiplied) RGBA, SIZE x SIZE. The design is laid out on 256 px, as 1.7.x drew it
pub fn draw(state: &str, segs: &[String]) -> Vec<u8> {
    let mut pm = Pixmap::new(SIZE, SIZE).expect("icon size");
    let k = SIZE as f32 / 256.0;
    let unk = vec!["unk".to_string(); 6];
    let segs = if segs.is_empty() { &unk[..] } else { segs };
    let disc = |r: f32| PathBuilder::from_circle(128.0 * k, 128.0 * k, r * k);
    if let Some(p) = disc(122.0) {
        pm.fill_path(&p, &paint((18, 18, 22)), FillRule::Winding, Transform::identity(), None);
    }
    // the ring: on the circle of radius 97 (PIL's arc of width 34 inside the box 14..242), clockwise from 12 o'clock
    let step = 360.0 / segs.len() as f32;
    let gap = if segs.len() > 1 { 10.0 } else { 0.0 };
    let stroke = Stroke { width: 34.0 * k, line_cap: LineCap::Butt, ..Stroke::default() };
    for (i, s) in segs.iter().enumerate() {
        let start = -90.0 + i as f32 * step + gap / 2.0;
        let sweep = step - gap;
        let n = (sweep / 2.0).ceil().max(2.0) as usize;
        let mut pb = PathBuilder::new();
        for j in 0..=n {
            let a = (start + sweep * j as f32 / n as f32).to_radians();
            let (x, y) = ((128.0 + 97.0 * a.cos()) * k, (128.0 + 97.0 * a.sin()) * k);
            if j == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
        }
        if let Some(p) = pb.finish() {
            pm.stroke_path(&p, &paint(color(s, false)), &stroke, Transform::identity(), None);
        }
    }
    if let Some(p) = disc(44.0) {
        pm.fill_path(&p, &paint(color(state, true)), FillRule::Winding, Transform::identity(), None);
    }
    pm.pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}
