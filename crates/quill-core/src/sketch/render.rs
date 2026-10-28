//! Draws a screenshot's ink, click ring, mark numbers and "#N 0:15" badge,
//! crops around the ink, and lays several screenshots out on one sheet.

use ab_glyph::{point, Font, FontVec, OutlinedGlyph, PxScale, ScaleFont};
use tiny_skia::{
    Color, FillRule, FilterQuality, IntSize, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect,
    Stroke as Pen, Transform,
};

use super::{Image, Moment, Point};

pub const MAXIMUM_LONG_EDGE: u32 = 2048;
pub const INK: [u8; 3] = [255, 59, 48];
pub const INK_WIDTH: f32 = 4.0;

const SHEET_WIDTH: u32 = 2400;
const GUTTER: u32 = 8;

pub struct Renderer {
    font: Option<FontVec>,
}

impl Renderer {
    /// `font` is a bold TTF. Without one, text is left out.
    pub fn new(font: Option<Vec<u8>>) -> Self {
        Self { font: font.and_then(|bytes| FontVec::try_from_vec(bytes).ok()) }
    }

    /// The moment's screenshot, at most `MAXIMUM_LONG_EDGE` on its long
    /// edge, with its ink, click and badge drawn on.
    pub fn render(&self, moment: &Moment, badge: Option<&str>) -> Image {
        let source = &moment.image;
        let Some(source_map) = to_pixmap(source) else { return (*source.as_ref()).clone() };
        let fit = (MAXIMUM_LONG_EDGE as f32 / source.width.max(source.height) as f32).min(1.0);
        let width = ((source.width as f32 * fit).round() as u32).max(1);
        let height = ((source.height as f32 * fit).round() as u32).max(1);
        let mut map = if width == source.width && height == source.height {
            source_map
        } else {
            let Some(mut map) = Pixmap::new(width, height) else { return (*source.as_ref()).clone() };
            draw_scaled(&mut map, &source_map, 0.0, 0.0, width as f32 / source.width as f32, height as f32 / source.height as f32);
            map
        };

        let scale = width as f32 / source.width as f32;
        let unit = (width as f32 / 1280.0).max(1.0);
        let at = |p: Point| Point { x: p.x * scale, y: p.y * scale };
        let (w, h) = (width as f32, height as f32);

        let ink_pen = Pen { width: INK_WIDTH * unit, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
        let red = paint(INK[0], INK[1], INK[2], 255);
        for stroke in &moment.strokes {
            let points: Vec<Point> = stroke.points.iter().copied().map(at).collect();
            if let Some(path) = smooth_path(&points) {
                map.stroke_path(&path, &red, &ink_pen, Transform::identity(), None);
            }
        }
        if moment.strokes.len() > 1 {
            for (index, stroke) in moment.strokes.iter().enumerate() {
                let Some(&start) = stroke.points.first() else { continue };
                let start = at(start);
                let radius = 11.0 * unit;
                // Up and to the left of where the stroke began, kept on the image.
                let cx = (start.x - 14.0 * unit).max(radius + 2.0).min(w - radius - 2.0);
                let cy = (start.y - 14.0 * unit).max(radius + 2.0).min(h - radius - 2.0);
                fill_circle(&mut map, cx, cy, radius, &red);
                let number = (index + 1).to_string();
                let size = if index + 1 > 9 { 11.0 } else { 13.0 } * unit;
                self.draw_text_centered(&mut map, &number, size, cx, cy);
            }
        }
        if let Some(click) = moment.click.map(at) {
            // A click is drawn apart from ink: a white ring with a dark edge.
            let radius = 16.0 * unit;
            if let Some(ring) = PathBuilder::from_circle(click.x, click.y, radius) {
                let pen = |width: f32| Pen { width, ..Default::default() };
                map.stroke_path(&ring, &paint(0, 0, 0, 140), &pen(5.0 * unit), Transform::identity(), None);
                map.stroke_path(&ring, &paint(255, 255, 255, 242), &pen(3.0 * unit), Transform::identity(), None);
            }
            fill_circle(&mut map, click.x, click.y, 3.0 * unit, &paint(255, 255, 255, 242));
        }
        if let Some(badge) = badge {
            let mut ink: Vec<Point> = moment.strokes.iter().flat_map(|s| s.points.iter().copied().map(at)).collect();
            ink.extend(moment.click.map(at));
            self.draw_badge(&mut map, badge, unit, &ink);
        }
        from_pixmap(map)
    }

    /// The part of a rendered screenshot around some ink, at least 720 px
    /// wide, so an agent sees what was marked and not only where. `around`
    /// is in the source screenshot's pixels, `source_width` wide. None when
    /// the crop would be most of the screen anyway.
    pub fn crop(&self, rendered: &Image, around: &[Point], source_width: u32) -> Option<Image> {
        let first = around.first()?;
        if source_width == 0 || rendered.width == 0 || rendered.height == 0 {
            return None;
        }
        let scale = rendered.width as f32 / source_width as f32;
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (first.x, first.y, first.x, first.y);
        for p in around {
            min_x = min_x.min(p.x);
            min_y = min_y.min(p.y);
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
        let (bw, bh) = (rendered.width as f32, rendered.height as f32);
        let minimum = 720f32.min(bw);
        let width = minimum.max((max_x - min_x) * scale * 1.6);
        let height = (minimum * 0.6).max((max_y - min_y) * scale * 1.6);
        let mid_x = (min_x + max_x) / 2.0 * scale;
        let mid_y = (min_y + max_y) / 2.0 * scale;
        let x = (mid_x - width / 2.0).max(0.0).min(bw - width.min(bw));
        let y = (mid_y - height / 2.0).max(0.0).min(bh - height.min(bh));
        let x0 = x.floor().max(0.0) as u32;
        let y0 = y.floor().max(0.0) as u32;
        let x1 = ((x + width).ceil() as u32).min(rendered.width);
        let y1 = ((y + height).ceil() as u32).min(rendered.height);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        // A crop that is nearly the whole screen adds nothing.
        if (x1 - x0) as f32 >= bw * 0.7 && (y1 - y0) as f32 >= bh * 0.7 {
            return None;
        }
        let row = rendered.width as usize * 4;
        let mut rgba = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 4) as usize);
        for y in y0..y1 {
            let start = y as usize * row + x0 as usize * 4;
            rgba.extend_from_slice(&rendered.rgba[start..start + (x1 - x0) as usize * 4]);
        }
        Some(Image { width: x1 - x0, height: y1 - y0, rgba })
    }

