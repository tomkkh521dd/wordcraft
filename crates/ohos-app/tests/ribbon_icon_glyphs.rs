//! The ribbon's icons are drawn in code, but some of their marks are *text* (`¶`, `π`, `Ω`, `あ`):
//! each needs a glyph in the font set the app builds, or epaint draws its `◻` box there.
//!
//! OHOS.3b: on HarmonyOS the Translate/Language icon was one of those boxes. The font set the app
//! builds already holds every other character the icons draw, so the check below is the guard that
//! keeps it that way (a new icon that writes a letter needs a face that has it).

use std::collections::BTreeSet;

use egui::{Color32, FontFamily, FontId, Rect, pos2, vec2};

/// A character no font has, so laying it out gives the glyph epaint draws for a missing one.
const ABSENT: char = '\u{10FFFD}';

/// Where the icon sweep paints (one row of 20 pt tiles, 25 pt apart).
const STRIP: Rect = Rect { min: pos2(0.0, 0.0), max: pos2(16000.0, 40.0) };

/// The interface fonts, exactly as the app builds them: the bundled families, egui's own fallback
/// faces, and whatever the UI registers after them (the app passes its input-method faces here).
fn app_fonts() -> egui::Context {
    let mut fonts = wordcraft_ui_egui::theme::font_definitions(false, false);
    wordcraft_ui_egui::theme::add_fallback_faces(&mut fonts, &[]);
    let ctx = egui::Context::default();
    ctx.set_fonts(fonts);
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(16000.0, 40.0))), ..Default::default() };
    // Fonts exist only after a pass (and the atlas deltas have to be applied or dropped).
    ctx.run_ui(raw, |_| {}).drop_without_applying_deltas();
    ctx
}

/// Whether the fonts behind `font` draw `c`. `Fonts::has_glyph` answers by *face*, so it reports
/// every character of the face holding the missing-character box as missing too (all of Inter, for
/// the `medium` and `semibold` families); the laid-out glyph itself does not lie: a character with
/// no face gets exactly the glyph [`ABSENT`] gets.
fn draws(ctx: &egui::Context, font: &FontId, c: char) -> bool {
    let uv = |c: char| {
        ctx.fonts_mut(|f| {
            let galley = f.layout_no_wrap(c.to_string(), font.clone(), Color32::BLACK);
            galley
                .rows
                .first()
                .and_then(|r| r.glyphs.first())
                .map_or([0; 4], |g| [g.uv_rect.min[0], g.uv_rect.min[1], g.uv_rect.max[0], g.uv_rect.max[1]])
        })
    };
    uv(c) != uv(ABSENT)
}

/// Every (character, font) an icon writes as text, painted from the icon set itself so the icons and
/// this test cannot drift apart: each command's ribbon icon is painted, and the text shapes it adds
/// are read back.
fn icon_text() -> Vec<(char, FontId)> {
    let registry = wordcraft_engine::cmd::registry();
    let mut names = BTreeSet::new();
    for spec in registry.all() {
        names.insert(wordcraft_ui_egui::widgets::command_icon(spec.id));
    }
    assert!(names.len() > 300, "the registry's icons are the sweep: {} names", names.len());
    let ctx = app_fonts();
    let raw = egui::RawInput { screen_rect: Some(STRIP), ..Default::default() };
    let mut output = ctx.run_ui(raw, |ui| {
        let painter = ui.painter().clone();
        let mut x = STRIP.min.x;
        for name in &names {
            wordcraft_ui_egui::icons::paint(&painter, Rect::from_min_size(pos2(x, 0.0), vec2(20.0, 20.0)), name, Color32::BLACK, Color32::BLUE);
            x += 25.0;
        }
    });
    output.textures_delta.clear();
    let mut found = Vec::new();
    let mut pending: Vec<&egui::Shape> = output.shapes.iter().map(|c| &c.shape).collect();
    while let Some(shape) = pending.pop() {
        match shape {
            egui::Shape::Text(text) => {
                for section in &text.galley.job.sections {
                    let run = text.galley.job.text.get(section.byte_range.start.0..section.byte_range.end.0).unwrap_or_default();
                    for c in run.chars().filter(|c| !c.is_control() && !c.is_whitespace()) {
                        let font = section.format.font_id.clone();
                        if !found.iter().any(|(f, k)| *f == c && *k == font) {
                            found.push((c, font));
                        }
                    }
                }
            }
            egui::Shape::Vec(shapes) => pending.extend(shapes.iter()),
            _ => {}
        }
    }
    found
}

/// The families the app builds keep egui's own fallback faces (the `default_fonts` feature): the
/// interface font list starts from `FontDefinitions::default()`, which is empty without it.
#[test]
fn the_app_fonts_keep_egui_fallback_faces() {
    let fonts = wordcraft_ui_egui::theme::font_definitions(false, false);
    let proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    assert!(proportional.contains(&"Inter".to_owned()), "the bundled interface face leads: {proportional:?}");
    assert!(proportional.contains(&"NotoEmoji-Regular".to_owned()), "egui's fallback faces: {proportional:?}");
}

/// Every character the ribbon's icons write as text has a glyph — on a build without craft-fonts
/// (no CJK face) and on one with it, so an icon cannot fall back to the `◻` box.
#[test]
fn every_ribbon_icon_character_has_a_glyph() {
    let ctx = app_fonts();
    let text = icon_text();
    assert!(text.len() > 60, "the icons write letters as text: {} (character, font) pairs", text.len());
    let missing: Vec<String> =
        text.iter().filter(|(c, font)| !draws(&ctx, font, *c)).map(|(c, font)| format!("{c} U+{:04X} in {font:?}", *c as u32)).collect();
    assert!(missing.is_empty(), "ribbon icons draw characters the interface fonts do not have: {missing:#?}");
}

/// The Translate/Language icon's second letter is a kana character, which only a CJK face can draw:
/// without one the icon writes the Greek `α` instead (OHOS.3b — the kana drew as a box on a build
/// without craft-fonts and without a CJK interface language).
#[test]
fn the_language_icon_does_not_need_a_cjk_face() {
    let ctx = app_fonts();
    let cjk = ctx.fonts_mut(|f| f.has_glyph(&FontId::proportional(12.0), '\u{3042}'));
    let drawn: Vec<char> = icon_text().iter().map(|(c, _)| *c).collect();
    if cjk {
        assert!(drawn.contains(&'\u{3042}'), "a CJK face is registered, so the icon keeps the kana: {drawn:?}");
    } else {
        assert!(drawn.contains(&'\u{3B1}'), "no face has the kana, so the icon writes α: {drawn:?}");
        assert!(!drawn.contains(&'\u{3042}'), "the kana would draw as a box: {drawn:?}");
    }
}
