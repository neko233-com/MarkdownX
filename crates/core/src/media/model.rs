//! Model preview: OBJ, STL and glTF/GLB parsing plus a small software
//! rasterizer. This is a previewer, not an exporter; the source is never
//! modified and unsupported primitives are skipped with an error.
use std::path::{Path, PathBuf};
use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
use serde_json::Value;

const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_VERTICES: usize = 1_000_000;
const MAX_TRIANGLES: usize = 500_000;
const MAX_RENDER_SIDE: u32 = 2048;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    pub width: u32,
    pub height: u32,
    /// RGBA, transparent background.
    pub rgba: Vec<u8>,
}

impl Mesh {
    fn push_triangle(&mut self, a: u32, b: u32, c: u32) -> Result<()> {
        ensure!((a as usize) < self.positions.len() && (b as usize) < self.positions.len() && (c as usize) < self.positions.len(), "model triangle references a missing vertex");
        ensure!(self.triangles.len() < MAX_TRIANGLES, "model has too many triangles");
        self.triangles.push([a, b, c]);
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        ensure!(!self.positions.is_empty() && !self.triangles.is_empty(), "model has no renderable triangles");
        ensure!(self.positions.len() <= MAX_VERTICES, "model has too many vertices");
        ensure!(self.triangles.len() <= MAX_TRIANGLES, "model has too many triangles");
        ensure!(self.positions.iter().all(|p| p.iter().all(|value| value.is_finite())), "model contains non-finite coordinates");
        Ok(())
    }
}

pub fn load(path: &Path) -> Result<Mesh> {
    let meta = std::fs::metadata(path).context("cannot inspect model")?;
    ensure!(meta.is_file(), "model is not a regular file");
    ensure!(meta.len() <= MAX_FILE_BYTES, "model file is too large");
    let extension = path.extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    let mesh = match extension.as_str() {
        "obj" => parse_obj(&std::fs::read_to_string(path).context("cannot read OBJ")?),
        "stl" => parse_stl(&std::fs::read(path).context("cannot read STL")?),
        "gltf" => parse_gltf(&std::fs::read(path).context("cannot read glTF")?, Some(path), None),
        "glb" => parse_glb(&std::fs::read(path).context("cannot read GLB")?, path),
        _ => bail!("unsupported model format"),
    }?;
    mesh.validate()?;
    Ok(mesh)
}

fn parse_obj(text: &str) -> Result<Mesh> {
    let mut mesh = Mesh::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("v") => {
                let mut value = [0.0f32; 3];
                for item in &mut value {
                    *item = parts.next().context("OBJ vertex needs three coordinates")?.parse().context("invalid OBJ coordinate")?;
                }
                ensure!(mesh.positions.len() < MAX_VERTICES, "model has too many vertices");
                mesh.positions.push(value);
            }
            Some("f") => {
                let mut indices = vec![];
                for token in parts {
                    let index = token.split('/').next().unwrap_or("");
                    let index: i64 = index.parse().context("invalid OBJ face index")?;
                    if index == 0 { bail!("OBJ face index 0 is invalid"); }
                    let index = if index < 0 { mesh.positions.len() as i64 + index } else { index - 1 };
                    ensure!(index >= 0, "OBJ face index is out of range");
                    indices.push(index as u32);
                }
                ensure!(indices.len() >= 3, "OBJ face needs at least three vertices");
                for window in 1..indices.len() - 1 { mesh.push_triangle(indices[0], indices[window], indices[window + 1])?; }
            }
            _ => {}
        }
    }
    Ok(mesh)
}