    /// Several screenshots in one image, two to a row, so they read as one
    /// attachment. One screenshot comes back as it is.
    pub fn contact_sheet(&self, images: &[Image]) -> Option<Image> {
        match images {
            [] => return None,
            [one] => return Some(one.clone()),
            _ => {}
        }
        let columns = 2;
        let cell_width = (SHEET_WIDTH - GUTTER * (columns + 1)) / columns;
        let cells: Vec<u32> = images
            .iter()
            .map(|i| ((cell_width as f32 * i.height as f32 / i.width.max(1) as f32).round() as u32).max(1))
            .collect();
        let rows: Vec<u32> = cells.chunks(columns as usize).map(|r| r.iter().copied().max().unwrap_or(0)).collect();
        let height = rows.iter().fold(GUTTER, |sum, row| sum + row + GUTTER);
        let mut sheet = Pixmap::new(SHEET_WIDTH, height)?;
        sheet.fill(Color::from_rgba8(18, 18, 20, 255));
        let mut top = GUTTER;
        for (row, row_height) in rows.iter().enumerate() {
            for column in 0..columns as usize {
                let index = row * columns as usize + column;
                let Some(image) = images.get(index) else { break };
                let Some(source) = to_pixmap(image) else { continue };
                let x = GUTTER + column as u32 * (cell_width + GUTTER);
                let sx = cell_width as f32 / image.width as f32;
                let sy = cells[index] as f32 / image.height as f32;
                draw_scaled(&mut sheet, &source, x as f32, top as f32, sx, sy);
            }
            top += row_height + GUTTER;
        }
        Some(from_pixmap(sheet))
    }

    /// "#1  0:15" in the top-left corner, or top-right when ink is there.
    fn draw_badge(&self, map: &mut Pixmap, text: &str, unit: f32, ink: &[Point]) {
        let Some((glyphs, bounds)) = self.layout(text, 15.0 * unit) else { return };
        let (pad_x, pad_y) = (10.0 * unit, 6.0 * unit);
        let box_w = bounds.width() + pad_x * 2.0;
        let box_h = bounds.height() + pad_y * 2.0;
        let margin = 14.0 * unit;
        let mut x = margin;
        let y = margin;
        let near = |p: &Point| p.x >= x - margin && p.x <= x + box_w + margin && p.y >= y - margin && p.y <= y + box_h + margin;
        if ink.iter().any(near) {
            x = map.width() as f32 - margin - box_w;
        }
        let r = box_h / 2.0;
        let mut pill = PathBuilder::new();
        if let Some(middle) = Rect::from_xywh(x + r, y, (box_w - box_h).max(0.0), box_h) {
            pill.push_rect(middle);
        }
        pill.push_circle(x + r, y + r, r);
        pill.push_circle(x + box_w - r, y + r, r);
        if let Some(path) = pill.finish() {
            map.fill_path(&path, &paint(20, 20, 23, 224), FillRule::Winding, Transform::identity(), None);
        }
        draw_glyphs(map, &glyphs, x + pad_x - bounds.min.x, y + pad_y - bounds.min.y);
    }

