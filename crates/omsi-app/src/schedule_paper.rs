//! The paper timetable in the driver's cab.
//!
//! Stock vehicle models show this through `[matl_freetex] file_schedule`. OMSI supplies the
//! bitmap named by that string; openOMSI makes it from the player's current duty and keeps
//! it in its own cache so the original installation remains read-only.

use crate::schedule::PlayerDuty;
use ab_glyph::{Font as _, FontVec, PxScale, ScaleFont};
use anyhow::{anyhow, Context, Result};
use omsi_content::font::{Font, FontAtlas, FontChar, TextAlign};
use omsi_sim::VehicleInstance;
use omsi_texture::Image;
use std::borrow::Cow;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const PAPER_X: u32 = 100;
const COLUMN_GAP: u32 = 32;
const PAPER_TOP: u32 = 88;
const ROWS_TOP_GAP: u32 = 32;
// Schedule.bmp's lower 290 pixels are the grey area outside the sheet.
const PAPER_HEIGHT: u32 = 734;
const PAPER_BOTTOM_MARGIN: u32 = 24;
const FONT_HEIGHT: u32 = 23;
const TEXT_COLOR: [u8; 3] = [17, 15, 14];

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaperRow {
    name: String,
    time: String,
}

#[derive(Debug, Clone, Copy)]
struct PaperLayout {
    columns: usize,
    rows_per_column: usize,
    column_width: u32,
    scale: f32,
    line_height: u32,
}

/// Update `file_schedule` to a cached drawing of the current trip. The renderer already
/// handles the model's `[matl_freetex]` slot, so switching this string updates the paper.
pub(crate) fn update_vehicle(
    vehicle: &mut VehicleInstance,
    duty: &PlayerDuty,
    fonts: &mut omsi_sim::texttex::FontLibrary,
) -> Result<()> {
    let (arr, dep) = tt_labels();
    let (title, rows) = paper_content(&duty.line, &duty.tour, &duty.trips, duty.trip_index, (arr, dep));
    let signature = content_signature(&title, &rows);
    let path = cache_dir()?.join(format!("schedule-v5-{signature:016x}.png"));
    let filename = path.to_string_lossy().into_owned();

    if vehicle.str_var("file_schedule") == filename {
        return Ok(());
    }

    if !path.is_file() {
        let Some(font) = schedule_font(fonts, &title, &rows) else {
            set_filename(vehicle, "");
            return Err(anyhow!(
                "no OMSI bitmap font is available for the driver's timetable"
            ));
        };
        let mut image = paper_base();
        draw_schedule(&mut image, &font, &title, &rows);
        save_png(&image, &path)?;
    }
    set_filename(vehicle, &filename);
    Ok(())
}

fn cache_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("cannot locate the openOMSI user data folder"))?;
    Ok(home.join(".openomsi").join("cache").join("schedules"))
}

fn set_filename(vehicle: &mut VehicleInstance, value: &str) {
    if let Some(i) = vehicle.ty.program.str_var("file_schedule") {
        vehicle.state.str_vars[i as usize] = value.to_string();
    } else {
        log::debug!(
            "{} has no file_schedule string variable",
            vehicle.ty.def.type_name
        );
    }
}

fn schedule_font(
    fonts: &mut omsi_sim::texttex::FontLibrary,
    title: &str,
    rows: &[PaperRow],
) -> Option<std::sync::Arc<FontAtlas>> {
    typewriter_font(title, rows)
        .map(std::sync::Arc::new)
        .or_else(|| {
            ["19_HHAschedule_font", "DIN Narrow", "DIN_Narrow", "DIN"]
                .into_iter()
                .find_map(|name| fonts.load(name))
        })
}

