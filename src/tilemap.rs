use std::{collections::BTreeMap, f32::consts::PI};

use bevy::{gltf::Gltf, prelude::*};

use crate::geopos::GeoPos;

#[derive(Component, Default)]
pub struct TileMap {
    /// All currently loaded tiles.
    tiles: BTreeMap<u32, BTreeMap<u32, Entity>>,
    /// The tile currently being loaded.
    loading: Option<(UVec2, Handle<Gltf>)>,
    /// Dummy square to show while a scene is loading
    dummy: Handle<Mesh>,
}

pub const TILE_ZOOM: u8 = 15;

#[derive(Component)]
pub struct Tile;

impl TileMap {
    pub fn load_next(
        &mut self,
        tilemap_id: Entity,
        commands: &mut Commands,
        server: &AssetServer,
        origin: TileCoord,
        radius: Vec2,
    ) {
        let radius = radius.abs().ceil().copysign(radius).as_ivec2();
        let origin = origin.0.floor().as_uvec2();
        self.tiles.retain(|&x, line| {
            line.retain(|&y, tile| {
                let offset = IVec2::new(x as i32, y as i32) - origin.as_ivec2();
                let oob = offset.length_squared() > radius.length_squared();
                if oob {
                    if let Some(entity) = commands.get_entity(*tile) {
                        debug!("despawn: {}/{}",x,y);
                        entity.despawn_recursive();
                    }
                }
                !oob
            });
            !line.is_empty()
        });
        let mut best_score = f32::INFINITY;
        let mut best_pos = None;
        for x_i in -radius.x..=radius.x {
            for y_i in -radius.y..=radius.y {
                let offset = IVec2::new(x_i, y_i);
                if offset.length_squared() > radius.length_squared() {
                    continue;
                }

                let pos = (origin.as_ivec2() + offset).as_uvec2();
                let score = self.get_view_tile_score(pos, offset);
                if score < best_score {
                    best_pos = Some(pos);
                    best_score = score;
                }
            }
        }
        if let Some(best_pos) = best_pos {
            self.load(tilemap_id, commands, server, best_pos);
        }
    }

    /// Takes an offset to the player position and returns a score for how important
    /// to load it is. Lower values are better.
    // FIXME(#18): use a smarter algorithm
    pub fn get_view_tile_score(&self, pos: UVec2, offset: IVec2) -> f32 {
        if let Some(line) = self.tiles.get(&pos.x) {
            if line.get(&pos.y).is_some() {
                return f32::INFINITY;
            }
        }
        offset.as_vec2().length_squared()
    }

    /// Queue a tile coordinate for loading. This will load tiles
    /// in sequence to reduce lag (which would happen if we loaded lots
    /// of tiles at the same time).
    /// Silently does nothing if the tile was already loaded or is in the process of loading.
    /// Silently does nothing if another tile is already being loaded.
    pub fn load(
        &mut self,
        tilemap_id: Entity,
        commands: &mut Commands,
        server: &AssetServer,
        pos: UVec2,
    ) {
        if self.loading.is_some() {
            return;
        }
        // https://gltiles.osm2world.org/glb/lod1/15/17388/11332.glb#Scene0"
        let name: String = format!("tile://{}_{}.glb", pos.x, pos.y);
        // Start loading next tile
        self.loading = Some((pos, server.load(name))); // "models/17430_11371.glb#Scene0"
                                                       // Insert dummy tile while loading.
        self.tiles
            .entry(pos.x)
            .or_default()
            .entry(pos.y)
            .or_insert_with(|| {
                let transform = Self::test_transform(pos);

                let id = commands
                    .spawn(PbrBundle {
                        mesh: self.dummy.clone(),
                        transform,
                        ..default()
                    })
                    .id();
                commands.entity(tilemap_id).add_child(id);
                id
            });
    }

    pub fn update(
        mut commands: Commands,
        server: Res<AssetServer>,
        scenes: ResMut<Assets<Gltf>>,
        mut tilemap: Query<(Entity, &mut Self)>,
    ) {
        for (id, mut tilemap) in &mut tilemap {
            // check if the currently loading tile is done
            if let Some((pos, scene)) = tilemap.loading.take() {
                use bevy::asset::LoadState::*;
                match server.get_load_state(&scene).unwrap() {
                    NotLoaded | Loading => {
                        tilemap.loading = Some((pos, scene));
                    }
                    Loaded => {
                        // FIXME: implement caching of downloaded assets by implementing something like
                        // https://github.com/bevyengine/bevy/blob/main/examples/asset/processing/asset_processing.rs

                        // Done, remove dummy tile and insert the real one
                        let Some(entity) = tilemap.tiles.entry(pos.x).or_default().get_mut(&pos.y)
                        else {
                            continue;
                        };

                        let transform = Self::test_transform(pos);
                        let scene = scenes.get(scene).unwrap().scenes[0].clone();
                        let tile = commands
                            .spawn((
                                SceneBundle {
                                    scene, // "models/17430_11371.glb#Scene0"
                                    transform,
                                    ..default()
                                },
                                Tile,
                            ))
                            .id();
                        commands.entity(id).add_child(tile);
                        let dummy = std::mem::replace(entity, tile);
                        if let Some(mut entity) = commands.get_entity(dummy) {
                            entity.despawn();
                        }
                    }
                    Failed => {
                        error!("failed to load tile {pos} from network");
                    }
                }
            }
        }
    }

    pub fn new(meshes: &mut Assets<Mesh>) -> Self {
        // FIXME: compute dummy tile size on the fly
        let half = 814.5 / 2.0;
        Self {
            dummy: meshes.add(
                shape::Box {
                    min_x: -half,
                    max_x: half,
                    min_y: 0.0,
                    max_y: 1.0,
                    min_z: -half,
                    max_z: half,
                }
                .into(),
            ),
            ..default()
        }
    }

    fn test_transform(pos: UVec2) -> Transform {
        let coord = TileCoord(pos.as_vec2());
        let pos = coord.to_geo_pos(TILE_ZOOM).to_cartesian();
        let next = TileCoord(Vec2 {
            x: coord.0.x,
            y: coord.0.y - 1.0,
        })
        .to_geo_pos(TILE_ZOOM)
        .to_cartesian();
        Transform::from_translation(pos).looking_to(next - pos, pos.normalize())
    }
}

/// A coordinate in the OWM tile coordinate system. Allows for positions within a tile. ???
#[derive(Debug, Copy, Clone)]
pub struct TileCoord(pub Vec2);

impl TileCoord {
    pub fn to_geo_pos(self, zoom: u8) -> GeoPos {
        let pow_zoom = 2_u32.pow(zoom.into()) as f32;

        let lon = self.0.x / pow_zoom * 360.0 - 180.0;
        let lat_rad = (PI * (1. - 2. * self.0.y / pow_zoom)).sinh().atan();
        let lat = lat_rad * 180.0 / PI;
        GeoPos { lat, lon }
    }
}
