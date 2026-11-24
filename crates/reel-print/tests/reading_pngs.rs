use reel_print::vector::reading_pdf_from_pngs;
use resvg::tiny_skia::{Color, Pixmap};

#[test]
fn selected_images_become_ordered_lossless_pdf_pages_without_extra_sheets() {
    let mut a = Pixmap::new(2, 3).unwrap();
    a.fill(Color::WHITE);
    let mut b = Pixmap::new(3, 2).unwrap();
    b.fill(Color::from_rgba8(255, 0, 0, 255));
    let pngs = [a.encode_png().unwrap(), b.encode_png().unwrap()];
    let render =
        || reading_pdf_from_pngs(2, 1024 * 1024, 1024 * 1024, |i| Ok(pngs[i].clone())).unwrap();
    let bytes = render();
    assert_eq!(bytes, render());
    let doc = lopdf::Document::load_mem(&bytes).unwrap();
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 2);
    for (index, id) in pages.values().enumerate() {
        let page = doc.get_object(*id).unwrap().as_dict().unwrap();
        let box_ = page.get(b"MediaBox").unwrap().as_array().unwrap();
        let expected = if index == 0 {
            [480.0, 720.0]
        } else {
            [720.0, 480.0]
        };
        assert_eq!(box_[2].as_float().unwrap(), expected[0]);
        assert_eq!(box_[3].as_float().unwrap(), expected[1]);
        let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
        let image = resources
            .get(b"XObject")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Im0")
            .unwrap()
            .as_reference()
            .unwrap();
        let stream = doc.get_object(image).unwrap().as_stream().unwrap();
        let rgb = stream.decompressed_content().unwrap();
        assert_eq!(
            rgb,
            if index == 0 {
                vec![255; 18]
            } else {
                [255, 0, 0].repeat(6)
            }
        );
    }
}
#[test]
fn reading_png_assembly_refuses_alpha_and_declared_byte_overflow() {
    let transparent = Pixmap::new(2, 2).unwrap().encode_png().unwrap();
    assert!(reading_pdf_from_pngs(1, 1024, 1024 * 1024, |_| Ok(transparent.clone())).is_err());
    let mut opaque = Pixmap::new(2, 2).unwrap();
    opaque.fill(Color::WHITE);
    let raw = opaque.encode_png().unwrap();
    assert!(reading_pdf_from_pngs(1, 1, 1024 * 1024, |_| Ok(raw.clone())).is_err());
    assert!(reading_pdf_from_pngs(1, 1024, 1, |_| Ok(raw.clone())).is_err());
}

#[test]
#[ignore = "Read-only production evidence: set REEL_READING_PDF and REEL_READING_ORDER (JSON array of ordered PNG paths)"]
fn existing_pdf_embeds_every_selected_page_with_exact_opaque_pixels() {
    let paths: Vec<std::path::PathBuf> = serde_json::from_slice(
        &std::fs::read(std::env::var_os("REEL_READING_ORDER").unwrap()).unwrap(),
    )
    .unwrap();
    assert!(!paths.is_empty() && paths.len() <= 128);
    let raw = std::fs::read(std::env::var_os("REEL_READING_PDF").unwrap()).unwrap();
    assert!(raw.len() <= 256 * 1024 * 1024);
    let doc = lopdf::Document::load_mem(&raw).unwrap();
    let pages = doc.get_pages();
    assert_eq!(pages.len(), paths.len());
    for (id, path) in pages.values().zip(paths) {
        let png = std::fs::read(path).unwrap();
        assert!(png.len() <= 16 * 1024 * 1024);
        let pixels = Pixmap::decode_png(&png).unwrap();
        let page = doc.get_object(*id).unwrap().as_dict().unwrap();
        let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
        let objects = resources.get(b"XObject").unwrap().as_dict().unwrap();
        assert_eq!(objects.len(), 1);
        let image = objects.get(b"Im0").unwrap().as_reference().unwrap();
        let stream = doc.get_object(image).unwrap().as_stream().unwrap();
        assert_eq!(
            stream.dict.get(b"Width").unwrap().as_i64().unwrap(),
            i64::from(pixels.width())
        );
        assert_eq!(
            stream.dict.get(b"Height").unwrap().as_i64().unwrap(),
            i64::from(pixels.height())
        );
        let rgb = stream.decompressed_content().unwrap();
        assert_eq!(rgb.len(), pixels.data().len() / 4 * 3);
        for (actual, expected) in rgb.chunks_exact(3).zip(pixels.data().chunks_exact(4)) {
            assert_eq!(expected[3], 255);
            assert_eq!(actual, &expected[..3]);
        }
    }
}