/// OMSI prints the schedule in a bold typewriter face, rather than a bus display's
/// proportional bitmap font. Rasterize an installed equivalent into fixed-width cells.
fn typewriter_font(title: &str, rows: &[PaperRow]) -> Option<FontAtlas> {
    static FONT: OnceLock<Option<FontVec>> = OnceLock::new();
    let font = FONT
        .get_or_init(|| {
            #[cfg(target_os = "windows")]
            let paths = [std::env::var_os("WINDIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("C:/Windows"))
                .join("Fonts/courbd.ttf")];
            #[cfg(target_os = "macos")]
            let paths = [PathBuf::from(
                "/System/Library/Fonts/Supplemental/Courier New Bold.ttf",
            )];
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            let paths = [
                PathBuf::from("/usr/share/fonts/truetype/liberation2/LiberationMono-Bold.ttf"),
                PathBuf::from("/usr/share/fonts/truetype/liberation/LiberationMono-Bold.ttf"),
                PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"),
            ];
            paths
                .iter()
                .find_map(|path| FontVec::try_from_vec(std::fs::read(path).ok()?).ok())
        })
        .as_ref()?;
    let scaled = font.as_scaled(PxScale::from(FONT_HEIGHT as f32));
    let cell_width = scaled.h_advance(scaled.glyph_id('M')).ceil().max(1.0) as u32;
    let mut characters: Vec<char> = (32u8..=126).map(char::from).collect();
    characters.extend(title.chars());
    for row in rows {
        characters.extend(row.name.chars());
        characters.extend(row.time.chars());
    }
    characters.sort_unstable();
    characters.dedup();
    characters.retain(|&ch| scaled.glyph_id(ch).0 != 0);
    let width = cell_width * characters.len() as u32;
    let mut alpha = vec![0; (width * FONT_HEIGHT * 4) as usize];
    let mut chars = Vec::with_capacity(characters.len());
    for (index, ch) in characters.into_iter().enumerate() {
        let x0 = index as u32 * cell_width;
        chars.push(FontChar {
            ch,
            x0: x0 as i32,
            x1: (x0 + cell_width) as i32,
            y: 0,
        });
        let glyph = scaled.glyph_id(ch).with_scale_and_position(
            PxScale::from(FONT_HEIGHT as f32),
            ab_glyph::point(0.0, scaled.ascent()),
        );
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|px, py, coverage| {
                let x = px as i32 + bounds.min.x as i32;
                let y = py as i32 + bounds.min.y as i32;
                if x >= 0 && x < cell_width as i32 && y >= 0 && y < FONT_HEIGHT as i32 {
                    let offset = ((y as u32 * width + x0 + x as u32) * 4) as usize;
                    let a = (coverage * 255.0).round() as u8;
                    alpha[offset..offset + 4].fill(a);
                }
            });
        }
    }
    Some(FontAtlas::new(
        Font {
            name: "openOMSI timetable".into(),
            height: FONT_HEIGHT as i32,
            gap: 0,
            chars,
            ..Default::default()
        },
        width,
        FONT_HEIGHT,
        vec![0; alpha.len()],
        alpha,
    ))
}

/// `TT_Arr` and `TT_Dep` of the game's language (`Languages/<LANG>_basic.olf`), as
/// Omsi.exe translates them for the paper: "Arrival " and "Depart." in English.
fn tt_labels() -> &'static (String, String) {
    static LABELS: OnceLock<(String, String)> = OnceLock::new();
    LABELS.get_or_init(|| {
        let lang = crate::settings::Settings::load().language;
        let find = |lang: &str| {
            omsi_cfg::content_dirs("Languages").into_iter().find_map(|dir| {
                omsi_content::language::Language::load(&dir.join(format!("{lang}_basic.olf"))).ok()
            })
        };
        let l = find(&lang).or_else(|| find("ENG"));
        let get = |key: &str, default: &str| {
            l.as_ref()
                .and_then(|l| l.strings.get(key))
                .filter(|v| !v.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| default.to_string())
        };
        (get("TT_Arr", "Arrival "), get("TT_Dep", "Depart. "))
    })
}