fn parse_stl(bytes: &[u8]) -> Result<Mesh> {
    if bytes.len() >= 84 {
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        if count > 0 && 84 + count.saturating_mul(50) == bytes.len() {
            ensure!(count <= MAX_TRIANGLES, "model has too many triangles");
            let mut mesh = Mesh { positions: Vec::with_capacity(count * 3), triangles: Vec::with_capacity(count) };
            for index in 0..count {
                let base = 84 + index * 50 + 12;
                let mut triangle = [0u32; 3];
                for (vertex, slot) in triangle.iter_mut().enumerate() {
                    let start = base + vertex * 12;
                    let read = |offset: usize| f32::from_le_bytes(bytes[start + offset..start + offset + 4].try_into().unwrap());
                    mesh.positions.push([read(0), read(4), read(8)]);
                    *slot = mesh.positions.len() as u32 - 1;
                }
                mesh.triangles.push(triangle);
            }
            return Ok(mesh);
        }
    }
    parse_ascii_stl(std::str::from_utf8(bytes).context("invalid STL text")?)
}

fn parse_ascii_stl(text: &str) -> Result<Mesh> {
    let mut mesh = Mesh::default();
    let mut triangle = vec![];
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("vertex") { continue; }
        let mut value = [0.0f32; 3];
        for item in &mut value { *item = parts.next().context("STL vertex needs three coordinates")?.parse().context("invalid STL coordinate")?; }
        ensure!(mesh.positions.len() < MAX_VERTICES, "model has too many vertices");
        mesh.positions.push(value);
        triangle.push(mesh.positions.len() as u32 - 1);
        if triangle.len() == 3 {
            mesh.push_triangle(triangle[0], triangle[1], triangle[2])?;
            triangle.clear();
        }
    }
    Ok(mesh)
}

fn parse_glb(bytes: &[u8], path: &Path) -> Result<Mesh> {
    ensure!(bytes.len() >= 12 && &bytes[..4] == b"glTF", "invalid GLB header");
    ensure!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) == 2, "unsupported GLB version");
    let mut offset = 12;
    let mut json = None;
    let mut binary = None;
    while offset + 8 <= bytes.len() {
        let length = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let kind = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
        ensure!(length <= bytes.len() - offset - 8, "invalid GLB chunk length");
        let chunk = &bytes[offset + 8..offset + 8 + length];
        match kind {
            0x4E4F534A => json = Some(chunk),
            0x004E4942 => binary = Some(chunk),
            _ => {}
        }
        offset += 8 + length;
    }
    parse_gltf(json.context("GLB has no JSON chunk")?, Some(path), binary)
}

fn parse_gltf(json: &[u8], path: Option<&Path>, glb_binary: Option<&[u8]>) -> Result<Mesh> {
    let root: Value = serde_json::from_slice(json).context("invalid glTF JSON")?;
    let buffers = root.get("buffers").and_then(Value::as_array).context("glTF has no buffers")?;
    let mut resolved = vec![];
    for (index, buffer) in buffers.iter().enumerate() {
        let uri = buffer.get("uri").and_then(Value::as_str);
        let bytes = match uri {
            Some(uri) if uri.starts_with("data:") => decode_data_uri(uri)?,
            Some(uri) => {
                let base = path.and_then(Path::parent).context("glTF buffer needs a base directory")?;
                read_relative(base, uri)?
            }
            None if index == 0 => glb_binary.context("glTF buffer has no data")?.to_vec(),
            None => bail!("glTF buffer has no data"),
        };
        ensure!(bytes.len() as u64 <= MAX_FILE_BYTES, "model buffer is too large");
        resolved.push(bytes);
    }
    let views = root.get("bufferViews").and_then(Value::as_array).context("glTF has no bufferViews")?;
    let accessors = root.get("accessors").and_then(Value::as_array).context("glTF has no accessors")?;
    let mut mesh = Mesh::default();
    let nodes = root.get("nodes").and_then(Value::as_array);
    if let Some(nodes) = nodes {
        let scene = root.get("scene").and_then(Value::as_u64).and_then(|index| root.get("scenes").and_then(Value::as_array).and_then(|scenes| scenes.get(index as usize)));
        let roots = scene.and_then(|scene| scene.get("nodes")).and_then(Value::as_array).map(|nodes| nodes.iter().filter_map(Value::as_u64).collect::<Vec<_>>())
            .unwrap_or_else(|| {
                // No scene: every node that is not another node's child is a root.
                let children = nodes.iter().flat_map(|node| node.get("children").and_then(Value::as_array).into_iter().flat_map(|list| list.iter().filter_map(Value::as_u64))).collect::<std::collections::BTreeSet<_>>();
                (0..nodes.len() as u64).filter(|index| !children.contains(index)).collect()
            });
        let mut stack = roots.into_iter().map(|index| (index, identity())).collect::<Vec<_>>();
        let mut visited = 0usize;
        while let Some((index, parent)) = stack.pop() {
            visited += 1;
            ensure!(visited <= nodes.len().max(1) * 16, "glTF node graph is too deep");
            let node = &nodes[index as usize];
            let transform = multiply(&parent, &node_transform(node)?);
            if let Some(mesh_index) = node.get("mesh").and_then(Value::as_u64) {
                add_gltf_mesh(&mut mesh, &root, &resolved, views, accessors, mesh_index as usize, &transform)?;
            }
            if let Some(children) = node.get("children").and_then(Value::as_array) {
                for child in children.iter().filter_map(Value::as_u64) { stack.push((child, transform)); }
            }
        }
    } else {
        let count = root.get("meshes").and_then(Value::as_array).map_or(0, Vec::len);
        for index in 0..count { add_gltf_mesh(&mut mesh, &root, &resolved, views, accessors, index, &identity())?; }
    }
    Ok(mesh)
}

