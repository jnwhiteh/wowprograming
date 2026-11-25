//! Map data from the game's UiMap tables, for `C_Map` (`lua/maps.lua`).
//!
//! UiMap holds each map's name, parent, type and flags. UiMapAssignment
//! ties a part of a map (`UiMin`..`UiMax`, 0 to 1) to a box in world
//! coordinates (`Region`: min x, y, z, then max x, y, z) on a continent
//! (`MapID`). The map's horizontal axis runs along world y (west is
//! positive, so left is the larger y) and its vertical axis along world
//! x (north is positive, so top is the larger x). From the assignment that
//! covers most of a map, the sandbox extends the box to the whole map.

use mlua::{Lua, Result, Table, Value};
use wowprogramming_formats::db2;
use wowprogramming_lua::mlua;

use crate::state::state;

/// FileDataIDs from the community listfile; column layouts from
/// WoWDBDefs for build 12.1.5.70961.
const UI_MAP: u32 = 1957206;
const UI_MAP_ASSIGNMENT: u32 = 1957219;
const UI_MAP_GROUP_MEMBER: u32 = 1957204;

fn field(r: &db2::Record, i: usize, j: usize) -> u32 {
    r.values.get(i).and_then(|v| v.get(j)).copied().unwrap_or(0)
}

fn float(r: &db2::Record, i: usize, j: usize) -> f64 {
    f32::from_bits(field(r, i, j)) as f64
}

/// The world box of a whole map: continent, left, right, top, bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldRect {
    pub continent: u32,
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

/// Extend an assignment's box to the whole map (UI coordinates 0 to 1).
pub fn whole_map(
    continent: u32,
    ui_min: [f64; 2],
    ui_max: [f64; 2],
    region: [f64; 6],
) -> Option<WorldRect> {
    let du = ui_max[0] - ui_min[0];
    let dv = ui_max[1] - ui_min[1];
    if du <= 0.0 || dv <= 0.0 {
        return None;
    }
    let (min_x, min_y, max_x, max_y) = (region[0], region[1], region[3], region[4]);
    let yards_u = (max_y - min_y) / du;
    let yards_v = (max_x - min_x) / dv;
    Some(WorldRect {
        continent,
        left: max_y + ui_min[0] * yards_u,
        right: max_y - (1.0 - ui_min[0]) * yards_u,
        top: max_x + ui_min[1] * yards_v,
        bottom: max_x - (1.0 - ui_min[1]) * yards_v,
    })
}