/// The rows of the paper as Omsi.exe makes them (0x7e72a0): every stop of the trip with
/// its departure (a station the profile passes is left out), the last one as
/// "<stop> <TT_Arr>" with its arrival, then one more row "<TT_Dep>" with the last stop's
/// departure.
fn paper_content(
    duty_line: &str,
    duty_tour: &str,
    trips: &[crate::schedule::PlannedTrip],
    trip_index: usize,
    (arr_label, dep_label): (&str, &str),
) -> (String, Vec<PaperRow>) {
    let trip = &trips[trip_index];
    let line = if trip.line.trim().is_empty() {
        duty_line.trim()
    } else {
        trip.line.trim()
    };
    let title = format!("{line} - {} - {}", trip.terminus.trim(), duty_tour.trim());

    let mut rows = Vec::new();
    let Some(last) = trip.stops.len().checked_sub(1) else {
        return (title, rows);
    };
    for i in 0..=last + 1 {
        let stop = &trip.stops[i.min(last)];
        if i < last && !stop.stops {
            continue;
        }
        // (the station's name, a blank and its second name - empty for a map's bus stop -
        // and another blank)
        let name = if i > last {
            dep_label.to_string()
        } else if i == last {
            format!("{}  {arr_label}", stop.name.trim())
        } else {
            format!("{}  ", stop.name.trim())
        };
        let time = format_time(if i == last { stop.arr } else { stop.dep });
        rows.push(PaperRow { name, time });
    }
    (title, rows)
}

