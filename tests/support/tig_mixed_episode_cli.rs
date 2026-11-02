//! Native opening/credits in the same tiny captured episode as normal scenes.
use super::*;

pub(super) fn configure_browser(
    p: &mut Project,
    request: &mut reel::tig_episode::Request,
) -> Vec<Value> {
    let mut presentations = configure(p, request);
    let schema =
        Object::from_bytes(&fs::read(p.repo.path().join("definitions/authored.json")).unwrap());
    let definition = write(
        p.repo.path(),
        "definitions/mixed-browser-source.json",
        &json!({
            "schema":"tig.definition.design-1","id":"types/mixed-browser-source","authored_schema":schema,
            "authoring":{"properties":["document"],"required":["document"],"defaults":{}},"stream_rules":{},"semantic_rules":{}
        }),
    );
    p.profile["admitted"].as_array_mut().unwrap().push(json!({
        "id":"types/mixed-browser-source","object":definition,"payload":{"kind":"selected"}
    }));
    let media =
        Object::from_bytes(&fs::read(p.repo.path().join("definitions/media.json")).unwrap());
    let svg=p.publish("hot",br##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect id="r" width="64" height="64" fill="red"/><script>addEventListener('reelframe',e=>document.getElementById('r').setAttribute('fill',e.detail.frame%2?'blue':'red'))</script></svg>"##);
    write(
        p.repo.path(),
        "records/mixed-browser-svg.tig",
        &json!({
            "schema":"tig.instance.design-1","id":"mixed/browser-svg","definition":{"id":"types/media","sha256":media.sha256,"bytes":media.bytes},
            "authored":{},"selected":svg,"streams":[],"status":{"selection_state":"selected-private-production"},"metadata":{}
        }),
    );
    write(
        p.repo.path(),
        "records/mixed-opening-source.tig",
        &json!({
            "schema":"tig.instance.design-1","id":"mixed/opening/source",
            "definition":{"id":"types/mixed-browser-source","sha256":definition.sha256,"bytes":definition.bytes},
            "authored":{"document":{"source":"opening.svg","audio":"audio.wav","inputs":{"opening.svg":{"id":"mixed/browser-svg"},"audio.wav":{"id":"cue/main"}},
                "width":64,"height":64,"fps":24,"frame_count":24}},
            "selected":null,"streams":[],"status":{"selection_state":"selected-private-production"},"metadata":{}
        }),
    );
    p.commit("declare browser opening primaries alongside still credits");
    let request_value = json!({"schema":reel::tig_browser::SCHEMA,"root_id":"mixed/opening/source",
        "root_definition":"types/mixed-browser-source","primary_definitions":["types/media"],
        "bounds":{"max_inputs":8,"max_input_bytes":1048576,"max_frames":24,"max_canvas_pixels":4096}});
    let browser_path =
        PathBuf::from(std::env::var_os("REEL_TEST_BROWSER").expect("explicit browser required"));
    let identity = reel::media_runtime::ByteIdentity::observe_file(&browser_path).unwrap();
    let program = json!({"path":browser_path,"sha256":identity.sha256,"bytes":identity.bytes});
    let browser =
        reel::media_runtime::BoundTool::bind(&serde_json::from_value(program.clone()).unwrap())
            .unwrap();
    let execution = json!({"program":program,"clean_picture":false,"limits":{
        "capture":{"max_requests":32,"max_resource_bytes":1048576,"max_served_bytes":4194304,"max_message_bytes":4194304,
            "max_messages":4096,"max_png_bytes":1048576,"max_frames":24,"max_canvas_pixels":4096,"max_elapsed_ms":60000,"max_io_wait_ms":15000},
        "max_workspace_bytes":134217728,"max_output_bytes":1048576,"browser_memory_bytes":2147483648_u64,"browser_processes":64,
        "encoder_memory_bytes":268435456,"encoder_processes":4}});
    let limits = serde_json::from_value(execution["limits"].clone()).unwrap();
    let mut source = p.source();
    let prepared = reel::tig_browser::lower(
        &mut source,
        &serde_json::from_value(request_value.clone()).unwrap(),
    )
    .unwrap();
    let inputs = source
        .prepare_files(&prepared.inputs.values().cloned().collect::<Vec<_>>())
        .unwrap();
    let workspace = p.data.path().join("original-browser-opening");
    fs::create_dir(&workspace).unwrap();
    let output = workspace.join("master.mkv");
    reel::tig_browser::render::native(
        &prepared,
        &inputs,
        &p.programs,
        &browser,
        &workspace,
        &output,
        &limits,
        false,
        || Ok(()),
    )
    .unwrap();
    let selected = p.publish("hot", &fs::read(&output).unwrap());
    let mut record = read(p.repo.path().join("records/output-opening.tig"));
    record["selected"] = json!(selected);
    write(p.repo.path(), "records/output-opening.tig", &record);
    presentations[0]["request"] = request_value;
    presentations[0]["browser"] = execution;
    p.commit("select synthetic browser master for fresh-reference episode assembly");
    presentations
}

pub(super) fn configure(p: &mut Project, request: &mut reel::tig_episode::Request) -> Vec<Value> {
    let authored =
        Object::from_bytes(&fs::read(p.repo.path().join("definitions/authored.json")).unwrap());
    let mut definitions = BTreeMap::new();
    for kind in [
        "source", "template", "timing", "clock", "evidence", "output",
    ] {
        let fields = if kind == "output" {
            json!([
                "document",
                "presentation",
                "episode",
                "episode_id",
                "language",
                "source_segment_id",
                "component_kind"
            ])
        } else {
            json!(["document"])
        };
        let object = write(
            p.repo.path(),
            &format!("definitions/mixed-{kind}.json"),
            &json!({"schema":"tig.definition.design-1","id":format!("types/mixed-{kind}"),"authored_schema":authored,
                "authoring":{"properties":fields,"required":[],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
        );
        p.profile["admitted"].as_array_mut().unwrap().push(json!({"id":format!("types/mixed-{kind}"),"object":object,"payload":{"kind":"selected"}}));
        definitions.insert(kind, object);
    }
    let media: Object =
        Object::from_bytes(&fs::read(p.repo.path().join("definitions/media.json")).unwrap());
    let layer=p.publish("hot", b"[Script Info]\nScriptType: v4.00+\nPlayResX: 64\nPlayResY: 64\n[V4+ Styles]\nFormat: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding\nStyle: Default,Arial,10,&H00FFFFFF,&H00000000,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,2,2,2,1\n[Events]\nFormat: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,Mixed fixture\n");
    write(
        p.repo.path(),
        "records/mixed-layer.tig",
        &json!({"schema":"tig.instance.design-1","id":"mixed/layer",
        "definition":{"id":"types/media","sha256":media.sha256,"bytes":media.bytes},"authored":{},"selected":layer,"streams":[],
        "status":{"selection_state":"selected-private-production"},"metadata":{}}),
    );
    for name in ["cel-opening", "cel-credits", "cue-main"] {
        let path = p.repo.path().join(format!("records/{name}.tig"));
        let mut record = read(&path);
        record["status"] = json!({"selection_state":"selected-private-production"});
        write(p.repo.path(), &format!("records/{name}.tig"), &record);
    }
    let cue = read(p.repo.path().join("records/cue-main.tig"));
    let mut result = Vec::new();
    for unit in ["opening", "credits"] {
        for (kind, document) in [
            (
                "source",
                json!({"schema":"reel.tig-still-sequence-source.v1","episode_id":"episode-1","language":"es","role":unit,
                "template":{"id":format!("mixed/{unit}/template")},"pictures":[{"id":format!("cel/{unit}")}],"editable_layer":{"id":"mixed/layer"},"audio":{"id":"cue/main"},"picture_timing":{"id":format!("mixed/{unit}/timing")}}),
            ),
            (
                "template",
                json!({"schema":"reel.presentation-still-template.v2","template_id":unit,"width":64,"height":64,"frame_rate":24,"sample_rate":48000,"picture_count":1,"total_frames":24,"audio_required":true}),
            ),
            (
                "timing",
                json!({"schema":"reel.presentation-picture-timing.v1","clock":{"id":format!("mixed/{unit}/clock")},"spans":[{"start_anchor":"start","end_anchor":"end"}]}),
            ),
            (
                "clock",
                json!({"schema":"reel.presentation-source-clock.v1","source":{"id":"cue/main"},"source_at_measurement":cue["selected"],"sample_rate":48000,"total_samples":48000,"evidence":{"id":format!("mixed/{unit}/evidence")},"anchors":[{"id":"start","sample":0},{"id":"end","sample":48000}]}),
            ),
            ("evidence", json!("synthetic measured clock")),
        ] {
            write(
                p.repo.path(),
                &format!("records/mixed-{unit}-{kind}.tig"),
                &json!({"schema":"tig.instance.design-1","id":format!("mixed/{unit}/{kind}"),
                "definition":{"id":format!("types/mixed-{kind}"),"sha256":definitions[kind].sha256,"bytes":definitions[kind].bytes},
                "authored":{"document":document},"selected":null,"streams":[],"status":{},"metadata":{}}),
            );
        }
        result.push(json!({"id":unit,"output_id":format!("output/{unit}"),"output_definition":"types/mixed-output","request":{
            "schema":reel::tig_presentation::SCHEMA,"presentation_id":format!("mixed/{unit}/source"),"source_definition":"types/mixed-source",
            "template_definition":"types/mixed-template","timing_definition":"types/mixed-timing","clock_definition":"types/mixed-clock",
            "evidence_definition":"types/mixed-evidence","primary_definitions":["types/media"],"allowed_primary_states":["selected-private-production"]}}));
    }
    p.commit("declare native opening and credits inputs");
    let mut conform = read(p.data.path().join("episode-cli-v1.json"));
    for spec in &result {
        let unit = spec["id"].as_str().unwrap();
        let native_request = serde_json::from_value(spec["request"].clone()).unwrap();
        let mut source = p.source();
        let compiled = reel::tig_presentation::compile(&source, &native_request).unwrap();
        let inputs = compiled.prepare(&mut source).unwrap();
        let output = p.data.path().join(format!("mixed-original-{unit}"));
        reel::presentation_still_sequence::build_with_control_files(
            compiled.manifest(),
            &compiled,
            &p.programs.with_inputs(&inputs),
            &output,
        )
        .unwrap();
        let selected = p.publish("hot", &fs::read(output.join("master.mkv")).unwrap());
        let receipt = p.publish("hot", &fs::read(output.join("receipt.json")).unwrap());
        write(
            p.repo.path(),
            &format!("records/output-{unit}.tig"),
            &json!({"schema":"tig.instance.design-1","id":format!("output/{unit}"),
            "definition":{"id":"types/mixed-output","sha256":definitions["output"].sha256,"bytes":definitions["output"].bytes},
            "authored":{"document":{},"presentation":{"id":format!("mixed/{unit}/source")},"episode":{"id":"source/episode"},"episode_id":"episode-1","language":"es","source_segment_id":unit,"component_kind":"episode-presentation"},
            "selected":selected,"streams":[],"status":{},"metadata":{}}),
        );
        let index = if unit == "opening" { 0 } else { 3 };
        conform["segments"][index] = json!({"kind":"episode-presentation","id":unit,
            "master":reference(&format!("mixed/{unit}/master.mkv"),&selected),"source_receipt":reference(&format!("mixed/{unit}/receipt.json"),&receipt)});
    }
    let episode = json!({"schema":"reel.episode-authoring.v1","episode_id":"episode-1","season_id":"season-1","authoring_state":"ready-for-private-build",
        "master_template_id":"master","scene_policy_id":"policy","presentation":[{"role":"opening","template_id":"opening","content":{}},{"role":"credits","template_id":"credits","content":{}}],"score_palette":[],"scene_ids":["scene-a","scene-b"]});
    let master = json!({"schema":"reel.episode-master-template.v2","template_id":"master","ordered_units":[{"kind":"episode-presentation","id":"opening"},{"kind":"scene","id":"scene-a"},{"kind":"scene","id":"scene-b"},{"kind":"episode-presentation","id":"credits"}]});
    let master_pin = p.publish("hot", &serde_json::to_vec(&master).unwrap());
    let catalog = json!({"schema":"reel.scene-template-catalog.v1","templates":[{"template_id":"master","kind":"episode-master","definition_sha256":master_pin.sha256,"required_content_keys":[]},
        {"template_id":"opening","kind":"opening","definition_sha256":Object::from_bytes(b"opening layout").sha256,"required_content_keys":[]},
        {"template_id":"credits","kind":"credits","definition_sha256":Object::from_bytes(b"credits layout").sha256,"required_content_keys":[]}]});
    let episode_pin = p.publish("hot", &serde_json::to_vec(&episode).unwrap());
    let catalog_pin = p.publish("hot", &serde_json::to_vec(&catalog).unwrap());
    conform["episode"] = reference("mixed/episode.json", &episode_pin);
    conform["master_template_definition"] = reference("mixed/master.json", &master_pin);
    conform["catalog"] = reference("mixed/catalog.json", &catalog_pin);
    let conform_pin = p.publish("hot", &serde_json::to_vec(&conform).unwrap());
    let mut record = read(p.repo.path().join("records/episode-source.tig"));
    record["selected"] = json!(episode_pin);
    record["authored"]["scene_ids"] = json!([{"id":"scene/scene-a"},{"id":"scene/scene-b"}]);
    record["streams"][0]["object"] = json!(conform_pin);
    write(p.repo.path(), "records/episode-source.tig", &record);
    request
        .component_definitions
        .push("types/mixed-output".into());
    p.commit("compose native presentations and chapter scenes in one episode");
    result
}
