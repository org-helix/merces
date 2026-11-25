//! Pixel art in 3D: small models made of cubes ("voxels"), turned into one
//! mesh per model. Only the faces between a voxel and empty space are kept,
//! and each face carries a baked shade (lighter on top, darker underneath,
//! with a faint per-pixel variation), so the models keep a crisp pixel look
//! under any light.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// A colour in sRGB components, 0-1.
pub type Rgb = [f32; 3];

pub fn rgb(hex: u32) -> Rgb {
    [
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
    ]
}

/// White: the material's base colour tints it (used for the bodies, so their
/// colour can change without rebuilding the mesh).
pub const TINT: Rgb = [1.0, 1.0, 1.0];

#[derive(Default, Clone)]
pub struct Model {
    voxels: HashMap<IVec3, Rgb>,
}

impl Model {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, p: IVec3, c: Rgb) -> &mut Self {
        self.voxels.insert(p, c);
        self
    }

    /// Fill the box `min..max` (max exclusive).
    pub fn fill(&mut self, min: IVec3, max: IVec3, c: Rgb) -> &mut Self {
        for x in min.x..max.x {
            for y in min.y..max.y {
                for z in min.z..max.z {
                    self.voxels.insert(IVec3::new(x, y, z), c);
                }
            }
        }
        self
    }

    pub fn clear(&mut self, min: IVec3, max: IVec3) -> &mut Self {
        for x in min.x..max.x {
            for y in min.y..max.y {
                for z in min.z..max.z {
                    self.voxels.remove(&IVec3::new(x, y, z));
                }
            }
        }
        self
    }

    pub fn len(&self) -> usize {
        self.voxels.len()
    }

    /// Build the mesh. `unit` is the size of one voxel; voxel (0,0,0) spans
    /// `[0, unit]³` shifted by `-origin * unit`.
    pub fn mesh(&self, unit: f32, origin: Vec3) -> Mesh {
        const FACES: [(IVec3, [[f32; 3]; 4], f32); 6] = [
            // normal, corners (counter-clockwise seen from outside), shade
            (
                IVec3::Y,
                [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]],
                1.0,
            ),
            (
                IVec3::NEG_Y,
                [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
                0.62,
            ),
            (
                IVec3::X,
                [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]],
                0.84,
            ),
            (
                IVec3::NEG_X,
                [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
                0.84,
            ),
            (
                IVec3::Z,
                [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
                0.93,
            ),
            (
                IVec3::NEG_Z,
                [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
                0.76,
            ),
        ];
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut colors = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        // Stable order so the mesh is the same on every run.
        let mut keys: Vec<&IVec3> = self.voxels.keys().collect();
        keys.sort_by_key(|p| (p.x, p.y, p.z));
        for p in keys {
            let c = self.voxels[p];
            // Faint per-pixel variation: the "hand-painted" pixel feel.
            let h = (p.x.wrapping_mul(73_856_093)
                ^ p.y.wrapping_mul(19_349_663)
                ^ p.z.wrapping_mul(83_492_791)) as u32;
            let jitter = 0.965 + (h % 1000) as f32 / 1000.0 * 0.07;
            for (n, corners, shade) in FACES {
                if self.voxels.contains_key(&(*p + n)) {
                    continue;
                }
                let base = positions.len() as u32;
                for corner in corners {
                    let v = (p.as_vec3() + Vec3::from(corner) - origin) * unit;
                    positions.push(v.to_array());
                    normals.push(n.as_vec3().to_array());
                    let k = shade * jitter;
                    let lin = Color::srgb(c[0] * k, c[1] * k, c[2] * k).to_linear();
                    colors.push([lin.red, lin.green, lin.blue, 1.0]);
                }
                indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_faces_are_culled() {
        let mut m = Model::new();
        m.set(IVec3::ZERO, TINT);
        let one = m.mesh(1.0, Vec3::ZERO);
        assert_eq!(one.count_vertices(), 24); // 6 faces
        m.set(IVec3::X, TINT);
        let two = m.mesh(1.0, Vec3::ZERO);
        assert_eq!(two.count_vertices(), 40); // 10 faces: the shared pair is gone
    }

    #[test]
    fn fill_and_clear_are_exclusive_of_max() {
        let mut m = Model::new();
        m.fill(IVec3::ZERO, IVec3::new(3, 2, 1), TINT);
        assert_eq!(m.len(), 6);
        m.clear(IVec3::ZERO, IVec3::new(1, 2, 1));
        assert_eq!(m.len(), 4);
    }
}
