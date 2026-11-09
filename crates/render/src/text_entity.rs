use super::{Bounds2, Builder, Color, Ctx, Drawing, Ink, Rgb, ink};

/// The text style of an entity (Standard defaults when missing).
pub(crate) fn text_style(d: &Drawing, name: &str) -> iridium_cad_doc::TextStyle {
    d.text_style(name).cloned().unwrap_or_default()
}

/// Shape a TEXT / ATTRIB / ATTDEF with its style's font and generation flags.
pub fn place_text_entity(d: &Drawing, t: &iridium_cad_doc::Text, value: &str) -> (iridium_cad_fonts::Shaped, Bounds2) {
    let ts = text_style(d, &t.style);
    let font = iridium_cad_fonts::TextFont::resolve(&ts.font);
    let (h, v) = align(t.halign, t.valign);
    let p = iridium_cad_fonts::TextParams {
        insert: t.insert.xy(),
        align_pt: t.align_pt.map(|p| p.xy()),
        height: t.height,
        rotation: t.rotation,
        width_factor: t.width_factor,
        oblique: t.oblique,
        h,
        v,
        backwards: ts.backwards,
        upside_down: ts.upside_down,
    };
    iridium_cad_fonts::place(&font, value, &p)
}

/// Lay out an MTEXT with its style's font.
pub fn layout_mtext_entity(d: &Drawing, t: &iridium_cad_doc::MText) -> iridium_cad_fonts::MTextLayout {
    let ts = text_style(d, &t.style);
    let params = iridium_cad_fonts::MTextParams {
        insert: t.insert.xy(),
        height: t.height,
        width: t.width,
        attach: t.attach,
        rotation: t.rotation,
        line_spacing: t.line_spacing,
        line_spacing_exact: t.line_spacing_exact,
        font: iridium_cad_fonts::TextFont::resolve(&ts.font),
        width_factor: ts.width_factor,
        oblique: ts.oblique,
        columns: t.columns.as_ref().map(|c| iridium_cad_fonts::MTextColumnSpec {
            dynamic: c.kind == iridium_cad_doc::MTextColumnType::Dynamic,
            count: usize::from(c.count.max(1)),
            gutter: c.gutter,
            height: c.height,
            heights: if c.kind == iridium_cad_doc::MTextColumnType::Dynamic && !c.auto_height { c.heights.clone() } else { Vec::new() },
            reversed: c.flow_reversed,
        }),
    };
    iridium_cad_fonts::layout_mtext_with(&t.contents, &params)
}

fn mtext_color(c: Option<iridium_cad_fonts::MTextColor>, rgb: Ink) -> Ink {
    match c {
        Some(iridium_cad_fonts::MTextColor::Aci(i)) => {
            u8::try_from(i).ok().filter(|i| *i > 0).map(|i| ink(Color::Index(i), Color::Index(7), Color::Index(7))).unwrap_or(rgb)
        }
        Some(iridium_cad_fonts::MTextColor::Rgb(v)) => {
            Ink { rgb: Rgb(((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8), aci7: false, aci: 0 }
        }
        None => rgb,
    }
}

pub(crate) fn mtext(b: &mut Builder, ctx: &Ctx, t: &iridium_cad_doc::MText, rgb: Ink, lw: f32) {
    let l = layout_mtext_entity(ctx.d, t);
    for p in &l.pieces {
        b.shaped(ctx, mtext_color(p.color, rgb), lw, &p.shaped);
    }
}

/// A TEXT's justification as the fonts' alignment (used where text is placed outside a build).
pub fn text_align(h: iridium_cad_doc::HAlign, v: iridium_cad_doc::VAlign) -> (iridium_cad_fonts::Align, iridium_cad_fonts::VAlign) {
    align(h, v)
}

fn align(h: iridium_cad_doc::HAlign, v: iridium_cad_doc::VAlign) -> (iridium_cad_fonts::Align, iridium_cad_fonts::VAlign) {
    use iridium_cad_fonts::{Align as A, VAlign as V};
    let ha = match h {
        iridium_cad_doc::HAlign::Left => A::Left,
        iridium_cad_doc::HAlign::Center => A::Center,
        iridium_cad_doc::HAlign::Right => A::Right,
        iridium_cad_doc::HAlign::Aligned => A::Aligned,
        iridium_cad_doc::HAlign::Middle => A::Middle,
        iridium_cad_doc::HAlign::Fit => A::Fit,
    };
    let va = match v {
        iridium_cad_doc::VAlign::Baseline => V::Baseline,
        iridium_cad_doc::VAlign::Bottom => V::Bottom,
        iridium_cad_doc::VAlign::Middle => V::Middle,
        iridium_cad_doc::VAlign::Top => V::Top,
    };
    (ha, va)
}