fn decode_data_uri(uri: &str) -> Result<Vec<u8>> {
    let (_, data) = uri.split_once(',').context("invalid glTF data URI")?;
    ensure!(uri[..uri.find(',').unwrap_or(0)].contains("base64"), "only base64 glTF data URIs are supported");
    base64::engine::general_purpose::STANDARD.decode(data).context("invalid glTF base64 data")
}

fn read_relative(base: &Path, uri: &str) -> Result<Vec<u8>> {
    let relative = PathBuf::from(uri.replace('\\', "/"));
    ensure!(!relative.is_absolute() && relative.components().all(|part| !matches!(part, std::path::Component::ParentDir)), "glTF buffer path escapes its folder");
    let root = base.canonicalize().context("glTF folder unavailable")?;
    let path = root.join(relative).canonicalize().context("glTF buffer unavailable")?;
    ensure!(path.starts_with(&root) && path.is_file(), "glTF buffer is outside its folder");
    let meta = std::fs::metadata(&path)?;
    ensure!(meta.len() <= MAX_FILE_BYTES, "model buffer is too large");
    std::fs::read(path).context("cannot read glTF buffer")
}

fn add_gltf_mesh(mesh: &mut Mesh, root: &Value, buffers: &[Vec<u8>], views: &[Value], accessors: &[Value], index: usize, transform: &[[f32; 4]; 4]) -> Result<()> {
    let primitives = root.get("meshes").and_then(Value::as_array).and_then(|meshes| meshes.get(index)).and_then(|mesh| mesh.get("primitives")).and_then(Value::as_array).context("glTF mesh has no primitives")?;
    for primitive in primitives {
        let mode = primitive.get("mode").and_then(Value::as_u64).unwrap_or(4);
        if mode != 4 { continue; }
        let attributes = primitive.get("attributes").context("glTF primitive has no attributes")?;
        let position_accessor = attributes.get("POSITION").and_then(Value::as_u64).context("glTF primitive has no POSITION")? as usize;
        let positions = read_accessor_vec3(buffers, views, accessors, position_accessor)?;
        ensure!(mesh.positions.len() + positions.len() <= MAX_VERTICES, "model has too many vertices");
        let base = mesh.positions.len() as u32;
        mesh.positions.extend(positions.into_iter().map(|position| transform_point(transform, position)));
        let indices = match primitive.get("indices").and_then(Value::as_u64) {
            Some(accessor) => read_accessor_indices(buffers, views, accessors, accessor as usize)?,
            None => (0..(mesh.positions.len() as u32 - base)).collect(),
        };
        ensure!(indices.len() % 3 == 0, "glTF triangle indices are incomplete");
        for triangle in indices.chunks(3) {
            mesh.push_triangle(base + triangle[0], base + triangle[1], base + triangle[2])?;
        }
    }
    Ok(())
}

