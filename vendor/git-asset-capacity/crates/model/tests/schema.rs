#![cfg(feature = "schema")]
#[path = "schema/contract_cases.rs"]
mod contract_cases;
use git_asset_core::Object;
use git_asset_model::{
    native::{BoundInstance, Definition, Instance},
    schema::{Dialect, Error, Schema},
};
use serde_json::{Value, json};

fn documents(schema: &Value, authored: Value) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let schema = serde_json::to_vec(schema).unwrap();
    let definition = serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":"types/example",
        "authored_schema":Object::from_bytes(&schema),"authoring":{"properties":["text","n","items","kind","amount","currency","email","width","other"],"required":[],"defaults":{"width":80}},
        "stream_rules":{},"semantic_rules":{"unexecuted":true}})).unwrap();
    let pin = Object::from_bytes(&definition);
    let instance = serde_json::to_vec(&json!({"schema":"tig.instance.design-1","id":"private/example",
        "definition":{"id":"types/example","sha256":pin.sha256,"bytes":pin.bytes},"authored":authored,
        "streams":[],"selected":null,"status":{},"metadata":{"private":"DO NOT PRINT"}})).unwrap();
    (instance, definition, schema)
}
fn bound(documents: &(Vec<u8>, Vec<u8>, Vec<u8>)) -> BoundInstance {
    Instance::parse(&documents.0)
        .unwrap()
        .bind(&documents.1, &documents.2)
        .unwrap()
}
fn check(schema: &Value, authored: Value) -> Result<(), Error> {
    let docs = documents(schema, authored);
    let bound = bound(&docs);
    Schema::compile(bound.definition(), &docs.2, Dialect::Draft202012)?
        .check(&bound)
        .map(|_| ())
}

#[test]
fn schema_executes_each_explicit_dialect_and_rejects_wrong_declarations() {
    for dialect in [
        Dialect::Draft4,
        Dialect::Draft6,
        Dialect::Draft7,
        Dialect::Draft201909,
        Dialect::Draft202012,
    ] {
        let schema = json!({"$schema":dialect.uri(),"type":"object","properties":{"n":{"type":"integer","minimum":1}},"required":["n"]});
        let valid = documents(&schema, json!({"n":1}));
        let engine = Schema::compile(bound(&valid).definition(), &valid.2, dialect).unwrap();
        let proof = engine.check(&bound(&valid)).unwrap();
        assert_eq!(proof.instance(), &Object::from_bytes(&valid.0));
        assert_eq!(proof.authored_schema(), &Object::from_bytes(&valid.2));
        assert_eq!(proof.definition(), &Object::from_bytes(&valid.1));
        assert_eq!(proof.policy(), &Object::from_bytes(engine.policy_bytes()));
        assert!(!proof.complete_semantic_coverage());
        let invalid = documents(&schema, json!({"n":0}));
        assert!(engine.check(&bound(&invalid)).is_err());
    }
    for schema in [
        json!({"$schema":"https://example.invalid/unknown"}),
        json!({"$schema":false}),
        json!({"$schema":"https://json-schema.org/draft/2020-12/schema##"}),
    ] {
        assert_eq!(check(&schema, json!({})), Err(Error::Dialect));
    }
    assert!("latest".parse::<Dialect>().is_err());
}

#[test]
fn schema_checks_nested_objects_arrays_bounds_composition_and_local_references() {
    let schema = json!({"type":"object","$defs":{"row":{"type":"object","properties":{"name":{"type":"string","minLength":1}},"required":["name"],"additionalProperties":false}},
        "properties":{"items":{"type":"array","items":{"$ref":"#/$defs/row"},"minItems":1,"maxItems":2,"uniqueItems":true},"kind":{"enum":["sale","gift"]},"amount":{"type":"integer","minimum":1,"maximum":100}},
        "required":["items","kind"],"allOf":[{"if":{"properties":{"kind":{"const":"sale"}}},"then":{"required":["amount"]},"else":{"not":{"required":["amount"]}}}],"unevaluatedProperties":false});
    assert!(
        check(
            &schema,
            json!({"items":[{"name":"x"}],"kind":"sale","amount":10})
        )
        .is_ok()
    );
    for bad in [
        json!({"items":[],"kind":"gift"}),
        json!({"items":[{"name":""}],"kind":"gift"}),
        json!({"items":[{"name":"x"},{"name":"x"}],"kind":"gift"}),
        json!({"items":[{"name":"x"}],"kind":"sale"}),
        json!({"items":[{"name":"x"}],"kind":"gift","amount":10}),
        json!({"items":[{"name":"x"}],"kind":"sale","amount":101}),
    ] {
        assert!(check(&schema, bad).is_err());
    }
}

