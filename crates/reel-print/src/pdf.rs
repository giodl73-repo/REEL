use crate::{
    Page,
    pages::{Content, Sheet},
};
use anyhow::{Context, Result, ensure};
use lopdf::{Document, Object, ObjectId, dictionary};
use std::{collections::BTreeMap, sync::Arc};

fn inherited(doc: &Document, id: ObjectId, name: &[u8]) -> Result<Option<Object>> {
    let mut current = id;
    for _ in 0..32 {
        let dict = doc.get_object(current)?.as_dict()?;
        if let Ok(value) = dict.get(name) {
            return Ok(Some(value.clone()));
        }
        match dict.get(b"Parent").and_then(Object::as_reference) {
            Ok(parent) => current = parent,
            Err(_) => return Ok(None),
        }
    }
    anyhow::bail!("PDF page inheritance exceeds bound")
}
fn geometry(doc: &Document, id: ObjectId) -> Result<(f32, f32)> {
    let bounds = inherited(doc, id, b"MediaBox")?.context("page MediaBox absent")?;
    let bounds = match bounds {
        Object::Reference(id) => doc.get_object(id)?.clone(),
        v => v,
    };
    let v = bounds.as_array()?;
    ensure!(v.len() == 4, "invalid MediaBox");
    let x = v[2].as_float()? - v[0].as_float()?;
    let y = v[3].as_float()? - v[1].as_float()?;
    ensure!(
        x.is_finite()
            && y.is_finite()
            && (72.0..=2048.0).contains(&x)
            && (72.0..=2048.0).contains(&y),
        "PDF page geometry exceeds bound"
    );
    Ok((x, y))
}
pub fn retained(id: &str, raw: &[u8]) -> Result<Vec<Sheet>> {
    let doc = Arc::new(Document::load_mem(raw)?);
    ensure!(!doc.is_encrypted(), "encrypted PDF refused");
    ensure!(
        doc.get_pages().len() <= 1024,
        "retained PDF page budget exceeded"
    );
    // Static printing only. Do not carry actions, forms, attachments or scripts.
    for object in doc.objects.values() {
        if let Ok(dict) = object.as_dict() {
            if let Ok(kind) = dict.get(b"S").and_then(Object::as_name) {
                ensure!(
                    ![
                        b"URI".as_slice(),
                        b"Launch",
                        b"GoToR",
                        b"SubmitForm",
                        b"ImportData",
                        b"JavaScript",
                        b"Rendition"
                    ]
                    .contains(&kind),
                    "external PDF action refused"
                );
            }
            for name in [
                b"OpenAction".as_slice(),
                b"AA",
                b"JS",
                b"JavaScript",
                b"AcroForm",
                b"EmbeddedFiles",
                b"XFA",
            ] {
                ensure!(dict.get(name).is_err(), "active PDF content refused");
            }
        }
    }
    doc.get_pages()
        .values()
        .map(|page| {
            let (width, height) = geometry(&doc, *page)?;
            Ok(Sheet {
                description: Page {
                    entry: id.into(),
                    kind: "retained-pdf".into(),
                    width,
                    height,
                    source_band: None,
                },
                content: Content::Pdf(doc.clone()),
            })
        })
        .collect()
}
pub fn merge(parts: Vec<Arc<Document>>) -> Result<Vec<u8>> {
    let mut output = Document::with_version("1.7");
    let mut objects = BTreeMap::new();
    let mut pages = Vec::new();
    let mut next = 1;
    for part in parts {
        let mut doc = (*part).clone();
        doc.renumber_objects_with(next);
        next = doc
            .max_id
            .checked_add(1)
            .context("PDF object id overflow")?;
        for id in doc.get_pages().values() {
            let mut page = doc.get_object(*id)?.as_dict()?.clone();
            // Preserve inherited resources and geometry before assigning a new parent.
            for key in [b"Resources".as_slice(), b"MediaBox", b"CropBox", b"Rotate"] {
                if let Some(value) = inherited(&doc, *id, key)? {
                    page.set(key, value);
                }
            }
            geometry(&doc, *id)?;
            doc.objects.insert(*id, Object::Dictionary(page));
            pages.push(*id);
        }
        objects.extend(doc.objects);
    }
    ensure!(
        !pages.is_empty() && pages.len() <= 1024,
        "invalid final PDF page count"
    );
    let parent = (next, 0);
    let catalog = (next + 1, 0);
    for id in &pages {
        objects
            .get_mut(id)
            .context("page vanished")?
            .as_dict_mut()?
            .set("Parent", parent);
    }
    objects.insert(parent,dictionary!{"Type"=>"Pages","Count"=>pages.len() as i64,"Kids"=>pages.into_iter().map(Object::Reference).collect::<Vec<_>>()}.into());
    objects.insert(
        catalog,
        dictionary! {"Type"=>"Catalog","Pages"=>parent}.into(),
    );
    output.objects = objects;
    output.max_id = next + 1;
    output.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    output.save_to(&mut bytes)?;
    Ok(bytes)
}
