//! Game data read from the local install: files, atlases and images.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use wowprogramming_casc::Storage;
use wowprogramming_formats::{blp, db2};

pub use blp::Image;

/// FileDataIDs of the atlas tables. Their paths are not hashed in the root.
const UI_TEXTURE_ATLAS: u32 = 897470;
const UI_TEXTURE_ATLAS_MEMBER: u32 = 897532;
/// FileDataID of GlobalStrings.db2: BaseTag, TagText, Flags.
const GLOBAL_STRINGS: u32 = 1394440;
/// GlobalStrings flag for strings used in the game (not only the login screen).
const GLOBAL_STRINGS_GAME: u32 = 0x1;

#[derive(Clone, Debug, PartialEq)]
pub struct AtlasInfo {
    pub name: String,
    pub file: u32,
    /// Size of the atlas member in UI units.
    pub width: f64,
    pub height: f64,
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub tiles_horizontally: bool,
    pub tiles_vertically: bool,
}

pub struct GameData {
    storage: Storage,
    atlases: HashMap<String, AtlasInfo>,
    images: RefCell<HashMap<ImageKey, Option<Rc<Image>>>>,
}

/// Cache key for a decoded image.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum ImageKey {
    Id(u32),
    Path(String),
}

impl GameData {
    /// Open the storage of the Retail client in `install_dir` (the folder
    /// with `.build.info`) and read the atlas tables.
    /// `install_dir` may also be the flavor folder (such as `_retail_`)
    /// inside the install.
    pub fn open(install_dir: &Path) -> Result<GameData, String> {
        let root = if install_dir.join(".build.info").exists() {
            install_dir
        } else {
            install_dir.parent().unwrap_or(install_dir)
        };
        let storage = Storage::open(root, "wow").map_err(|e| e.to_string())?;
        let atlases = read_atlases(&storage).unwrap_or_else(|e| {
            eprintln!("warning: cannot read atlas tables: {e}");
            HashMap::new()
        });
        Ok(GameData {
            storage,
            atlases,
            images: RefCell::new(HashMap::new()),
        })
    }

    pub fn build_name(&self) -> &str {
        &self.storage.build_name
    }

    pub fn atlas(&self, name: &str) -> Option<&AtlasInfo> {
        self.atlases.get(&name.to_ascii_lowercase())
    }

    pub fn atlas_count(&self) -> usize {
        self.atlases.len()
    }

    /// FileDataID for a texture path. Paths without an extension get `.blp`.
    pub fn file_id(&self, path: &str) -> Option<u32> {
        let path = path.replace('/', "\\");
        self.storage.file_id(&path).or_else(|| {
            let has_ext = path.rsplit('\\').next().is_some_and(|f| f.contains('.'));
            if has_ext {
                None
            } else {
                self.storage.file_id(&format!("{path}.blp"))
            }
        })
    }

    pub fn has_file(&self, id: u32) -> bool {
        self.storage.has_file(id)
    }

    pub fn read_path(&self, path: &str) -> Option<Vec<u8>> {
        let id = self.file_id(path)?;
        self.storage.read_file(id).ok()
    }

    pub fn read_file(&self, id: u32) -> Option<Vec<u8>> {
        self.storage.read_file(id).ok()
    }

    /// The localized global strings of the game UI, such as `SPELLBOOK`.
    pub fn global_strings(&self) -> Vec<(String, String)> {
        let Ok(data) = self.storage.read_file(GLOBAL_STRINGS) else {
            return Vec::new();
        };
        let Ok(table) = db2::Table::parse(&data) else {
            return Vec::new();
        };
        table
            .records
            .iter()
            .filter(|r| {
                let flags = r
                    .values
                    .get(2)
                    .and_then(|v| v.first())
                    .copied()
                    .unwrap_or(0);
                flags & GLOBAL_STRINGS_GAME != 0 || flags == 0
            })
            .map(|r| (table.string(r, 0), table.string(r, 1)))
            .filter(|(k, _)| !k.is_empty())
            .collect()
    }

    /// Decoded image, cached. `None` if the file is missing or not a BLP.
    pub fn image(&self, key: &ImageKey) -> Option<Rc<Image>> {
        if let Some(img) = self.images.borrow().get(key) {
            return img.clone();
        }
        let id = match key {
            ImageKey::Id(id) => Some(*id),
            ImageKey::Path(p) => self.file_id(p),
        };
        let img = id
            .and_then(|id| self.storage.read_file(id).ok())
            .and_then(|data| blp::decode(&data).ok())
            .map(Rc::new);
        self.images.borrow_mut().insert(key.clone(), img.clone());
        img
    }
}

fn read_atlases(storage: &Storage) -> Result<HashMap<String, AtlasInfo>, String> {
    let atlas_data = storage
        .read_file(UI_TEXTURE_ATLAS)
        .map_err(|e| e.to_string())?;
    let atlas_table = db2::Table::parse(&atlas_data)?;
    // UiTextureAtlas: ID, FileDataID, AtlasWidth, AtlasHeight, UiCanvasID.
    let atlases: HashMap<u32, (u32, f32, f32)> = atlas_table
        .records
        .iter()
        .map(|r| {
            let v = |i: usize| {
                r.values
                    .get(i)
                    .and_then(|v| v.first())
                    .copied()
                    .unwrap_or(0)
            };
            (r.id, (v(1), v(2) as f32, v(3) as f32))
        })
        .collect();

    let member_data = storage
        .read_file(UI_TEXTURE_ATLAS_MEMBER)
        .map_err(|e| e.to_string())?;
    let members = db2::Table::parse(&member_data)?;
    // UiTextureAtlasMember: Name, ID, UiTextureAtlasID, Width, Height, Left,
    // Right, Top, Bottom, UiTextureAtlasElementID, OverrideWidth,
    // OverrideHeight, Flags, UiCanvasID.
    let mut out = HashMap::new();
    for r in &members.records {
        let v = |i: usize| {
            r.values
                .get(i)
                .and_then(|v| v.first())
                .copied()
                .unwrap_or(0)
        };
        let name = members.string(r, 0);
        if name.is_empty() {
            continue;
        }
        let Some(&(file, aw, ah)) = atlases.get(&v(2)) else {
            continue;
        };
        if aw <= 0.0 || ah <= 0.0 {
            continue;
        }
        let (w, h) = (v(3) as u16 as f64, v(4) as u16 as f64);
        let (ow, oh) = (v(10) as u16 as f64, v(11) as u16 as f64);
        let flags = v(12);
        out.insert(
            name.to_ascii_lowercase(),
            AtlasInfo {
                width: if ow > 0.0 { ow } else { w },
                height: if oh > 0.0 { oh } else { h },
                left: v(5) as f32 / aw,
                right: v(6) as f32 / aw,
                top: v(7) as f32 / ah,
                bottom: v(8) as f32 / ah,
                tiles_horizontally: flags & 0x4 != 0,
                tiles_vertically: flags & 0x2 != 0,
                file,
                name,
            },
        );
    }
    Ok(out)
}
