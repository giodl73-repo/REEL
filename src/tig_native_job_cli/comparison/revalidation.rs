//! Read-only equivalence of original contracts under today's compiler. This is
//! deliberately not historical claim replay or a new producer correspondence.
use super::*;
use tig_core::{
    Digest,
    tree::{NodeStore, Tree},
};

const SEMANTICS: &[u8] = b"reel.native-language.render.experimental-1:derived-language-roots;leaf-payloads;compiled-contracts-and-native-authoring-in-frozen-parameters;full-retained-controls;explicit-producer";
const INPUTS: [&str; 3] = ["production.json", "contract.json", "job.json"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    schema: String,
    native: Language,
    compiler: Object,
    adapter: Object,
    authoring: Digest,
    compiled_inputs: BTreeMap<String, Object>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Language {
    schema: String,
    native: Scene,
    scope_adapter_sha256: Digest,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scene {
    schema: String,
    request: Request,
    scene_adapter_sha256: Digest,
    input_adapter_sha256: Digest,
    tig_source_pin_sha256: Digest,
    #[serde(default)]
    binding_adapter_sha256: Option<Digest>,
    #[serde(default)]
    control_adapter_sha256: Option<Digest>,
}

fn descriptor(
    raw: &[u8],
    request: &Request,
    operation: &tig_assets::graph::recipe::Operation,
) -> Result<Descriptor> {
    let d: Descriptor = serde_json::from_value(identity::parse_json(raw)?)?;
    if d.schema != "reel.tig-native-render-context.experimental-1"
        || d.native.schema != "reel.tig-native-language-context.experimental-1"
        || d.native.native.schema != "reel.tig-native-scene-context.experimental-1"
        || serde_json::to_value(&d.native.native.request)? != serde_json::to_value(request)?
        || d.native.native.binding_adapter_sha256.is_some()
            != d.native.native.control_adapter_sha256.is_some()
        || d.compiled_inputs
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != BTreeSet::from(INPUTS)
        || d.compiled_inputs
            .values()
            .chain([&d.compiler, &d.adapter])
            .any(|p| p.bytes == 0 || p.bytes > crate::tig_inputs::JOB_BYTES_MAX as u64)
        || operation.name != "reel.native-language.render.experimental-1"
        || operation.semantics != Object::from_bytes(SEMANTICS).sha256
        || operation.parameters != Object::from_bytes(raw)
    {
        bail!("unsupported original contract interpretation");
    }
    Ok(d)
}

pub(super) fn prepare(
    profile: ArchiveProfile,
    request: &Request,
    context: &Selection,
    claim: &Claim,
    output: &Path,
    reader: &mut impl NodeStore,
) -> Result<(crate::tig_inputs::Source, Value)> {
    let raw =
        tig_git::source::native::context::read_descriptor(context, reader, TreeLimits::default())?;
    let original = descriptor(&raw, request, claim.operation())?;
    let entries = Tree::from_root(context.root.clone())
        .entries(reader, TreeLimits::default())?
        .0;
    let identity = entries
        .get(&LogicalPath::parse("context/identity")?)
        .ok_or_else(|| anyhow::anyhow!("original header missing"))?;
    let header = reader
        .get(identity)?
        .ok_or_else(|| anyhow::anyhow!("original header unavailable"))?;
    let header = identity::parse_json(&header)?;
    if header["checkpoint"] != json!(profile.checkpoint)
        || entries.get(&LogicalPath::parse("source/archive")?) != Some(&profile.archive)
    {
        bail!("original context and external source admission differ");
    }
    let mut source =
        crate::tig_inputs::Source::from_archive(profile, reader, TreeLimits::default())?;
    let root = source.record(&request.scene_id)?.reference().object.clone();
    if entries.get(&LogicalPath::parse(&format!(
        "roots/{}",
        root.sha256.as_str()
    ))?) != Some(&root)
    {
        bail!("original context does not capture admitted scene");
    }
    tig_scene::compile_language_context(&mut source, request, output)?;
    check_contracts(&original.compiled_inputs, output)?;
    let evidence = json!({
        "schema":"reel.retained-contract-equivalence.experimental-1",
        "original_descriptor":Object::from_bytes(&raw),
        "original_compiler":original.compiler,"original_adapter":original.adapter,
        "original_authoring":original.authoring,
        "original_scope_adapter":original.native.scope_adapter_sha256,
        "original_scene_adapter":original.native.native.scene_adapter_sha256,
        "original_input_adapter":original.native.native.input_adapter_sha256,
        "original_tig_pin":original.native.native.tig_source_pin_sha256,
        "current_compiler":Object::from_bytes(include_bytes!("../../scene_delivery_compile.rs")),
        "verification_adapter":Object::from_bytes(include_bytes!("revalidation.rs")),
        "compiled_inputs":original.compiled_inputs,
        "original_claim_replayed":false,"production_claim":null,
        "current_production_currency":"not-asserted"
    });
    Ok((source, evidence))
}

fn check_contracts(original: &BTreeMap<String, Object>, output: &Path) -> Result<()> {
    for name in INPUTS {
        let mut bytes = Vec::new();
        fs::File::open(output.join("delivery").join(name))?
            .take(crate::tig_inputs::JOB_BYTES_MAX as u64 + 1)
            .read_to_end(&mut bytes)?;
        if original.get(name) != Some(&Object::from_bytes(&bytes)) {
            bail!("current compiled contract differs from original");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request {
        serde_json::from_value(json!({"schema":tig_scene::SCHEMA,"scene_id":"scene/one","scene_definition":"types/scene","primary_definitions":["types/media"],"language":"es","controls":{"catalog":"catalog","episode":"episode","policy":"policy","profile":"profile","template_receipt":null},"delivery_id":"delivery","delivery_title":"Test"})).unwrap()
    }
    fn original() -> Value {
        let pin = Object::from_bytes(b"historical interpreter");
        json!({"schema":"reel.tig-native-render-context.experimental-1","native":{"schema":"reel.tig-native-language-context.experimental-1","native":{"schema":"reel.tig-native-scene-context.experimental-1","request":request(),"scene_adapter_sha256":pin.sha256,"input_adapter_sha256":pin.sha256,"tig_source_pin_sha256":pin.sha256},"scope_adapter_sha256":pin.sha256},"compiler":pin,"adapter":pin,"authoring":pin.sha256,"compiled_inputs":INPUTS.into_iter().map(|name|(name,Object::from_bytes(name.as_bytes()))).collect::<BTreeMap<_,_>>()})
    }
    #[test]
    fn only_supported_exact_historical_contract_shapes_are_admitted() {
        for case in [
            "original",
            "unknown",
            "schema",
            "request",
            "partial-adapters",
            "extra-input",
            "missing-input",
            "operation",
            "parameters",
        ] {
            let mut value = original();
            match case {
                "unknown" => value["unknown"] = json!(true),
                "schema" => value["native"]["schema"] = json!("future"),
                "request" => value["native"]["native"]["request"]["language"] = json!("en"),
                "partial-adapters" => {
                    value["native"]["native"]["control_adapter_sha256"] =
                        json!(Object::from_bytes(b"control").sha256)
                }
                "extra-input" => {
                    value["compiled_inputs"]["other.json"] = json!(Object::from_bytes(b"other"))
                }
                "missing-input" => {
                    value["compiled_inputs"]
                        .as_object_mut()
                        .unwrap()
                        .remove("job.json");
                }
                _ => (),
            }
            let raw = serde_json::to_vec(&value).unwrap();
            let mut operation = tig_assets::graph::recipe::Operation {
                name: "reel.native-language.render.experimental-1".into(),
                semantics: Object::from_bytes(SEMANTICS).sha256,
                producer: Object::from_bytes(b"declared historical producer"),
                parameters: Object::from_bytes(&raw),
            };
            if case == "operation" {
                operation.semantics = Object::from_bytes(b"unknown semantics").sha256;
            }
            if case == "parameters" {
                operation.parameters = Object::from_bytes(b"other parameters");
            }
            assert_eq!(
                descriptor(&raw, &request(), &operation).is_ok(),
                case == "original",
                "{case}"
            );
        }
    }
    #[test]
    fn every_compiled_control_must_match_original_bytes() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("delivery")).unwrap();
        let pins = INPUTS
            .into_iter()
            .map(|name| {
                fs::write(root.path().join("delivery").join(name), name).unwrap();
                (name.to_owned(), Object::from_bytes(name.as_bytes()))
            })
            .collect();
        check_contracts(&pins, root.path()).unwrap();
        for name in INPUTS {
            let path = root.path().join("delivery").join(name);
            fs::write(&path, b"changed").unwrap();
            assert!(check_contracts(&pins, root.path()).is_err(), "{name}");
            fs::write(path, name).unwrap();
        }
    }
}