/// Build the Lua map data: `{ maps = { [id] = {...} }, groups = { ... } }`.
/// Returns nil without game data.
pub fn data(lua: &Lua) -> Result<Option<Table>> {
    let Some(gd) = state(lua).gamedata.clone() else {
        return Ok(None);
    };
    let read = |id| gd.read_file(id).and_then(|d| db2::Table::parse(&d).ok());
    let (Some(ui_map), Some(assignments)) = (read(UI_MAP), read(UI_MAP_ASSIGNMENT)) else {
        return Ok(None);
    };
    // The assignment that covers most of each map gives its world box.
    let mut best: std::collections::HashMap<u32, (f64, WorldRect)> = Default::default();
    for r in &assignments.records {
        let ui_min = [float(r, 0, 0), float(r, 0, 1)];
        let ui_max = [float(r, 1, 0), float(r, 1, 1)];
        let region = [
            float(r, 2, 0),
            float(r, 2, 1),
            float(r, 2, 2),
            float(r, 2, 3),
            float(r, 2, 4),
            float(r, 2, 5),
        ];
        let map = field(r, 4, 0);
        let continent = field(r, 6, 0);
        let cover = (ui_max[0] - ui_min[0]) * (ui_max[1] - ui_min[1]);
        let Some(rect) = whole_map(continent, ui_min, ui_max, region) else {
            continue;
        };
        if best.get(&map).is_none_or(|(c, _)| cover > *c) {
            best.insert(map, (cover, rect));
        }
    }
    let maps = lua.create_table()?;
    for r in &ui_map.records {
        let t = lua.create_table()?;
        t.set("name", ui_map.string(r, 0))?;
        t.set("parent", field(r, 2, 0))?;
        t.set("flags", field(r, 3, 0))?;
        t.set("system", field(r, 4, 0))?;
        t.set("type", field(r, 5, 0))?;
        if let Some((_, rect)) = best.get(&r.id) {
            t.set("continent", rect.continent)?;
            t.set("left", rect.left)?;
            t.set("right", rect.right)?;
            t.set("top", rect.top)?;
            t.set("bottom", rect.bottom)?;
        }
        maps.set(r.id, t)?;
    }
    let groups = lua.create_table()?;
    if let Some(members) = read(UI_MAP_GROUP_MEMBER) {
        for (i, r) in members.records.iter().enumerate() {
            let t = lua.create_table()?;
            t.set("name", members.string(r, 0))?;
            t.set("group", field(r, 1, 0))?;
            t.set("map", field(r, 2, 0))?;
            t.set("floor", field(r, 3, 0))?;
            t.set("relativeHeightIndex", field(r, 4, 0))?;
            groups.set(i + 1, t)?;
        }
    }
    let out = lua.create_table()?;
    out.set("maps", maps)?;
    out.set("groups", groups)?;
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extends_a_partial_box_to_the_whole_map() {
        // The box covers the right half of the map: world y 0..100 is
        // UI x 0.5..1, world x 0..200 is UI y 0..1.
        let r = whole_map(
            1,
            [0.5, 0.0],
            [1.0, 1.0],
            [0.0, 0.0, 0.0, 200.0, 100.0, 0.0],
        )
        .unwrap();
        assert_eq!((r.left, r.right, r.top, r.bottom), (200.0, 0.0, 200.0, 0.0));
    }
}

const UI_MAP_X_MAP_ART: u32 = 1957217;
const UI_MAP_ART: u32 = 1957202;
const UI_MAP_ART_STYLE_LAYER: u32 = 1957208;
const UI_MAP_ART_TILE: u32 = 1957210;
const WORLD_MAP_OVERLAY: u32 = 1134579;
const WORLD_MAP_OVERLAY_TILE: u32 = 1957212;

/// An explored area's overlay on a map's art (WorldMapOverlay).
#[derive(Clone, Debug, Default)]
pub struct Overlay {
    pub width: u32,
    pub height: u32,
    pub offset_x: u32,
    pub offset_y: u32,
    /// Top, bottom, left, right.
    pub hit_rect: [u32; 4],
    /// Tile textures row by row.
    pub tiles: Vec<u32>,
}

/// One layer of a map art style (UiMapArtStyleLayer).
#[derive(Clone, Debug)]
pub struct ArtLayer {
    pub index: u32,
    pub width: u32,
    pub height: u32,
    pub tile_width: u32,
    pub tile_height: u32,
    pub min_scale: f64,
    pub max_scale: f64,
    pub additional_zoom_steps: u32,
}

/// A tile of map art: row, column, FileDataID.
type Tile = (u32, u32, u32);
/// Tiles by (art ID, layer index).
type TileMap = std::collections::HashMap<(u32, u32), Vec<Tile>>;

/// Map art: which art a map shows, its layers and its tiles.
#[derive(Default)]
pub struct MapArt {
    /// UiMap ID -> UiMapArt ID (phase 0).
    art_of_map: std::collections::HashMap<u32, u32>,
    /// UiMapArt ID -> style ID.
    style_of_art: std::collections::HashMap<u32, u32>,
    /// Style ID -> layers, by layer index.
    layers: std::collections::HashMap<u32, Vec<ArtLayer>>,
    /// (art ID, layer) -> tiles as (row, column, FileDataID).
    tiles: TileMap,
    /// UiMapArt ID -> overlays of explored areas.
    overlays: std::collections::HashMap<u32, Vec<Overlay>>,
}