fn accessor_data<'a>(buffers: &'a [Vec<u8>], views: &[Value], accessors: &[Value], index: usize) -> Result<(&'a [u8], usize, usize)> {
    let accessor = accessors.get(index).context("glTF accessor is missing")?;
    let view_index = accessor.get("bufferView").and_then(Value::as_u64).context("glTF accessor has no bufferView")? as usize;
    let view = views.get(view_index).context("glTF bufferView is missing")?;
    let buffer_index = view.get("buffer").and_then(Value::as_u64).context("glTF bufferView has no buffer")? as usize;
    let buffer = buffers.get(buffer_index).context("glTF buffer is missing")?;
    let view_offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let accessor_offset = accessor.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let stride = view.get("byteStride").and_then(Value::as_u64).map(|value| value as usize);
    let start = view_offset.checked_add(accessor_offset).context("glTF accessor offset overflow")?;
    ensure!(start <= buffer.len(), "glTF accessor is out of range");
    Ok((buffer, start, stride.unwrap_or(0)))
}

fn read_accessor_vec3(buffers: &[Vec<u8>], views: &[Value], accessors: &[Value], index: usize) -> Result<Vec<[f32; 3]>> {
    let accessor = accessors.get(index).context("glTF accessor is missing")?;
    ensure!(accessor.get("type").and_then(Value::as_str) == Some("VEC3"), "unsupported glTF POSITION type");
    ensure!(accessor.get("componentType").and_then(Value::as_u64) == Some(5126), "unsupported glTF POSITION component");
    let count = accessor.get("count").and_then(Value::as_u64).context("glTF accessor has no count")? as usize;
    ensure!(count <= MAX_VERTICES, "model has too many vertices");
    let (buffer, start, stride) = accessor_data(buffers, views, accessors, index)?;
    let stride = if stride == 0 { 12 } else { stride };
    ensure!(stride >= 12, "invalid glTF vertex stride");
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let offset = start + index * stride;
        ensure!(offset + 12 <= buffer.len(), "glTF vertex is out of range");
        let read = |at: usize| f32::from_le_bytes(buffer[at..at + 4].try_into().unwrap());
        out.push([read(offset), read(offset + 4), read(offset + 8)]);
    }
    Ok(out)
}

fn read_accessor_indices(buffers: &[Vec<u8>], views: &[Value], accessors: &[Value], index: usize) -> Result<Vec<u32>> {
    let accessor = accessors.get(index).context("glTF accessor is missing")?;
    ensure!(accessor.get("type").and_then(Value::as_str) == Some("SCALAR"), "unsupported glTF index type");
    let component = accessor.get("componentType").and_then(Value::as_u64).context("glTF index accessor has no componentType")?;
    let size = match component { 5121 => 1, 5123 => 2, 5125 => 4, _ => bail!("unsupported glTF index component") };
    let count = accessor.get("count").and_then(Value::as_u64).context("glTF accessor has no count")? as usize;
    ensure!(count <= MAX_TRIANGLES * 3, "model has too many indices");
    let (buffer, start, stride) = accessor_data(buffers, views, accessors, index)?;
    let stride = if stride == 0 { size } else { stride };
    ensure!(stride >= size, "invalid glTF index stride");
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let offset = start + index * stride;
        ensure!(offset + size <= buffer.len(), "glTF index is out of range");
        out.push(match size {
            1 => buffer[offset] as u32,
            2 => u16::from_le_bytes(buffer[offset..offset + 2].try_into().unwrap()) as u32,
            _ => u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap()),
        });
    }
    Ok(out)
}

type Matrix = [[f32; 4]; 4];

fn identity() -> Matrix { [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]] }

fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [[0.0; 4]; 4];
    for row in 0..4 { for column in 0..4 { out[row][column] = (0..4).map(|k| a[row][k] * b[k][column]).sum(); } }
    out
}

