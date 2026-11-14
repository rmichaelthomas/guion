use serde_json::{json, Value};
use xtask::surface_diff::compare;

fn surface(functions: Value) -> Value {
    json!({
        "schema_version": 1,
        "kind": "guion_std_surface",
        "modules": [{
            "module": "demo",
            "functions": functions,
            "records": [{"name": "Point", "fields": [{"name": "x", "type": "Int"}]}],
        }],
    })
}

fn function(name: &str, parameter_type: &str) -> Value {
    json!({
        "name": name,
        "source": "std:demo",
        "parameters": [{"name": "value", "type": parameter_type}],
        "result": "Int",
        "effects": [],
    })
}

#[test]
fn an_added_function_passes() {
    let from = surface(json!([function("one", "Int")]));
    let to = surface(json!([function("one", "Int"), function("two", "Text")]));
    assert_eq!(
        compare(&from, &to),
        Ok(vec!["added std:demo.two".to_owned()])
    );
}

#[test]
fn a_removed_function_fails() {
    let from = surface(json!([function("one", "Int"), function("two", "Text")]));
    let to = surface(json!([function("one", "Int")]));
    let problems = compare(&from, &to).expect_err("removal must fail");
    assert_eq!(problems, vec!["removed function std:demo.two".to_owned()]);
}

#[test]
fn a_changed_parameter_type_fails() {
    let from = surface(json!([function("one", "Int")]));
    let to = surface(json!([function("one", "Text")]));
    let problems = compare(&from, &to).expect_err("re-signing must fail");
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("re-signed function std:demo.one"));
    assert!(problems[0].contains("Int") && problems[0].contains("Text"));
}

#[test]
fn a_removed_module_or_record_fails_and_a_new_module_passes() {
    let from = surface(json!([]));
    let mut to = from.clone();
    to["modules"][0]["records"] = json!([]);
    assert!(compare(&from, &to).is_err());
    let mut gone = from.clone();
    gone["modules"] = json!([]);
    assert!(compare(&from, &gone).is_err());
    let mut grown = from.clone();
    grown["modules"]
        .as_array_mut()
        .unwrap()
        .push(json!({"module": "other", "functions": [], "records": []}));
    assert!(compare(&from, &grown).is_ok());
}

#[test]
fn a_deprecation_note_is_not_a_change() {
    let from = surface(json!([function("one", "Int")]));
    let mut marked = function("one", "Int");
    marked["deprecated"] = json!({"since": "0.3.0", "use": "two"});
    let to = surface(json!([marked]));
    assert_eq!(compare(&from, &to), Ok(vec![]));
}
