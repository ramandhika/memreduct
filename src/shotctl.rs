//! `--screenshot <file.png>` mode: paint a number of frames, save the window
//! picture, then exit. Used by automated visual checks.

pub const FRAMES: u32 = 120;

pub struct ShotCtl {
    pub path: String,
    pub frames: u32,
    pub saved: bool,
    /// Size of the last frame that looked painted; two identical painted
    /// frames in a row mean the window has settled.
    pub last_dims: Option<(u32, u32)>,
    pub ok_streak: u32,
}

impl ShotCtl {
    pub fn new(path: String) -> ShotCtl {
        ShotCtl {
            path,
            frames: 0,
            saved: false,
            last_dims: None,
            ok_streak: 0,
        }
    }
}