fn node_transform(node: &Value) -> Result<Matrix> {
    if let Some(matrix) = node.get("matrix").and_then(Value::as_array) {
        ensure!(matrix.len() == 16, "invalid glTF node matrix");
        let mut out = [[0.0; 4]; 4];
        for (index, value) in matrix.iter().enumerate() {
            out[index % 4][index / 4] = value.as_f64().context("invalid glTF node matrix")? as f32;
        }
        return Ok(out);
    }
    let translation = node.get("translation").and_then(Value::as_array).map(|values| values.iter().filter_map(Value::as_f64).map(|v| v as f32).collect::<Vec<_>>()).unwrap_or_default();
    let scale = node.get("scale").and_then(Value::as_array).map(|values| values.iter().filter_map(Value::as_f64).map(|v| v as f32).collect::<Vec<_>>()).unwrap_or_default();
    let rotation = node.get("rotation").and_then(Value::as_array).map(|values| values.iter().filter_map(Value::as_f64).map(|v| v as f32).collect::<Vec<_>>()).unwrap_or_default();
    let (tx, ty, tz) = (translation.first().copied().unwrap_or(0.0), translation.get(1).copied().unwrap_or(0.0), translation.get(2).copied().unwrap_or(0.0));
    let (sx, sy, sz) = (scale.first().copied().unwrap_or(1.0), scale.get(1).copied().unwrap_or(1.0), scale.get(2).copied().unwrap_or(1.0));
    let (x, y, z, w) = (rotation.first().copied().unwrap_or(0.0), rotation.get(1).copied().unwrap_or(0.0), rotation.get(2).copied().unwrap_or(0.0), rotation.get(3).copied().unwrap_or(1.0));
    let (xx, yy, zz) = (x * x, y * y, z * z);
    let (xy, xz, yz, wx, wy, wz) = (x * y, x * z, y * z, w * x, w * y, w * z);
    Ok([
        [(1. - 2. * (yy + zz)) * sx, 2. * (xy - wz) * sy, 2. * (xz + wy) * sz, tx],
        [2. * (xy + wz) * sx, (1. - 2. * (xx + zz)) * sy, 2. * (yz - wx) * sz, ty],
        [2. * (xz - wy) * sx, 2. * (yz + wx) * sy, (1. - 2. * (xx + yy)) * sz, tz],
        [0., 0., 0., 1.],
    ])
}

fn transform_point(matrix: &Matrix, point: [f32; 3]) -> [f32; 3] {
    [
        matrix[0][0] * point[0] + matrix[0][1] * point[1] + matrix[0][2] * point[2] + matrix[0][3],
        matrix[1][0] * point[0] + matrix[1][1] * point[1] + matrix[1][2] * point[2] + matrix[1][3],
        matrix[2][0] * point[0] + matrix[2][1] * point[1] + matrix[2][2] * point[2] + matrix[2][3],
    ]
}