impl MapArt {
    fn load(gd: &crate::gamedata::GameData) -> MapArt {
        let read = |id| gd.read_file(id).and_then(|d| db2::Table::parse(&d).ok());
        let mut art = MapArt::default();
        if let Some(t) = read(UI_MAP_X_MAP_ART) {
            for r in &t.records {
                // Phased art (PhaseID != 0) is skipped: the sandbox has no phases.
                if field(r, 0, 0) == 0 {
                    art.art_of_map.insert(field(r, 2, 0), field(r, 1, 0));
                }
            }
        }
        if let Some(t) = read(UI_MAP_ART) {
            for r in &t.records {
                art.style_of_art.insert(r.id, field(r, 2, 0));
            }
        }
        if let Some(t) = read(UI_MAP_ART_STYLE_LAYER) {
            for r in &t.records {
                art.layers
                    .entry(field(r, 8, 0))
                    .or_default()
                    .push(ArtLayer {
                        index: field(r, 0, 0),
                        width: field(r, 1, 0),
                        height: field(r, 2, 0),
                        tile_width: field(r, 3, 0),
                        tile_height: field(r, 4, 0),
                        min_scale: float(r, 5, 0),
                        max_scale: float(r, 6, 0),
                        additional_zoom_steps: field(r, 7, 0),
                    });
            }
            for layers in art.layers.values_mut() {
                layers.sort_by_key(|l| l.index);
            }
        }
        if let Some(t) = read(UI_MAP_ART_TILE) {
            for r in &t.records {
                let key = (field(r, 4, 0), field(r, 2, 0));
                art.tiles.entry(key).or_default().push((
                    field(r, 0, 0),
                    field(r, 1, 0),
                    field(r, 3, 0),
                ));
            }
            for tiles in art.tiles.values_mut() {
                tiles.sort();
            }
        }
        if let (Some(t), Some(tiles)) = (read(WORLD_MAP_OVERLAY), read(WORLD_MAP_OVERLAY_TILE)) {
            let mut by_overlay: std::collections::HashMap<u32, Vec<(u32, u32, u32)>> =
                Default::default();
            for r in &tiles.records {
                // Layer 0 only: the overlay art of the base layer.
                if field(r, 2, 0) == 0 {
                    by_overlay.entry(field(r, 4, 0)).or_default().push((
                        field(r, 0, 0),
                        field(r, 1, 0),
                        field(r, 3, 0),
                    ));
                }
            }
            for r in &t.records {
                let mut list = by_overlay.remove(&r.id).unwrap_or_default();
                list.sort();
                art.overlays
                    .entry(field(r, 1, 0))
                    .or_default()
                    .push(Overlay {
                        width: field(r, 2, 0),
                        height: field(r, 3, 0),
                        offset_x: field(r, 4, 0),
                        offset_y: field(r, 5, 0),
                        hit_rect: [
                            field(r, 6, 0),
                            field(r, 7, 0),
                            field(r, 8, 0),
                            field(r, 9, 0),
                        ],
                        tiles: list.into_iter().map(|(_, _, f)| f).collect(),
                    });
            }
        }
        art
    }