#[test]
fn schema_enforces_patterns_known_formats_and_rejects_unknown_formats() {
    let schema = json!({"properties":{"text":{"type":"string","pattern":"^[a-z]+$"},"email":{"type":"string","format":"email"}},"required":["text","email"]});
    assert!(
        check(
            &schema,
            json!({"text":"hola","email":"person@example.invalid"})
        )
        .is_ok()
    );
    assert!(check(&schema, json!({"text":"123","email":"bad"})).is_err());
    assert!(
        check(
            &json!({"properties":{"text":{"format":"unadmitted-private-format"}}}),
            json!({"text":"anything"})
        )
        .is_err()
    );
}

#[test]
fn schema_boolean_invalid_and_missing_external_resources_are_not_success() {
    assert!(check(&json!(true), json!({})).is_ok());
    assert!(check(&json!(false), json!({})).is_err());
    for schema in [
        json!({"type":12}),
        json!({"required":"n"}),
        json!({"$ref":"#/$defs/missing"}),
        json!({"$ref":"https://127.0.0.1:9/private"}),
        json!({"$ref":"file:///definitely-not-authorized.json"}),
    ] {
        assert!(check(&schema, json!({})).is_err());
    }
}

#[test]
fn schema_exact_pins_defaults_and_private_error_categories_remain_immutable() {
    let docs = documents(
        &json!({"properties":{"width":{"type":"integer","default":99},"text":{"type":"integer"}}}),
        json!({"text":"PRIVATE INSTANCE TEXT"}),
    );
    let original = docs.0.clone();
    let bound = bound(&docs);
    let engine = Schema::compile(bound.definition(), &docs.2, Dialect::Draft202012).unwrap();
    let error = engine.check(&bound).unwrap_err();
    assert!(!format!("{error:?} {error}").contains("PRIVATE"));
    assert!(!bound.instance().value().authored.contains_key("width"));
    assert_eq!(docs.0, original);
    let mut wrong = docs.2.clone();
    wrong.push(b' ');
    assert!(matches!(
        Schema::compile(bound.definition(), &wrong, Dialect::Draft202012),
        Err(Error::Pin)
    ));
    assert!(matches!(
        Schema::compile(
            bound.definition(),
            &vec![b' '; 65_537],
            Dialect::Draft202012
        ),
        Err(Error::Json)
    ));
    let mut definition: Value = serde_json::from_slice(&docs.1).unwrap();
    definition["semantic_rules"] = json!({"other":true});
    let raw = serde_json::to_vec(&definition).unwrap();
    let parsed = Definition::parse(&raw).unwrap();
    let other = Schema::compile(&parsed, &docs.2, Dialect::Draft202012).unwrap();
    assert_eq!(other.check(&bound).unwrap_err(), Error::Pin);
}

#[test]
fn schema_unfinished_regex_under_negation_cannot_become_a_valid_instance() {
    let schema = json!({"properties":{"text":{"not":{"pattern":"^(a|a)*\\1b"}}}});
    // A bounded backtracking engine may not finish this match. Negation must not
    // turn that unknown result into a successful authored-schema capability.
    let result = check(&schema, json!({"text":format!("{}!","a".repeat(50))}));
    assert!(result.is_err());
}

#[test]
fn schema_offline_policy_refuses_existing_file_and_live_http_resources() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("external.json");
    std::fs::write(&path, b"true").unwrap();
    let path = path.to_str().unwrap().replace('\\', "/");
    let file = if cfg!(windows) {
        format!("file:///{path}")
    } else {
        format!("file://{path}")
    };
    assert!(check(&json!({"$ref":file}), json!({})).is_err());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let signal = stopped.clone();
    let server = std::thread::spawn(move || {
        let started = Instant::now();
        while !signal.load(Ordering::Acquire) && started.elapsed() < Duration::from_secs(5) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut request = [0u8; 1024];
                    let _ = stream.read(&mut request);
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntrue",
                    );
                    return true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Err(e) => panic!("fixture accept: {e}"),
            }
        }
        false
    });
    let result = check(
        &json!({"$ref":format!("http://{address}/schema")}),
        json!({}),
    );
    stopped.store(true, Ordering::Release);
    assert!(
        !server.join().unwrap(),
        "schema validation made a network request"
    );
    assert!(result.is_err());
}