fn format_time(seconds: f64) -> String {
    let minute = (seconds / 60.0).round() as i64;
    let minute = minute.rem_euclid(24 * 60);
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

fn content_signature(title: &str, rows: &[PaperRow]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    title.hash(&mut hasher);
    for row in rows {
        row.name.hash(&mut hasher);
        row.time.hash(&mut hasher);
    }
    hasher.finish()
}

fn paper_base() -> Image {
    let dirs = omsi_cfg::content_dirs("Texture");
    let refs: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
    omsi_texture::find_texture("Schedule.bmp", &refs)
        .and_then(|path| omsi_texture::decode_file(&path).ok())
        .unwrap_or_else(|| Image {
            width: 1024,
            height: 1024,
            rgba: [230, 227, 216, 255]
                .into_iter()
                .cycle()
                .take(1024 * 1024 * 4)
                .collect(),
            has_alpha: false,
        })
}

fn draw_schedule(image: &mut Image, font: &FontAtlas, title: &str, rows: &[PaperRow]) {
    let native_height = font.font.height.max(1) as u32;
    let layout = paper_layout(image.width, image.height, native_height, rows.len());
    let title = fit_text(
        font,
        title,
        layout.scale,
        image.width.saturating_sub(2 * PAPER_X),
    );
    draw_text(image, font, &title, PAPER_X, PAPER_TOP, layout.scale);

    let equal_width = scaled_text_width(font, "=", layout.scale).max(1);
    let count = (image.width.saturating_sub(2 * PAPER_X) / equal_width).min(120) as usize;
    draw_text(
        image,
        font,
        &"=".repeat(count),
        PAPER_X,
        PAPER_TOP + scaled_height(native_height, layout.scale) + 4,
        layout.scale,
    );

    for (index, row) in rows.iter().enumerate() {
        let (column_x, y) = row_position(index, native_height, &layout);
        if y + layout.line_height > paper_height(image.height) {
            continue;
        }
        let time_width = scaled_text_width(font, &row.time, layout.scale);
        let time_x = column_x + layout.column_width.saturating_sub(time_width);
        let name_limit = time_x.saturating_sub(column_x + scaled_size(12, layout.scale));
        let name = fit_text(font, &row.name, layout.scale, name_limit);
        let name_width = scaled_text_width(font, &name, layout.scale);
        let dot_width = scaled_text_width(font, ".", layout.scale).max(1);
        let available =
            time_x.saturating_sub(column_x + name_width + scaled_size(12, layout.scale));
        let dots = (available / dot_width).min(96) as usize;
        draw_text(image, font, &name, column_x, y, layout.scale);
        draw_text(
            image,
            font,
            &".".repeat(dots),
            column_x + name_width + scaled_size(9, layout.scale),
            y,
            layout.scale,
        );
        draw_text(image, font, &row.time, time_x, y, layout.scale);
    }
}

fn paper_layout(
    image_width: u32,
    image_height: u32,
    line_height: u32,
    row_count: usize,
) -> PaperLayout {
    let single_column_rows = full_size_rows(image_height, line_height);
    let columns = if row_count > single_column_rows { 2 } else { 1 };
    let rows_per_column = if columns == 2 {
        single_column_rows.max(row_count.div_ceil(2))
    } else {
        row_count
    };
    let column_width = image_width.saturating_sub(2 * PAPER_X + COLUMN_GAP) / 2;
    let scale = fit_scale(image_height, line_height, rows_per_column);
    PaperLayout {
        columns,
        rows_per_column,
        column_width,
        scale,
        line_height: scaled_height(line_height, scale),
    }
}

fn row_position(index: usize, native_height: u32, layout: &PaperLayout) -> (u32, u32) {
    let column = usize::from(layout.columns == 2 && index >= layout.rows_per_column);
    let row = index.saturating_sub(column * layout.rows_per_column);
    let x = PAPER_X + column as u32 * (layout.column_width + COLUMN_GAP);
    let y = PAPER_TOP + native_height + ROWS_TOP_GAP + row as u32 * layout.line_height;
    (x, y)
}

fn full_size_rows(image_height: u32, line_height: u32) -> usize {
    let rows_top = PAPER_TOP + line_height + ROWS_TOP_GAP;
    let available = paper_height(image_height).saturating_sub(rows_top + PAPER_BOTTOM_MARGIN);
    (available / line_height.max(1)) as usize
}

fn fit_scale(image_height: u32, line_height: u32, row_count: usize) -> f32 {
    if row_count == 0 {
        return 1.0;
    }
    let rows_top = PAPER_TOP + line_height + ROWS_TOP_GAP;
    let available = paper_height(image_height).saturating_sub(rows_top + PAPER_BOTTOM_MARGIN);
    ((available as f32 / (row_count as f32 * line_height as f32)).min(1.0)).max(0.01)
}

fn paper_height(image_height: u32) -> u32 {
    image_height.saturating_mul(PAPER_HEIGHT) / 1024
}

fn scaled_size(size: u32, scale: f32) -> u32 {
    ((size as f32 * scale).round() as u32).max(1)
}

fn scaled_height(size: u32, scale: f32) -> u32 {
    ((size as f32 * scale).floor() as u32).max(1)
}

fn scaled_text_width(font: &FontAtlas, text: &str, scale: f32) -> u32 {
    scaled_size(font.text_width(text).max(0) as u32, scale)
}

/// Keep the font size and the clock's position fixed. Long labels end with dots before
/// they reach the time field; remove whole characters so UTF-8 stop names stay valid.
fn fit_text<'a>(font: &FontAtlas, text: &'a str, scale: f32, max_width: u32) -> Cow<'a, str> {
    if scaled_text_width(font, text, scale) <= max_width {
        return Cow::Borrowed(text);
    }
    if scaled_text_width(font, "...", scale) > max_width {
        return Cow::Borrowed("");
    }
    let mut shortened = text.to_string();
    loop {
        let length = shortened.len();
        shortened.push_str("...");
        if scaled_text_width(font, &shortened, scale) <= max_width {
            return Cow::Owned(shortened);
        }
        shortened.truncate(length);
        shortened.pop();
    }
}

fn draw_text(image: &mut Image, font: &FontAtlas, text: &str, x: u32, y: u32, scale: f32) {
    if x >= image.width || y >= image.height {
        return;
    }
    let source_width = (font.text_width(text).max(0) as u32).max(1);
    let source_height = font.font.height.max(1) as u32;
    let rgba = font.render_aligned(
        text,
        source_width,
        source_height,
        false,
        TEXT_COLOR,
        TextAlign {
            orientation: 1,
            grid: 1,
        },
    );
    let width = scaled_size(source_width, scale).min(image.width - x);
    let height = scaled_height(source_height, scale).min(image.height - y);
    for py in 0..height {
        for px in 0..width {
            let source_x = ((px as f32 / scale).floor() as u32).min(source_width - 1);
            let source_y = ((py as f32 / scale).floor() as u32).min(source_height - 1);
            let source = ((source_y * source_width + source_x) * 4) as usize;
            let alpha = rgba[source + 3] as u32;
            if alpha == 0 {
                continue;
            }
            let target = (((y + py) * image.width + x + px) * 4) as usize;
            for channel in 0..3 {
                let ink = TEXT_COLOR[channel] as u32;
                let paper = image.rgba[target + channel] as u32;
                image.rgba[target + channel] = ((ink * alpha + paper * (255 - alpha)) / 255) as u8;
            }
            image.rgba[target + 3] = 255;
        }
    }
}

