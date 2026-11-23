//! Bounded in-memory composition of existing edits; no intermediate publication.
use super::*;

/// Hard operation ceiling in addition to the existing per-operation limits.
pub const MAX_CHANGES: usize = 64;

struct Overlay<'a, R> {
    reader: &'a mut R,
    limits: Limits,
    used: usize,
    objects: BTreeMap<Object, Vec<u8>>,
}
impl<R> Overlay<'_, R> {
    fn room(&self, object: &Object) -> std::result::Result<(), revisions::Error> {
        if self.objects.contains_key(object) {
            return Ok(());
        }
        if self.objects.len() >= self.limits.documents
            || object.bytes > self.limits.bytes.saturating_sub(self.used) as u64
        {
            return Err(revisions::Error::Budget);
        }
        Ok(())
    }
    fn insert(
        &mut self,
        object: &Object,
        bytes: Vec<u8>,
    ) -> std::result::Result<(), revisions::Error> {
        self.room(object)?;
        if Object::from_bytes(&bytes) != *object {
            return Err(revisions::Error::Pin);
        }
        if !self.objects.contains_key(object) {
            self.used += bytes.len();
            self.objects.insert(object.clone(), bytes);
        }
        Ok(())
    }
}
impl<R: Reader> Reader for Overlay<'_, R> {
    fn read(
        &mut self,
        object: &Object,
        maximum: usize,
    ) -> std::result::Result<Vec<u8>, revisions::Error> {
        if object.bytes > maximum as u64 {
            return Err(revisions::Error::Budget);
        }
        self.room(object)?;
        if let Some(bytes) = self.objects.get(object) {
            return Ok(bytes.clone());
        }
        let bytes = self.reader.read(object, maximum)?;
        self.insert(object, bytes.clone())?;
        Ok(bytes)
    }
}

/// Apply an explicit ordered sequence to one exact original record. Every step
/// uses the normal validator. New metadata feeds later steps without writes;
/// unique old/new metadata shares one aggregate document/byte budget. Existing
/// reference/CPU limits apply per step, with at most `MAX_CHANGES` steps.
/// Failure returns no candidate. Success still requires existing publication,
/// independent readback and expected-original-base Git adoption.
pub fn prepare(
    inputs: Inputs<'_>,
    changes: Vec<Change<'_>>,
    reader: &mut impl Reader,
) -> Result<Prepared> {
    if changes.is_empty()
        || changes.len() > MAX_CHANGES
        || [
            inputs.instance.len(),
            inputs.definition.len(),
            inputs.authored_schema.len(),
        ]
        .into_iter()
        .any(|bytes| bytes > 65_536)
    {
        return Err(Error::Budget);
    }
    if Object::from_bytes(inputs.instance) != *inputs.expected {
        return Err(Error::Stale);
    }
    let mut overlay = Overlay {
        reader,
        limits: inputs.limits,
        used: 0,
        objects: BTreeMap::new(),
    };
    let mut record = inputs.instance.to_vec();
    let mut metadata = BTreeMap::new();
    for change in changes {
        let expected = Object::from_bytes(&record);
        let prepared = super::prepare(
            Inputs {
                expected: &expected,
                instance: &record,
                definition: inputs.definition,
                authored_schema: inputs.authored_schema,
                dialect: inputs.dialect,
                limits: inputs.limits,
            },
            change,
            &mut overlay,
        )?;
        for blob in prepared.metadata {
            overlay
                .insert(&blob.object, blob.bytes.clone())
                .map_err(Error::Revision)?;
            metadata.entry(blob.object.clone()).or_insert(blob);
        }
        record = prepared.record.bytes;
    }
    let noop = identity::parse_json(&record).map_err(|_| Error::Native)?
        == identity::parse_json(inputs.instance).map_err(|_| Error::Native)?;
    if noop {
        record = inputs.instance.to_vec();
        metadata.clear();
    }
    Ok(Prepared {
        expected: inputs.expected.clone(),
        record: Blob {
            object: Object::from_bytes(&record),
            bytes: record,
        },
        metadata: metadata.into_values().collect(),
        noop,
    })
}
