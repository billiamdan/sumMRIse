//! Paints what egui tessellated into a [`Picture`], in software: the export
//! pass of "with annotations" (see `annotated`) needs no window and no GPU, so
//! it never shows on screen and is tested like any other function. It does what
//! egui's renderers do: premultiplied alpha, blended in gamma space, clipped to
//! the rectangle of each primitive, one sample at the centre of each pixel
//! (egui draws anti-aliasing into its meshes).

use std::collections::HashMap;

use egui::epaint::{ImageData, ImageDelta, Primitive, Vertex};
use egui::{ClippedPrimitive, Color32, TextureFilter, TextureId, TexturesDelta};
use summrise_core::export::Picture;

struct Texture {
    width: usize,
    height: usize,
    pixels: Vec<Color32>,
    linear: bool,
}

impl Texture {
    /// The premultiplied colour at (`u`, `v`), 0 to 255 per channel.
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let texel = |x: isize, y: isize| {
            let x = x.clamp(0, self.width as isize - 1) as usize;
            let y = y.clamp(0, self.height as isize - 1) as usize;
            self.pixels[y * self.width + x].to_array().map(f32::from)
        };
        let (x, y) = (u * self.width as f32, v * self.height as f32);
        if !self.linear {
            return texel(x.floor() as isize, y.floor() as isize);
        }
        let (x, y) = (x - 0.5, y - 0.5);
        let (left, top) = (x.floor(), y.floor());
        let (fx, fy) = (x - left, y - top);
        let (left, top) = (left as isize, top as isize);
        let (a, b) = (texel(left, top), texel(left + 1, top));
        let (c, d) = (texel(left, top + 1), texel(left + 1, top + 1));
        std::array::from_fn(|i| {
            let upper = a[i] + (b[i] - a[i]) * fx;
            let lower = c[i] + (d[i] - c[i]) * fx;
            upper + (lower - upper) * fy
        })
    }
}

/// The textures of one export pass.
#[derive(Default)]
pub(super) struct Raster {
    textures: HashMap<TextureId, Texture>,
}

impl Raster {
    /// Takes over what egui says about its textures.
    pub(super) fn apply(&mut self, delta: &TexturesDelta) {
        for (id, deltas) in &delta.set {
            for delta in deltas {
                self.set(*id, delta);
            }
        }
    }

    fn set(&mut self, id: TextureId, delta: &ImageDelta) {
        let ImageData::Color(image) = &delta.image;
        let linear = delta.options.magnification == TextureFilter::Linear;
        let [width, height] = image.size;
        match delta.pos {
            None => {
                self.textures.insert(id, Texture { width, height, pixels: image.pixels.clone(), linear });
            }
            Some([x, y]) => {
                let Some(texture) = self.textures.get_mut(&id) else { return };
                for row in 0..height.min(texture.height.saturating_sub(y)) {
                    let columns = width.min(texture.width.saturating_sub(x));
                    let target = (y + row) * texture.width + x;
                    texture.pixels[target..target + columns]
                        .copy_from_slice(&image.pixels[row * width..row * width + columns]);
                }
            }
        }
    }

    /// Paints `primitives` on black into a picture of `width` × `height`
    /// pixels, at `pixels_per_point`.
    pub(super) fn paint(
        &self,
        primitives: &[ClippedPrimitive],
        width: usize,
        height: usize,
        pixels_per_point: f32,
    ) -> Picture {
        let mut canvas = vec![[0f32; 3]; width * height];
        for primitive in primitives {
            let Primitive::Mesh(mesh) = &primitive.primitive else { continue };
            let Some(texture) = self.textures.get(&mesh.texture_id) else { continue };
            let clip = primitive.clip_rect;
            let scale = pixels_per_point;
            let bounds = [
                (clip.left() * scale).round().max(0.0) as usize,
                (clip.top() * scale).round().max(0.0) as usize,
                ((clip.right() * scale).round().max(0.0) as usize).min(width),
                ((clip.bottom() * scale).round().max(0.0) as usize).min(height),
            ];
            for triangle in mesh.indices.as_chunks::<3>().0 {
                let corner = |i: usize| &mesh.vertices[triangle[i] as usize];
                fill_triangle(&mut canvas, width, bounds, [corner(0), corner(1), corner(2)], texture, scale);
            }
        }
        let rgb = canvas.iter().flat_map(|p| p.map(|v| (v + 0.5).clamp(0.0, 255.0) as u8)).collect();
        Picture { width, height, rgb }
    }
}

