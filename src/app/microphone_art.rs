use eframe::egui::{Color32, Mesh, Painter, Pos2};

pub(super) const LOGO_BLUE: Color32 = Color32::from_rgb(7, 87, 239);
const FACET_ALPHA: u8 = 51;
// An irregular faceted surface, mirrored by the web microphone SVG.
const POINTS: [(f32, f32); 14] = [
    (-0.85, -0.36),
    (-0.32, -0.92),
    (0.28, -0.92),
    (0.82, -0.47),
    (0.96, 0.10),
    (0.65, 0.65),
    (0.33, 0.86),
    (-0.40, 0.88),
    (-0.70, 0.33),
    (-0.28, -0.18),
    (0.18, -0.36),
    (0.48, 0.02),
    (0.07, 0.20),
    (-0.06, 0.53),
];
const FACETS: [([usize; 3], bool); 12] = [
    ([0, 1, 9], true),
    ([1, 2, 10], false),
    ([2, 3, 11], true),
    ([2, 11, 10], false),
    ([4, 5, 11], false),
    ([5, 6, 12], true),
    ([6, 13, 12], false),
    ([6, 7, 13], true),
    ([8, 9, 13], true),
    ([8, 0, 9], false),
    ([9, 12, 13], false),
    ([10, 11, 12], true),
];

pub(super) fn paint_facets(painter: &Painter, center: Pos2, radius: f32, active: bool) {
    painter.add(facet_mesh(center, radius, active));
}

fn facet_mesh(center: Pos2, radius: f32, active: bool) -> Mesh {
    let mut mesh = Mesh::default();
    for (indices, light) in FACETS {
        let color = facet_color(active, light);
        let start = mesh.vertices.len() as u32;
        for index in indices {
            let (x, y) = POINTS[index];
            mesh.colored_vertex(center + eframe::egui::vec2(x * radius, y * radius), color);
        }
        mesh.add_triangle(start, start + 1, start + 2);
    }
    mesh
}

fn facet_color(active: bool, light: bool) -> Color32 {
    let (red, green, blue) = match (active, light) {
        (false, true) => (74, 135, 255),
        (false, false) => (0, 49, 191),
        (true, true) => (227, 129, 143),
        (true, false) => (119, 25, 42),
    };
    Color32::from_rgba_unmultiplied(red, green, blue, FACET_ALPHA)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gem_facets_are_inside_the_disc_and_twenty_percent_opaque() {
        let center = Pos2::new(125.0, 80.0);
        let radius = 60.0;
        for active in [false, true] {
            let mesh = facet_mesh(center, radius, active);
            assert!(!mesh.indices.is_empty());
            assert!(mesh
                .indices
                .iter()
                .all(|index| (*index as usize) < mesh.vertices.len()));
            for vertex in &mesh.vertices {
                assert!(vertex.pos.distance(center) <= radius + 0.001);
                assert_eq!(vertex.color.a(), 51);
            }
        }
        assert_eq!(LOGO_BLUE.to_array(), [7, 87, 239, 255]);
    }
}
