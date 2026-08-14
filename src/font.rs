use std::env;
use std::path::{Path, PathBuf};

pub fn font_search_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    v.push(PathBuf::from("assets/fonts/VT323-Regular.ttf"));
    v.push(PathBuf::from("./assets/fonts/VT323-Regular.ttf"));
    if let Ok(exe) = env::current_exe() {
        if let Some(p) = exe.parent() {
            v.push(p.join("assets/fonts/VT323-Regular.ttf"));
            v.push(p.join("../assets/fonts/VT323-Regular.ttf"));
            v.push(p.join("../../assets/fonts/VT323-Regular.ttf"));
        }
    }
    if let Ok(cwd) = env::current_dir() {
        for anc in cwd.ancestors() {
            v.push(anc.join("assets/fonts/VT323-Regular.ttf"));
        }
    }
    v.push(PathBuf::from("/System/Library/Fonts/Monaco.ttf"));
    v.push(PathBuf::from("/System/Library/Fonts/Courier.dfont"));
    v.push(PathBuf::from("/Library/Fonts/Courier New.ttf"));
    v
}

pub fn load_best_font(
    ttf: &sdl2::ttf::Sdl2TtfContext,
    target_pt: u16,
) -> Result<(sdl2::ttf::Font<'_, 'static>, PathBuf), String> {
    let mut last_err = String::new();
    for p in font_search_paths() {
        if Path::new(&p).exists() {
            match ttf.load_font(&p, target_pt) {
                Ok(f) => return Ok((f, p)),
                Err(e) => last_err = e.to_string(),
            }
        }
    }
    Err(format!(
        "no VT323 font found; tried {:?}; last_err={}; will try system mono fallback",
        font_search_paths(),
        last_err
    ))
}

/// Find the largest point size that fits the grid area.
/// Returns the font, path and chosen point size.
pub fn choose_font(
    ttf: &sdl2::ttf::Sdl2TtfContext,
    grid_w: u32,
    grid_h: u32,
) -> Result<(sdl2::ttf::Font<'_, 'static>, PathBuf, u16), String> {
    use crate::constants::usize_to_u32;
    use crate::constants::COLS;
    use crate::constants::ROWS;

    let rows_u32 = usize_to_u32(ROWS);

    let mut chosen_pt: u16 = 22;
    let mut font_path_used = PathBuf::from("<fallback>");
    let mut font_opt: Option<sdl2::ttf::Font<'_, 'static>> = None;

    for pt in (14..=28).rev() {
        let pt_u16 = u16::try_from(pt).expect("pt fits in u16");
        match load_best_font(ttf, pt_u16) {
            Ok((f, path)) => {
                let sample = "M".repeat(COLS);
                if let Ok((w, _h)) = f.size_of(&sample) {
                    let line_skip_i32 = f.recommended_line_spacing();
                    let line_skip = u32::try_from(line_skip_i32).unwrap_or(0);
                    let needed_h2 = line_skip.saturating_mul(rows_u32);
                    let needed_w = w;
                    if needed_w <= grid_w && needed_h2 <= grid_h {
                        chosen_pt = pt_u16;
                        font_path_used = path;
                        font_opt = Some(f);
                        break;
                    }
                    if pt == 14 {
                        chosen_pt = pt_u16;
                        font_path_used = path;
                        font_opt = Some(f);
                    }
                } else {
                    chosen_pt = pt_u16;
                    font_path_used = path;
                    font_opt = Some(f);
                    break;
                }
            }
            Err(e) => {
                if pt == 14 && std::env::var("DEBUG").is_ok() {
                    eprintln!("font load warning: {e}");
                }
            }
        }
    }

    let font = if let Some(f) = font_opt {
        f
    } else {
        if std::env::var("DEBUG").is_ok() {
            eprintln!("No VT323 found, trying system mono at {chosen_pt}pt");
        }
        let mut fallback: Option<sdl2::ttf::Font<'_, 'static>> = None;
        let mut fallback_path = font_path_used;
        for p in font_search_paths() {
            if p.exists() {
                if let Ok(f) = ttf.load_font(&p, chosen_pt) {
                    fallback = Some(f);
                    fallback_path = p;
                    break;
                }
            }
        }
        let Some(f) = fallback else {
            return Err(format!(
                "Failed to load any font. Bundle VT323 at assets/fonts/VT323-Regular.ttf (tried {:?})",
                font_search_paths()
            ));
        };
        font_path_used = fallback_path;
        f
    };
    Ok((font, font_path_used, chosen_pt))
}
