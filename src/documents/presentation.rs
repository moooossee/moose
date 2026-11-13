use crate::i18n::gettext;

use super::{
    archive::Archive,
    content::{Content, xml},
    office::{content_types, relationships},
};

const DRAWING: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PRESENTATION: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const GROUP: &str = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;

pub fn slides(content: &Content) -> Result<Vec<(String, Vec<String>)>, String> {
    let mut result = Vec::new();
    for (title, paragraphs) in content.slides() {
        let mut lines = Vec::new();
        for paragraph in paragraphs {
            for line in paragraph.lines() {
                let mut current = String::new();
                for word in line.split_whitespace() {
                    if !current.is_empty() && current.chars().count() + word.chars().count() > 70 {
                        lines.push(std::mem::take(&mut current));
                    }
                    if !current.is_empty() {
                        current.push(' ');
                    }
                    current.push_str(word);
                }
                if !current.is_empty() {
                    lines.push(current);
                }
            }
        }
        if lines.is_empty() {
            result.push((title, Vec::new()));
        } else {
            for (index, part) in lines.chunks(10).enumerate() {
                result.push((
                    if index == 0 {
                        title.clone()
                    } else {
                        format!("{title} — continued")
                    },
                    part.to_vec(),
                ));
            }
        }
    }
    if result.is_empty() || result.len() > 300 {
        return Err(gettext("Use a presentation with between 1 and 300 slides."));
    }
    Ok(result)
}