    fn draw_text_centered(&self, map: &mut Pixmap, text: &str, size: f32, cx: f32, cy: f32) {
        let Some((glyphs, bounds)) = self.layout(text, size) else { return };
        let dx = cx - (bounds.min.x + bounds.max.x) / 2.0;
        let dy = cy - (bounds.min.y + bounds.max.y) / 2.0;
        draw_glyphs(map, &glyphs, dx, dy);
    }

    /// Glyphs on a baseline at y = 0, and the box their ink covers.
    fn layout(&self, text: &str, size: f32) -> Option<(Vec<OutlinedGlyph>, ab_glyph::Rect)> {
        let font = self.font.as_ref()?;
        let scaled = font.as_scaled(PxScale::from(size));
        let mut x = 0.0;
        let mut previous = None;
        let mut glyphs = Vec::new();
        for c in text.chars() {
            let id = scaled.glyph_id(c);
            if let Some(p) = previous {
                x += scaled.kern(p, id);
            }
            let glyph = id.with_scale_and_position(PxScale::from(size), point(x, 0.0));
            x += scaled.h_advance(id);
            previous = Some(id);
            if let Some(outlined) = font.outline_glyph(glyph) {
                glyphs.push(outlined);
            }
        }
        let mut bounds = glyphs.first()?.px_bounds();
        for g in &glyphs[1..] {
            let b = g.px_bounds();
            bounds.min.x = bounds.min.x.min(b.min.x);
            bounds.min.y = bounds.min.y.min(b.min.y);
            bounds.max.x = bounds.max.x.max(b.max.x);
            bounds.max.y = bounds.max.y.max(b.max.y);
        }
        Some((glyphs, bounds))
    }
}

/// White glyphs, offset by `(dx, dy)`, blended onto an opaque pixmap.
fn draw_glyphs(map: &mut Pixmap, glyphs: &[OutlinedGlyph], dx: f32, dy: f32) {
    let (width, height) = (map.width() as i32, map.height() as i32);
    let data = map.data_mut();
    for glyph in glyphs {
        let b = glyph.px_bounds();
        let ox = (b.min.x + dx).round() as i32;
        let oy = (b.min.y + dy).round() as i32;
        glyph.draw(|gx, gy, coverage| {
            let (x, y) = (ox + gx as i32, oy + gy as i32);
            if x < 0 || y < 0 || x >= width || y >= height {
                return;
            }
            let at = (y as usize * width as usize + x as usize) * 4;
            let c = coverage.clamp(0.0, 1.0);
            for channel in &mut data[at..at + 3] {
                *channel = (*channel as f32 * (1.0 - c) + 255.0 * c).round() as u8;
            }
        });
    }
}

/// A smooth line through the points: quadratic curves between midpoints.
fn smooth_path(points: &[Point]) -> Option<Path> {
    let (first, rest) = points.split_first()?;
    if rest.is_empty() {
        return None;
    }
    let mut path = PathBuilder::new();
    path.move_to(first.x, first.y);
    if points.len() <= 2 {
        for p in rest {
            path.line_to(p.x, p.y);
        }
    } else {
        for pair in points.windows(2) {
            let (previous, point) = (pair[0], pair[1]);
            path.quad_to(previous.x, previous.y, (previous.x + point.x) / 2.0, (previous.y + point.y) / 2.0);
        }
        let last = points[points.len() - 1];
        path.line_to(last.x, last.y);
    }
    path.finish()
}

fn paint(r: u8, g: u8, b: u8, a: u8) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, a);
    paint.anti_alias = true;
    paint
}

fn fill_circle(map: &mut Pixmap, cx: f32, cy: f32, radius: f32, paint: &Paint) {
    if let Some(circle) = PathBuilder::from_circle(cx, cy, radius) {
        map.fill_path(&circle, paint, FillRule::Winding, Transform::identity(), None);
    }
}

fn draw_scaled(target: &mut Pixmap, source: &Pixmap, x: f32, y: f32, sx: f32, sy: f32) {
    let quality = PixmapPaint { quality: FilterQuality::Bicubic, ..Default::default() };
    target.draw_pixmap(0, 0, source.as_ref(), &quality, Transform::from_row(sx, 0.0, 0.0, sy, x, y), None);
}