fn save_png(image: &Image, path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("schedule cache path has no parent"))?;
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let buffer = image::RgbaImage::from_raw(image.width, image.height, image.rgba.clone())
        .ok_or_else(|| anyhow!("schedule image has the wrong pixel count"))?;
    buffer
        .save(path)
        .with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::{PlannedStop, PlannedTrip, StopDir};
    use omsi_content::font::FontChar;

    fn stop(id: i64, name: &str, arr: f64, dep: f64) -> PlannedStop {
        PlannedStop {
            object_id: id,
            name: name.into(),
            arr,
            dep,
            position: None,
            dir: StopDir::default(),
            stops: true,
        }
    }

    fn test_font() -> FontAtlas {
        let chars: Vec<_> = (32u8..=126)
            .enumerate()
            .map(|(index, byte)| FontChar {
                ch: byte as char,
                x0: index as i32 * 4,
                x1: (index as i32 + 1) * 4,
                y: 0,
            })
            .collect();
        let width = chars.len() as u32 * 4;
        let pixels = vec![255; (width * FONT_HEIGHT * 4) as usize];
        FontAtlas::new(
            omsi_content::font::Font {
                height: FONT_HEIGHT as i32,
                gap: 1,
                chars,
                ..Default::default()
            },
            width,
            FONT_HEIGHT,
            pixels.clone(),
            pixels,
        )
    }

    #[test]
    fn paper_rows_end_with_arrival_and_departure_of_the_last_stop() {
        let t = |h: f64, m: f64| h * 3600.0 + m * 60.0;
        let mut passed = stop(2, "Feld", t(11.0, 54.0), t(11.0, 54.0));
        passed.stops = false;
        let current = PlannedTrip {
            name: "76_Kk-BH".into(),
            line: "76".into(),
            terminus: "Bauernhof".into(),
            departure: t(11.0, 52.0),
            end: t(11.0, 59.0),
            stops: vec![
                stop(1, "Krankenhaus", t(11.0, 50.0), t(11.0, 52.0)),
                passed,
                stop(3, "Dorf", t(11.0, 55.0), t(11.0, 57.0)),
                stop(4, "Bauernhof", t(11.0, 59.0), t(12.0, 1.0)),
            ],
        };
        let (title, rows) = paper_content("76", "1", &[current], 0, ("Arrival ", "Depart. "));
        assert_eq!(title, "76 - Bauernhof - 1");
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["Krankenhaus  ", "Dorf  ", "Bauernhof  Arrival ", "Depart. "]
        );
        assert_eq!(
            rows.iter().map(|r| r.time.as_str()).collect::<Vec<_>>(),
            ["11:52", "11:57", "11:59", "12:01"]
        );
    }

    #[test]
    fn paper_times_wrap_after_midnight() {
        assert_eq!(format_time(24.0 * 3600.0 + 7.0 * 60.0), "00:07");
    }

    #[test]
    fn long_schedule_uses_two_columns_and_keeps_every_row_on_the_paper() {
        let layout = paper_layout(1024, 1024, FONT_HEIGHT, 36);
        assert_eq!(layout.columns, 2);
        assert_eq!(layout.rows_per_column, 24);
        assert_eq!(layout.scale, 1.0);

        let short = paper_layout(1024, 1024, FONT_HEIGHT, 9);
        assert_eq!(short.columns, 1);
        assert_eq!(short.scale, 1.0);

        let extra_long = paper_layout(1024, 1024, FONT_HEIGHT, 72);
        assert_eq!(extra_long.columns, 2);
        assert!(extra_long.scale < 1.0);
        let rows_top = PAPER_TOP + FONT_HEIGHT + ROWS_TOP_GAP;
        assert!(
            rows_top + extra_long.line_height * extra_long.rows_per_column as u32
                <= PAPER_HEIGHT - PAPER_BOTTOM_MARGIN
        );

        let font = test_font();
        let rows: Vec<_> = (0..36)
            .map(|i| PaperRow {
                name: format!("Stop {i:02}"),
                time: "12:34".into(),
            })
            .collect();
        let mut image = Image {
            width: 1024,
            height: 1024,
            rgba: [255, 255, 255, 255]
                .into_iter()
                .cycle()
                .take(1024 * 1024 * 4)
                .collect(),
            has_alpha: false,
        };
        draw_schedule(&mut image, &font, "76 - Dense - 1", &rows);

        for i in 0..36 {
            let (x, y) = row_position(i, FONT_HEIGHT, &layout);
            let has_ink = (y..y + layout.line_height).any(|py| {
                (x..x + 80).any(|px| {
                    let pixel = ((py * image.width + px) * 4) as usize;
                    image.rgba[pixel] < 100
                })
            });
            assert!(has_ink, "schedule row {i} was not rendered");
        }
    }

    #[test]
    fn long_names_keep_the_clock_and_column_gap_clear() {
        let font = test_font();
        for row_count in [9, 36, 72] {
            let rows: Vec<_> = (0..row_count)
                .map(|i| PaperRow {
                    name: "Gustav-Adolf-Str./Langhansstr.".repeat(4),
                    time: if i % 2 == 0 { "12:34" } else { "12:34 - 12:36" }.into(),
                })
                .collect();
            let layout = paper_layout(1024, 1024, FONT_HEIGHT, row_count);
            let mut image = Image {
                width: 1024,
                height: 1024,
                rgba: vec![255; 1024 * 1024 * 4],
                has_alpha: false,
            };
            let mut clocks = image.clone();
            draw_schedule(
                &mut image,
                &font,
                "156 - Stad. Buschall/Hansastr. - 3 (Mo-Fr)",
                &rows,
            );
            for (i, row) in rows.iter().enumerate() {
                let (x, y) = row_position(i, FONT_HEIGHT, &layout);
                let time_x =
                    x + layout.column_width - scaled_text_width(&font, &row.time, layout.scale);
                draw_text(&mut clocks, &font, &row.time, time_x, y, layout.scale);
                for py in y..y + layout.line_height {
                    for px in time_x..x + layout.column_width {
                        let pixel = ((py * image.width + px) * 4) as usize;
                        assert_eq!(
                            &image.rgba[pixel..pixel + 4],
                            &clocks.rgba[pixel..pixel + 4],
                            "name overlaps time at row {i} of {row_count}"
                        );
                    }
                    for px in x + layout.column_width..x + layout.column_width + COLUMN_GAP {
                        let pixel = ((py * image.width + px) * 4) as usize;
                        assert_eq!(image.rgba[pixel], 255, "name overflows column at row {i}");
                    }
                }
            }
            assert!(image.rgba[(PAPER_HEIGHT * image.width * 4) as usize..]
                .iter()
                .all(|&v| v == 255));
        }
    }

    #[test]
    fn fitted_labels_keep_complete_unicode_characters() {
        let font = test_font();
        let name = "Südstadt/Gustav-Adolf-Straße Ankunft";
        for scale in [1.0, 0.75, 0.5] {
            let fitted = fit_text(&font, name, scale, 60);
            assert!(fitted.ends_with("..."));
            assert!(name.starts_with(fitted.trim_end_matches('.')));
            assert!(scaled_text_width(&font, &fitted, scale) <= 60);
        }
        assert_eq!(fit_text(&font, "Kurz", 1.0, 60), "Kurz");
        assert_eq!(fit_text(&font, name, 1.0, 5), "");
    }
}
