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
/// UiTextureAtlasElementSliceData: ID, UiTextureAtlasElementID, Left, Top,
/// Right, Bottom, SliceMode.
const UI_TEXTURE_ATLAS_ELEMENT_SLICE_DATA: u32 = 5319042;
/// FileDataID of GlobalColor.db2: BaseTag, Color (ARGB).
const GLOBAL_COLOR: u32 = 4328468;
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
    /// Nine-slice margins in pixels (left, top, right, bottom), if the
    /// atlas element has slice data.
    pub slice: Option<Slice>,
}

/// Nine-slice drawing of a texture: corners keep their size, edges and the
/// centre stretch (or tile).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slice {
    /// Margins in texture pixels: left, top, right, bottom.
    pub margins: [f32; 4],
    /// `Enum.UITextureSliceMode`: 0 stretched, 1 tiled.
    pub mode: u8,
}

pub struct GameData {
    storage: Storage,
    /// The client executable of the flavor, if found.
    pub client_exe: Option<std::path::PathBuf>,
    atlases: HashMap<String, AtlasInfo>,
    images: RefCell<HashMap<ImageKey, Option<Rc<Image>>>>,
    extra_atlases: RefCell<HashMap<String, AtlasInfo>>,
    colors: std::cell::OnceCell<Vec<(String, [f32; 4])>>,
    /// Folders of third-party addons: `Interface\AddOns\<name>\...`
    /// textures that are not in the game data are read from here.
    addon_dirs: RefCell<Vec<std::path::PathBuf>>,
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
        let client_exe = [
            install_dir.join("Wow.exe"),
            root.join("_retail_").join("Wow.exe"),
        ]
        .into_iter()
        .find(|p| p.exists());
        Ok(GameData {
            storage,
            client_exe,
            atlases,
            images: RefCell::new(HashMap::new()),
            extra_atlases: RefCell::new(HashMap::new()),
            colors: std::cell::OnceCell::new(),
            addon_dirs: RefCell::new(Vec::new()),
        })
    }

    pub fn build_name(&self) -> &str {
        &self.storage.build_name
    }

    /// An atlas from the game tables, or else one a scenario added.
    pub fn atlas(&self, name: &str) -> Option<AtlasInfo> {
        let key = name.to_ascii_lowercase();
        self.atlases
            .get(&key)
            .cloned()
            .or_else(|| self.extra_atlases.borrow().get(&key).cloned())
    }

    /// Add an atlas that the game tables lack, such as one recorded by a
    /// capture. Atlases in the game tables take precedence.
    pub fn add_atlas(&self, info: AtlasInfo) {
        self.extra_atlases
            .borrow_mut()
            .insert(info.name.to_ascii_lowercase(), info);
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
    /// With `glue`, also the strings of the login and character select
    /// screens (all strings: the glue screens use game strings too).
    /// Named UI colours (`C_UIColor.GetColors`): tag and RGBA.
    pub fn global_colors(&self) -> Vec<(String, [f32; 4])> {
        self.colors
            .get_or_init(|| self.read_global_colors())
            .clone()
    }

    fn read_global_colors(&self) -> Vec<(String, [f32; 4])> {
        let Ok(data) = self.storage.read_file(GLOBAL_COLOR) else {
            return Vec::new();
        };
        let Ok(t) = db2::Table::parse(&data) else {
            return Vec::new();
        };
        t.records
            .iter()
            .filter_map(|r| {
                let tag = t.string(r, 0);
                let argb = r.values.get(1).and_then(|v| v.first()).copied()?;
                let c = |shift: u32| ((argb >> shift) & 0xff) as f32 / 255.0;
                (!tag.is_empty()).then(|| (tag, [c(16), c(8), c(0), c(24)]))
            })
            .collect()
    }

    pub fn global_strings(&self, glue: bool) -> Vec<(String, String)> {
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
                glue || flags & GLOBAL_STRINGS_GAME != 0 || flags == 0
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
        let img = match (id, key) {
            (Some(id), _) => self
                .storage
                .read_file(id)
                .ok()
                .and_then(|data| blp::decode(&data).ok()),
            (None, ImageKey::Path(p)) => self.addon_image(p),
            _ => None,
        }
        .map(Rc::new);
        self.images.borrow_mut().insert(key.clone(), img.clone());
        img
    }
}

impl GameData {
    /// Let addon textures be read from these folders (the addons folder).
    pub fn set_addon_dirs(&self, dirs: Vec<std::path::PathBuf>) {
        *self.addon_dirs.borrow_mut() = dirs;
    }

    /// An addon's own texture: `Interface\AddOns\Name\Images\Dot` is
    /// `Name/Images/Dot.blp` or `.tga` in an addons folder, matched
    /// without regard to case, as the client does on Windows.
    fn addon_image(&self, path: &str) -> Option<Image> {
        let path = path.replace('/', "\\");
        let lower = path.to_ascii_lowercase();
        let rest = path
            .get(lower.find("interface\\addons\\")? + 17..)?
            .to_string();
        let parts: Vec<&str> = rest.split('\\').filter(|p| !p.is_empty()).collect();
        for dir in self.addon_dirs.borrow().iter() {
            let Some(found) = find_ignoring_case(dir, &parts) else {
                continue;
            };
            let data = std::fs::read(&found).ok()?;
            let ext = found
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase());
            return match ext.as_deref() {
                Some("tga") => wowprogramming_formats::tga::decode(&data).ok(),
                _ => blp::decode(&data).ok(),
            };
        }
        None
    }
}

/// The file `parts` below `dir`, each part matched without case; the last
/// part may leave out its `.blp` or `.tga` extension.
fn find_ignoring_case(dir: &std::path::Path, parts: &[&str]) -> Option<std::path::PathBuf> {
    let (last, folders) = parts.split_last()?;
    let mut cur = dir.to_path_buf();
    for part in folders {
        cur = child_ignoring_case(&cur, part, false)?;
    }
    child_ignoring_case(&cur, last, true)
}

fn child_ignoring_case(
    dir: &std::path::Path,
    name: &str,
    file: bool,
) -> Option<std::path::PathBuf> {
    let exact = dir.join(name);
    if (file && exact.is_file()) || (!file && exact.is_dir()) {
        return Some(exact);
    }
    let want = name.to_ascii_lowercase();
    let mut fallback = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let n = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if n == want && (!file || entry.path().is_file()) {
            return Some(entry.path());
        }
        if file && (n == format!("{want}.blp") || n == format!("{want}.tga")) {
            fallback = fallback.or(Some(entry.path()));
        }
    }
    fallback
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
    // Slice data is keyed by the atlas element. A missing table only loses
    // nine-slice drawing.
    let mut slices: HashMap<u32, Slice> = HashMap::new();
    let slice_table = storage
        .read_file(UI_TEXTURE_ATLAS_ELEMENT_SLICE_DATA)
        .ok()
        .and_then(|d| db2::Table::parse(&d).ok());
    for r in slice_table.iter().flat_map(|t| t.records.iter()) {
        let v = |i: usize| {
            r.values
                .get(i)
                .and_then(|v| v.first())
                .copied()
                .unwrap_or(0)
        };
        slices.insert(
            v(1),
            Slice {
                margins: [v(2) as f32, v(3) as f32, v(4) as f32, v(5) as f32],
                mode: v(6) as u8,
            },
        );
    }
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
                slice: slices.get(&v(9)).copied(),
                file,
                name,
            },
        );
    }
    Ok(out)
}