/// Opaque images are the same premultiplied or not.
fn to_pixmap(image: &Image) -> Option<Pixmap> {
    Pixmap::from_vec(image.rgba.clone(), IntSize::from_wh(image.width, image.height)?)
}

fn from_pixmap(map: Pixmap) -> Image {
    let (width, height) = (map.width(), map.height());
    Image { width, height, rgba: map.take() }
}

/// PNG bytes, without the alpha channel (screenshots are opaque).
pub fn encode_png(image: &Image) -> Vec<u8> {
    let rgb: Vec<u8> = image.rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut data = Vec::new();
    let mut encoder = png::Encoder::new(&mut data, image.width, image.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    if let Ok(mut writer) = encoder.write_header() {
        let _ = writer.write_image_data(&rgb);
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::{MomentKind, Place, Stroke};
    use std::sync::Arc;

    fn gray(width: u32, height: u32) -> Image {
        Image { width, height, rgba: [128, 128, 128, 255].repeat((width * height) as usize) }
    }

    fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * image.width + x) * 4) as usize;
        image.rgba[at..at + 4].try_into().unwrap()
    }

    fn font() -> Option<Vec<u8>> {
        std::fs::read("C:\\Windows\\Fonts\\segoeuib.ttf").ok()
    }

    fn marked(width: u32, height: u32) -> Moment {
        let points = (0..=50).map(|i| Point { x: 200.0 + i as f32 * 20.0, y: 500.0 }).collect();
        Moment {
            kind: MomentKind::Marks,
            sample: 0,
            image: Arc::new(gray(width, height)),
            strokes: vec![Stroke { points, start_sample: 0, number: 1 }],
            click: Some(Point { x: 1500.0, y: 900.0 }),
            place: Place::default(),
        }
    }

    #[test]
    fn render_scales_down_and_draws_ink() {
        let renderer = Renderer::new(font());
        let out = renderer.render(&marked(2560, 1440), Some("#1  0:05"));
        assert_eq!((out.width, out.height), (2048, 1152));
        // The stroke runs along y = 500 * 0.8 = 400.
        let p = pixel(&out, 600, 400);
        assert!(p[0] > 200 && p[1] < 120 && p[2] < 120, "ink pixel {p:?}");
        // Away from everything stays gray.
        assert_eq!(pixel(&out, 1500, 1000), [128, 128, 128, 255]);
        if renderer.font.is_some() {
            // The badge darkens the top-left.
            assert!(pixel(&out, 30, 30)[0] < 60);
        }
    }

    #[test]
    fn render_keeps_small_screens_and_works_without_font() {
        let out = Renderer::new(None).render(&marked(1600, 900), Some("#1  0:05"));
        assert_eq!((out.width, out.height), (1600, 900));
        assert_eq!(pixel(&out, 30, 30), [128, 128, 128, 255]);
    }

    #[test]
    fn crop_around_ink() {
        let renderer = Renderer::new(None);
        let rendered = gray(2048, 1152);
        let crop = renderer.crop(&rendered, &[Point { x: 1000.0, y: 600.0 }, Point { x: 1100.0, y: 650.0 }], 2560).unwrap();
        assert_eq!(crop.width, 720);
        assert_eq!(crop.height, 432);
        assert_eq!(crop.rgba.len(), (720 * 432 * 4) as usize);
        // Ink across the whole screen: no crop.
        assert!(renderer.crop(&rendered, &[Point { x: 0.0, y: 0.0 }, Point { x: 2560.0, y: 1440.0 }], 2560).is_none());
        assert!(renderer.crop(&rendered, &[], 2560).is_none());
    }

    #[test]
    fn contact_sheet_two_per_row() {
        let renderer = Renderer::new(None);
        assert!(renderer.contact_sheet(&[]).is_none());
        let one = gray(100, 50);
        assert_eq!(renderer.contact_sheet(std::slice::from_ref(&one)).unwrap(), one);
        let sheet = renderer.contact_sheet(&[gray(1600, 900), gray(1600, 900), gray(1600, 900)]).unwrap();
        // Cells are 1188 wide, 668 tall; two rows.
        assert_eq!(sheet.width, 2400);
        assert_eq!(sheet.height, 8 + 668 + 8 + 668 + 8);
        assert_eq!(pixel(&sheet, 2, 2), [18, 18, 20, 255]);
        assert_eq!(pixel(&sheet, 100, 100), [128, 128, 128, 255]);
    }

    #[test]
    fn png_signature() {
        let data = encode_png(&gray(4, 4));
        assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n");
    }
}