/// Twice the signed area of the triangle (`a`, `b`, `p`): which side of the line
/// from `a` to `b` the point `p` is on.
fn side(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

/// Whether a pixel exactly on the edge from `a` to `b` belongs to this
/// triangle. Of two triangles that share an edge exactly one takes it (they run
/// along it in opposite directions), so that translucent shapes have no seams.
fn owns_edge(a: [f64; 2], b: [f64; 2]) -> bool {
    b[1] > a[1] || (b[1] == a[1] && b[0] > a[0])
}

fn fill_triangle(
    canvas: &mut [[f32; 3]],
    width: usize,
    [left, top, right, bottom]: [usize; 4],
    corners: [&Vertex; 3],
    texture: &Texture,
    scale: f32,
) {
    let at = |v: &Vertex| [f64::from(v.pos.x) * f64::from(scale), f64::from(v.pos.y) * f64::from(scale)];
    let [mut v0, mut v1, v2] = corners;
    let area = side(at(v0), at(v1), at(v2));
    if area == 0.0 {
        return;
    }
    if area < 0.0 {
        std::mem::swap(&mut v0, &mut v1);
    }
    let area = area.abs();
    let [a, b, c] = [at(v0), at(v1), at(v2)];
    let colours = [v0, v1, v2].map(|v| v.color.to_array().map(f32::from));

    let least = |i: usize| a[i].min(b[i]).min(c[i]).floor().max(0.0) as usize;
    let most = |i: usize| (a[i].max(b[i]).max(c[i]).ceil().max(0.0) as usize) + 1;
    let rows = top.max(least(1))..bottom.min(most(1));
    let columns = left.max(least(0))..right.min(most(0));
    let owned = [owns_edge(b, c), owns_edge(c, a), owns_edge(a, b)];
    for y in rows {
        let row_at = f64::from(y as u32) + 0.5;
        for x in columns.clone() {
            let p = [f64::from(x as u32) + 0.5, row_at];
            let weights = [side(b, c, p), side(c, a, p), side(a, b, p)];
            if !weights.iter().zip(&owned).all(|(w, owned)| *w > 0.0 || (*w == 0.0 && *owned)) {
                continue;
            }
            let [w0, w1, w2] = weights.map(|w| (w / area) as f32);
            let u = v0.uv.x * w0 + v1.uv.x * w1 + v2.uv.x * w2;
            let v = v0.uv.y * w0 + v1.uv.y * w1 + v2.uv.y * w2;
            let sampled = texture.sample(u, v);
            let source: [f32; 4] = std::array::from_fn(|i| {
                sampled[i] * (colours[0][i] * w0 + colours[1][i] * w1 + colours[2][i] * w2) / 255.0
            });
            let keep = 1.0 - source[3] / 255.0;
            let pixel = &mut canvas[y * width + x];
            for channel in 0..3 {
                pixel[channel] = source[channel] + pixel[channel] * keep;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui::epaint::{ClippedShape, Mesh};
    use egui::{ColorImage, Pos2, Rect, Shape, TextureOptions, pos2, vec2};

    use super::*;

    /// A raster whose default texture is one white pixel, as egui's font atlas
    /// has at its corner.
    fn raster() -> Raster {
        let mut raster = Raster::default();
        let white = ColorImage::filled([1, 1], Color32::WHITE);
        raster.set(TextureId::default(), &ImageDelta::full(ImageData::Color(Arc::new(white)), TextureOptions::NEAREST));
        raster
    }

    fn primitive(mesh: Mesh, clip: Rect) -> ClippedPrimitive {
        ClippedPrimitive { clip_rect: clip, primitive: Primitive::Mesh(mesh) }
    }

    fn rect_mesh(rect: Rect, color: Color32) -> Mesh {
        let mut mesh = Mesh::default();
        mesh.add_colored_rect(rect, color);
        mesh
    }

    const ALL: Rect = Rect { min: Pos2::ZERO, max: pos2(100.0, 100.0) };

    #[test]
    fn an_opaque_rectangle_covers_exactly_its_pixels() {
        let rect = Rect::from_min_size(pos2(2.0, 1.0), vec2(3.0, 2.0));
        let mesh = rect_mesh(rect, Color32::from_rgb(200, 100, 50));
        let picture = raster().paint(&[primitive(mesh, ALL)], 8, 4, 1.0);
        for y in 0..4 {
            for x in 0..8 {
                let inside = (2..5).contains(&x) && (1..3).contains(&y);
                let expected = if inside { [200, 100, 50] } else { [0, 0, 0] };
                assert_eq!(picture.pixel(x, y), expected, "({x}, {y})");
            }
        }
    }

    /// The two triangles of a square share a diagonal through pixel centres;
    /// a translucent square must not show it as a darker or brighter line.
    #[test]
    fn a_translucent_square_has_no_seam() {
        let square = Rect::from_min_size(Pos2::ZERO, vec2(8.0, 8.0));
        let mut meshes = vec![primitive(rect_mesh(square, Color32::WHITE), ALL)];
        meshes.push(primitive(rect_mesh(square, Color32::from_black_alpha(128)), ALL));
        let picture = raster().paint(&meshes, 8, 8, 1.0);
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(picture.pixel(x, y), [127; 3], "({x}, {y})");
            }
        }
    }

    #[test]
    fn points_scale_to_pixels_and_clipping_cuts() {
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(4.0, 1.0));
        let clip = Rect::from_min_size(Pos2::ZERO, vec2(2.0, 1.0));
        let picture = raster().paint(&[primitive(rect_mesh(rect, Color32::WHITE), clip)], 8, 2, 2.0);
        assert_eq!(picture.pixel(3, 1), [255; 3], "2 points are 4 pixels");
        assert_eq!(picture.pixel(4, 0), [0; 3], "cut off by the clip rectangle");
    }

    #[test]
    fn textures_are_sampled_like_the_renderers_do() {
        let image = ColorImage::from_rgb([2, 1], &[0, 0, 0, 200, 200, 200]);
        let id = TextureId::User(7);
        for (options, expected) in
            [(TextureOptions::NEAREST, [0, 0, 200, 200]), (TextureOptions::LINEAR, [0, 50, 150, 200])]
        {
            let mut raster = raster();
            raster.set(id, &ImageDelta::full(ImageData::Color(Arc::new(image.clone())), options));
            let mut mesh = Mesh::with_texture(id);
            let whole = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
            mesh.add_rect_with_uv(Rect::from_min_size(Pos2::ZERO, vec2(4.0, 1.0)), whole, Color32::WHITE);
            let picture = raster.paint(&[primitive(mesh, ALL)], 4, 1, 1.0);
            let row: Vec<u8> = (0..4).map(|x| picture.pixel(x, 0)[0]).collect();
            assert_eq!(row, expected, "{options:?}");
        }
    }

    #[test]
    fn part_of_a_texture_can_change() {
        let mut raster = raster();
        let id = TextureId::User(1);
        let image = ColorImage::filled([4, 4], Color32::BLACK);
        raster.set(id, &ImageDelta::full(ImageData::Color(Arc::new(image)), TextureOptions::NEAREST));
        let patch = ColorImage::filled([2, 1], Color32::WHITE);
        raster.set(id, &ImageDelta::partial([1, 2], ImageData::Color(Arc::new(patch)), TextureOptions::NEAREST));
        let texture = &raster.textures[&id];
        let at = |x: usize, y: usize| texture.pixels[y * 4 + x];
        assert_eq!((at(1, 2), at(2, 2)), (Color32::WHITE, Color32::WHITE));
        assert_eq!((at(0, 2), at(3, 2), at(1, 1)), (Color32::BLACK, Color32::BLACK, Color32::BLACK));
    }

    /// Text of a real pass reaches the picture through the font atlas.
    #[test]
    fn text_comes_through_the_font_atlas() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.painter().text(
                pos2(10.0, 10.0),
                egui::Align2::LEFT_TOP,
                "Not for diagnostic use",
                egui::FontId::proportional(14.0),
                Color32::WHITE,
            );
        });
        let mut raster = Raster::default();
        raster.apply(&output.textures_delta);
        output.textures_delta.clear();
        let shapes: Vec<ClippedShape> = std::mem::take(&mut output.shapes);
        assert!(shapes.iter().any(|shape| matches!(shape.shape, Shape::Text(_))));
        let primitives = ctx.tessellate(shapes, output.pixels_per_point);
        let picture = raster.paint(&primitives, 240, 40, output.pixels_per_point);
        let lit = picture.rgb.iter().filter(|v| **v > 128).count();
        assert!(lit > 100, "{lit} bright values: the text is there");
        assert_eq!(picture.pixel(200, 35), [0; 3], "and only there");
    }
}