    pub fn overlays(&self, map: u32) -> &[Overlay] {
        self.art_id(map)
            .and_then(|a| self.overlays.get(&a))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn art_id(&self, map: u32) -> Option<u32> {
        self.art_of_map.get(&map).copied()
    }

    pub fn layers(&self, map: u32) -> &[ArtLayer] {
        self.art_id(map)
            .and_then(|a| self.style_of_art.get(&a))
            .and_then(|s| self.layers.get(s))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Tile textures of a layer, row by row (as Blizzard's tiled map art
    /// reads them: index (row - 1) * columns + column).
    pub fn tiles(&self, map: u32, layer: u32) -> Vec<u32> {
        let Some(art) = self.art_id(map) else {
            return Vec::new();
        };
        self.tiles
            .get(&(art, layer))
            .map(|t| t.iter().map(|&(_, _, f)| f).collect())
            .unwrap_or_default()
    }
}

thread_local! {
    static ART: std::cell::RefCell<Option<std::rc::Rc<MapArt>>> = const { std::cell::RefCell::new(None) };
}

/// The map art tables, read on first use.
pub fn art(lua: &Lua) -> Option<std::rc::Rc<MapArt>> {
    if let Some(a) = ART.with(|a| a.borrow().clone()) {
        return Some(a);
    }
    let gd = state(lua).gamedata.clone()?;
    let a = std::rc::Rc::new(MapArt::load(&gd));
    ART.with(|c| *c.borrow_mut() = Some(a.clone()));
    Some(a)
}

/// Native `C_Map` functions for map art.
pub fn install_art(lua: &Lua, c_map: &Table) -> Result<()> {
    c_map.set(
        "GetMapArtID",
        lua.create_function(|lua, map: u32| Ok(art(lua).and_then(|a| a.art_id(map))))?,
    )?;
    c_map.set(
        "GetMapArtLayers",
        lua.create_function(|lua, map: u32| {
            let out = lua.create_table()?;
            if let Some(a) = art(lua) {
                for (i, l) in a.layers(map).iter().enumerate() {
                    let t = lua.create_table()?;
                    t.set("layerWidth", l.width)?;
                    t.set("layerHeight", l.height)?;
                    t.set("tileWidth", l.tile_width)?;
                    t.set("tileHeight", l.tile_height)?;
                    t.set("minScale", l.min_scale)?;
                    t.set("maxScale", l.max_scale)?;
                    t.set("additionalZoomSteps", l.additional_zoom_steps)?;
                    out.set(i + 1, t)?;
                }
            }
            Ok(out)
        })?,
    )?;
    // Every area counts as explored (inferred: the sandbox has no
    // exploration state). The meaning of WorldMapOverlay.Flags is not
    // known, so no overlay is "shown by mouse over" or "on top".
    let exploration = match lua.globals().get::<Value>("C_MapExplorationInfo")? {
        Value::Table(t) => t,
        _ => {
            let t = lua.create_table()?;
            lua.globals().set("C_MapExplorationInfo", t.clone())?;
            t
        }
    };
    exploration.set(
        "GetExploredMapTextures",
        lua.create_function(|lua, map: u32| {
            let out = lua.create_table()?;
            if let Some(a) = art(lua) {
                for (i, o) in a.overlays(map).iter().enumerate() {
                    let t = lua.create_table()?;
                    t.set("textureWidth", o.width)?;
                    t.set("textureHeight", o.height)?;
                    t.set("offsetX", o.offset_x)?;
                    t.set("offsetY", o.offset_y)?;
                    t.set("isShownByMouseOver", false)?;
                    t.set("isDrawOnTopLayer", false)?;
                    t.set("fileDataIDs", lua.create_sequence_from(o.tiles.clone())?)?;
                    let hit = lua.create_table()?;
                    hit.set("top", o.hit_rect[0])?;
                    hit.set("bottom", o.hit_rect[1])?;
                    hit.set("left", o.hit_rect[2])?;
                    hit.set("right", o.hit_rect[3])?;
                    t.set("hitRect", hit)?;
                    out.set(i + 1, t)?;
                }
            }
            Ok(out)
        })?,
    )?;
    c_map.set(
        "GetMapArtLayerTextures",
        lua.create_function(|lua, (map, layer): (u32, u32)| {
            // The layer argument counts from 1; tiles store it from 0.
            let tiles = art(lua)
                .map(|a| a.tiles(map, layer.saturating_sub(1)))
                .unwrap_or_default();
            lua.create_sequence_from(tiles)
        })?,
    )?;
    Ok(())
}
