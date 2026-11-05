use super::*;
fn evidence(text: &str, name: &str) -> Value {
    json!({"name":name,"tokens":words(text).into_iter().enumerate().map(|(i,w)|json!({"word":w,"span":[i as f64,(i+1) as f64]})).collect::<Vec<_>>()})
}
#[test]
fn normalization_and_render_repetitions_preserve_source_words() {
    assert_eq!(
        words("Árbol, NIÑO; canción ﬃ 12"),
        vec!["arbol", "nino", "cancion", "ffi"]
    );
    let canonical = "Alpha\nBeta\nGamma\n\nDelta\nEcho";
    assert_eq!(
        render_words(canonical, 2, Some(2)).unwrap(),
        words("alpha beta alpha beta gamma gamma delta echo delta echo")
    );
    assert_eq!(
        render_words("Alpha beta", 2, None).unwrap(),
        words("alpha beta alpha beta")
    );
    assert!(render_words(canonical, 1, Some(2)).is_err());
}
#[test]
fn near_contiguous_presence_allows_repetition_but_reports_order_separately() {
    let r = report(
        "Alpha beta gamma\nDelta echo foxtrot",
        &[evidence("delta echo foxtrot alpha beta gamma", "mix")],
    )
    .unwrap();
    assert_eq!(r["passed"], true);
    assert_eq!(r["continuous_order_passed"], false);
    assert_eq!(r["exact_lines"], 2);
}
#[test]
fn never_union_passes_reuse_tokens_or_pass_absent_single_word_lines() {
    let r = report(
        "Alpha beta gamma delta",
        &[
            evidence("alpha beta", "one"),
            evidence("gamma delta", "two"),
        ],
    )
    .unwrap();
    assert_eq!(r["passed"], false);
    assert_eq!(
        report("Alpha\nAlpha", &[evidence("alpha", "one")]).unwrap()["qualifying_lines"],
        1
    );
    assert_eq!(
        report("Alpha", &[evidence("beta", "one")]).unwrap()["passed"],
        false
    );
    assert_eq!(
        report("Alpha beta", &[evidence("alpha", "one")]).unwrap()["passed"],
        true
    );
}
#[test]
fn scattered_words_and_long_spans_do_not_supply_phrases() {
    assert_eq!(
        report(
            "Alpha beta gamma delta",
            &[evidence("alpha x x x beta y y y gamma z z z delta", "one")]
        )
        .unwrap()["passed"],
        false
    );
    let mut p = evidence("alpha beta gamma", "one");
    p["tokens"][2]["span"] = json!([30.0, 31.0]);
    assert_eq!(report("Alpha beta gamma", &[p]).unwrap()["exact_lines"], 0);
}
#[test]
fn first_complete_pass_wins_ties_and_missing_spans_refuse() {
    let r = report(
        "Alpha beta",
        &[
            evidence("alpha beta", "first"),
            evidence("alpha beta", "second"),
        ],
    )
    .unwrap();
    assert_eq!(r["lines"][0]["best_pass"], "first");
    let mut p = evidence("alpha", "one");
    p["tokens"][0]["span"] = json!([null, null]);
    assert!(report("Alpha", &[p]).is_err());
}