/// Orthographic, two-sided flat shading. `yaw`/`pitch` are radians and
/// `zoom` is clamped to a usable range.
pub fn render(mesh: &Mesh, width: u32, height: u32, yaw: f32, pitch: f32, zoom: f32) -> Result<Rendered> {
    mesh.validate()?;
    ensure!(width > 0 && height > 0 && width <= MAX_RENDER_SIDE && height <= MAX_RENDER_SIDE, "invalid preview size");
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for point in &mesh.positions {
        for axis in 0..3 { low[axis] = low[axis].min(point[axis]); high[axis] = high[axis].max(point[axis]); }
    }
    let center = [(low[0] + high[0]) / 2., (low[1] + high[1]) / 2., (low[2] + high[2]) / 2.];
    let extent = (0..3).map(|axis| (high[axis] - low[axis]) / 2.).fold(0.0f32, f32::max).max(1e-6);
    let scale = (width.min(height) as f32 * 0.42 * zoom.clamp(0.2, 5.0)) / extent;
    let (sy, cy) = (pitch.sin(), pitch.cos());
    let (sa, ca) = (yaw.sin(), yaw.cos());
    let transform = |point: [f32; 3]| {
        let x = point[0] - center[0];
        let y = point[1] - center[1];
        let z = point[2] - center[2];
        let x1 = ca * x + sa * z;
        let z1 = -sa * x + ca * z;
        let y1 = cy * y - sy * z1;
        let z2 = sy * y + cy * z1;
        [width as f32 / 2. + x1 * scale, height as f32 / 2. - y1 * scale, z2]
    };
    let light = {
        let length = (0.4f32 * 0.4 + 0.6 * 0.6 + 0.7 * 0.7).sqrt();
        [0.4 / length, 0.6 / length, 0.7 / length]
    };
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let mut depth = vec![f32::NEG_INFINITY; width as usize * height as usize];
    let base = [92u8, 142, 214];
    for triangle in &mesh.triangles {
        let p = [transform(mesh.positions[triangle[0] as usize]), transform(mesh.positions[triangle[1] as usize]), transform(mesh.positions[triangle[2] as usize])];
        let normal = [
            (p[1][1] - p[0][1]) * (p[2][2] - p[0][2]) - (p[1][2] - p[0][2]) * (p[2][1] - p[0][1]),
            (p[1][2] - p[0][2]) * (p[2][0] - p[0][0]) - (p[1][0] - p[0][0]) * (p[2][2] - p[0][2]),
            (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]),
        ];
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if length < 1e-9 { continue; }
        let shade = (0.28 + 0.72 * ((normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2]) / length).abs()).clamp(0.0, 1.0);
        let color = [ (base[0] as f32 * shade) as u8, (base[1] as f32 * shade) as u8, (base[2] as f32 * shade) as u8 ];
        let min_x = p.iter().map(|v| v[0]).fold(f32::INFINITY, f32::min).floor().max(0.0) as i32;
        let max_x = p.iter().map(|v| v[0]).fold(f32::NEG_INFINITY, f32::max).ceil().min(width as f32 - 1.0) as i32;
        let min_y = p.iter().map(|v| v[1]).fold(f32::INFINITY, f32::min).floor().max(0.0) as i32;
        let max_y = p.iter().map(|v| v[1]).fold(f32::NEG_INFINITY, f32::max).ceil().min(height as f32 - 1.0) as i32;
        let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
        if area.abs() < 1e-9 { continue; }
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((p[1][0] - px) * (p[2][1] - py) - (p[1][1] - py) * (p[2][0] - px)) / area;
                let w1 = ((p[2][0] - px) * (p[0][1] - py) - (p[2][1] - py) * (p[0][0] - px)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 { continue; }
                let z = w0 * p[0][2] + w1 * p[1][2] + w2 * p[2][2];
                let index = y as usize * width as usize + x as usize;
                if z <= depth[index] { continue; }
                depth[index] = z;
                pixels[index * 4..index * 4 + 4].copy_from_slice(&[color[0], color[1], color[2], 255]);
            }
        }
    }
    Ok(Rendered { width, height, rgba: pixels })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUBE: &str = "\
v -1 -1 -1
v 1 -1 -1
v 1 1 -1
v -1 1 -1
v -1 -1 1
v 1 -1 1
v 1 1 1
v -1 1 1
f 1 2 3 4
f 5 8 7 6
f 1 5 6 2
f 2 6 7 3
f 3 7 8 4
f 5 1 4 8
";

    fn loaded(text: &str) -> Mesh { let mesh = parse_obj(text).unwrap(); mesh.validate().unwrap(); mesh }

    fn opaque(rendered: &Rendered) -> usize { rendered.rgba.chunks(4).filter(|pixel| pixel[3] > 0).count() }

    #[test]
    fn object_faces_are_triangulated_and_negative_indices_resolve() {
        let mesh = loaded(CUBE);
        assert_eq!(mesh.positions.len(), 8);
        assert_eq!(mesh.triangles.len(), 12);
        let mesh = loaded("v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf -4 -3 -2 -1\n");
        assert_eq!(mesh.triangles, vec![[0, 1, 2], [0, 2, 3]]);
        assert!(parse_obj("v 0 0 0\nf 1 2 3\n").is_err());
        assert!(parse_obj("v nan 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n").unwrap().validate().is_err());
    }

    #[test]
    fn stl_binary_and_ascii_are_read() {
        let mut binary = vec![0u8; 80];
        binary.extend_from_slice(&1u32.to_le_bytes());
        for value in [0.0f32, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] { binary.extend_from_slice(&value.to_le_bytes()); }
        binary.extend_from_slice(&0u16.to_le_bytes());
        let mesh = parse_stl(&binary).unwrap();
        assert_eq!((mesh.positions.len(), mesh.triangles.len()), (3, 1));
        let ascii = "solid s\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid\n";
        assert_eq!(parse_stl(ascii.as_bytes()).unwrap().triangles.len(), 1);
    }

    fn triangle_gltf(uri: Option<&str>) -> (Vec<u8>, Vec<u8>) {
        let mut binary = vec![];
        for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] { binary.extend_from_slice(&value.to_le_bytes()); }
        for index in [0u16, 1, 2] { binary.extend_from_slice(&index.to_le_bytes()); }
        let uri = uri.map(|prefix| format!(r#","uri":"{prefix}{}""#, base64::engine::general_purpose::STANDARD.encode(&binary))).unwrap_or_default();
        let json = format!(r#"{{"asset":{{"version":"2.0"}},"buffers":[{{"byteLength":{}{uri}}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":6}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}]}}"#, binary.len());
        (json.into_bytes(), binary)
    }

    #[test]
    fn gltf_embedded_and_glb_buffers_are_read() {
        let (json, _binary) = triangle_gltf(Some("data:application/octet-stream;base64,"));
        let mesh = parse_gltf(&json, None, None).unwrap();
        assert_eq!(mesh.triangles.len(), 1);

        let (json, binary) = triangle_gltf(None);
        let mut glb = b"glTF".to_vec();
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&((12 + 8 + json.len() + 8 + binary.len()) as u32).to_le_bytes());
        glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        glb.extend_from_slice(&json);
        glb.extend_from_slice(&(binary.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004E4942u32.to_le_bytes());
        glb.extend_from_slice(&binary);
        let path = std::env::temp_dir().join("markdownx-model.glb");
        assert_eq!(parse_glb(&glb, &path).unwrap().triangles.len(), 1);
        assert!(parse_glb(b"bad", &path).is_err());
    }

    #[test]
    fn external_buffers_cannot_escape_their_folder() {
        let root = std::env::temp_dir().join(format!("markdownx-model-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let json = br#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":1,"uri":"../escape.bin"}],"bufferViews":[],"accessors":[],"meshes":[]}"#;
        assert!(parse_gltf(json, Some(&root.join("model.gltf")), None).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn renders_opaque_pixels_and_responds_to_rotation_and_zoom() {
        let mesh = loaded(CUBE);
        let front = render(&mesh, 128, 128, 0.0, 0.0, 1.0).unwrap();
        assert_eq!(front.rgba.len(), 128 * 128 * 4);
        assert!(opaque(&front) > 1000);
        let turned = render(&mesh, 128, 128, 0.7, 0.4, 1.0).unwrap();
        assert_ne!(front.rgba, turned.rgba);
        let smaller = render(&mesh, 128, 128, 0.0, 0.0, 0.5).unwrap();
        assert!(opaque(&smaller) < opaque(&front));
        assert!(render(&mesh, 0, 128, 0.0, 0.0, 1.0).is_err());
        assert!(render(&Mesh::default(), 128, 128, 0.0, 0.0, 1.0).is_err());
    }

    #[test]
    fn dispatches_by_suffix_and_reports_unsupported_formats() {
        let root = std::env::temp_dir().join(format!("markdownx-model-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let obj = root.join("cube.obj");
        std::fs::write(&obj, CUBE).unwrap();
        assert_eq!(load(&obj).unwrap().triangles.len(), 12);
        let bad = root.join("cube.fbx");
        std::fs::write(&bad, b"x").unwrap();
        assert!(load(&bad).is_err());
        assert!(load(&root.join("missing.obj")).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
