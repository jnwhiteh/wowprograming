//! Map data from the game's UiMap tables, for `C_Map` (`lua/maps.lua`).
//!
//! UiMap holds each map's name, parent, type and flags. UiMapAssignment
//! ties a part of a map (`UiMin`..`UiMax`, 0 to 1) to a box in world
//! coordinates (`Region`: min x, y, z, then max x, y, z) on a continent
//! (`MapID`). The map's horizontal axis runs along world y (west is
//! positive, so left is the larger y) and its vertical axis along world
//! x (north is positive, so top is the larger x). From the assignment that
//! covers most of a map, the sandbox extends the box to the whole map.

use mlua::{Lua, Result, Table};
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