pub fn pptx(content: &Content) -> Result<Vec<u8>, String> {
    let slides = slides(content)?;
    let mut archive = Archive::new();
    let mut overrides = String::from(
        r#"<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>"#,
    );
    let mut slide_ids = String::new();
    let mut slide_rels = Vec::new();
    for (index, (title, lines)) in slides.iter().enumerate() {
        let number = index + 1;
        slide_ids.push_str(&format!(
            r#"<p:sldId id="{}" r:id="rId{number}"/>"#,
            index + 256
        ));
        slide_rels.push((format!("rId{number}"), format!("slides/slide{number}.xml")));
        overrides.push_str(&format!(r#"<Override PartName="/ppt/slides/slide{number}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#));
        let title = shape(
            2,
            &gettext("Title"),
            650_000,
            450_000,
            10_900_000,
            950_000,
            &[(title.clone(), 3000, "1C3557", true)],
        );
        let paragraphs = lines
            .iter()
            .map(|line| (line.clone(), 2000, "263238", false))
            .collect::<Vec<_>>();
        let body = shape(
            3,
            &gettext("Content"),
            650_000,
            1_650_000,
            10_900_000,
            4_500_000,
            &paragraphs,
        );
        let footer = shape(
            4,
            &gettext("Page"),
            650_000,
            6_300_000,
            10_900_000,
            250_000,
            &[(
                format!("{number} / {}", slides.len()),
                1100,
                "777777",
                false,
            )],
        );
        archive.add(&format!("ppt/slides/slide{number}.xml"), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld xmlns:a="{DRAWING}" xmlns:r="{REL}" xmlns:p="{PRESENTATION}"><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree>{GROUP}{title}{body}{footer}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#))?;
        archive.add(
            &format!("ppt/slides/_rels/slide{number}.xml.rels"),
            relationships(&[("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml")]),
        )?;
    }
    archive.add("[Content_Types].xml", content_types(r#"<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>"#, &overrides))?;
    archive.add(
        "_rels/.rels",
        relationships(&[("rId1", "officeDocument", "ppt/presentation.xml")]),
    )?;
    archive.add("ppt/presentation.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:presentation xmlns:a="{DRAWING}" xmlns:r="{REL}" xmlns:p="{PRESENTATION}"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rMaster"/></p:sldMasterIdLst><p:sldIdLst>{slide_ids}</p:sldIdLst><p:sldSz cx="12192000" cy="6858000" type="screen16x9"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#))?;
    let mut rels = slide_rels
        .iter()
        .map(|(id, target)| (id.as_str(), "slide", target.as_str()))
        .collect::<Vec<_>>();
    rels.push(("rMaster", "slideMaster", "slideMasters/slideMaster1.xml"));
    archive.add("ppt/_rels/presentation.xml.rels", relationships(&rels))?;
    archive.add("ppt/slideMasters/slideMaster1.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldMaster xmlns:a="{DRAWING}" xmlns:r="{REL}" xmlns:p="{PRESENTATION}"><p:cSld name="Moose"><p:spTree>{GROUP}</p:spTree></p:cSld><p:clrMap accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" bg1="lt1" bg2="lt2" folHlink="folHlink" hlink="hlink" tx1="dk1" tx2="dk2"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rLayout"/></p:sldLayoutIdLst><p:txStyles><p:titleStyle/><p:bodyStyle/><p:otherStyle/></p:txStyles></p:sldMaster>"#))?;
    archive.add(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        relationships(&[
            ("rLayout", "slideLayout", "../slideLayouts/slideLayout1.xml"),
            ("rTheme", "theme", "../theme/theme1.xml"),
        ]),
    )?;
    archive.add("ppt/slideLayouts/slideLayout1.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldLayout xmlns:a="{DRAWING}" xmlns:r="{REL}" xmlns:p="{PRESENTATION}" type="blank" preserve="1"><p:cSld name="Blank"><p:spTree>{GROUP}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#))?;
    archive.add(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        relationships(&[("rMaster", "slideMaster", "../slideMasters/slideMaster1.xml")]),
    )?;
    archive.add("ppt/theme/theme1.xml", theme())?;
    archive.finish()
}

fn shape(
    id: usize,
    name: &str,
    x: i64,
    y: i64,
    width: i64,
    height: i64,
    paragraphs: &[(String, usize, &str, bool)],
) -> String {
    let name = xml(name);
    let paragraphs = paragraphs.iter().map(|(text, size, color, bold)| format!(r#"<a:p><a:pPr><a:spcAft><a:spcPts val="800"/></a:spcAft></a:pPr><a:r><a:rPr lang="en-US" sz="{size}" b="{}"><a:solidFill><a:srgbClr val="{color}"/></a:solidFill><a:latin typeface="Calibri"/></a:rPr><a:t>{}</a:t></a:r><a:endParaRPr lang="en-US" sz="{size}"/></a:p>"#, usize::from(*bold), xml(text))).collect::<String>();
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{width}" cy="{height}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="square" lIns="0" tIns="0" rIns="0" bIns="0"><a:normAutofit/></a:bodyPr><a:lstStyle/>{paragraphs}</p:txBody></p:sp>"#
    )
}

fn theme() -> String {
    let colors = [
        ("dk1", "17212B"),
        ("lt1", "FFFFFF"),
        ("dk2", "1C3557"),
        ("lt2", "F1F5F9"),
        ("accent1", "3584E4"),
        ("accent2", "33A17A"),
        ("accent3", "E5A50A"),
        ("accent4", "9141AC"),
        ("accent5", "E66100"),
        ("accent6", "C01C28"),
        ("hlink", "1C71D8"),
        ("folHlink", "813D9C"),
    ]
    .into_iter()
    .map(|(name, color)| format!(r#"<a:{name}><a:srgbClr val="{color}"/></a:{name}>"#))
    .collect::<String>();
    let fill = r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#;
    let line = r#"<a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/></a:ln>"#;
    let effect = "<a:effectStyle><a:effectLst/></a:effectStyle>";
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><a:theme xmlns:a="{DRAWING}" name="Moose"><a:themeElements><a:clrScheme name="Moose">{colors}</a:clrScheme><a:fontScheme name="Moose"><a:majorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Moose"><a:fillStyleLst>{}</a:fillStyleLst><a:lnStyleLst>{}</a:lnStyleLst><a:effectStyleLst>{}</a:effectStyleLst><a:bgFillStyleLst>{}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#,
        fill.repeat(3),
        line.repeat(3),
        effect.repeat(3),
        fill.repeat(3)
    )
}
