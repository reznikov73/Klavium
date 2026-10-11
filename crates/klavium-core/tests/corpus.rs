//! Тесты контрольного корпуса: конвертация и коррекция раскладки.
//!
//! 85 случаев: входной текст, раскладка и контекст — против
//! ожидаемого решения о коррекции, исправленного текста,
//! целевой раскладки и уверенности.

use klavium_core::detect::smart_correction;
use klavium_core::Layout;

#[derive(serde::Deserialize)]
struct Case {
    text: String,
    layout: Option<String>,
    #[serde(default)]
    context: Vec<String>,
    needs: bool,
    corrected: String,
    confidence: f64,
    target: Option<String>,
}

fn parse_layout(value: Option<&str>) -> Option<Layout> {
    match value {
        Some("en") => Some(Layout::En),
        Some("ru") => Some(Layout::Ru),
        _ => None,
    }
}

#[test]
fn corpus_cases_match_expected() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("fixtures/corpus.json"))
        .expect("корпус должен быть валидным JSON");

    for case in &cases {
        let layout = parse_layout(case.layout.as_deref());
        let context: Vec<&str> = case.context.iter().map(String::as_str).collect();
        let result = smart_correction(&case.text, &context, layout);

        assert_eq!(
            result.needs_correction, case.needs,
            "needs_correction для {:?} (layout={:?})",
            case.text, case.layout
        );
        assert_eq!(
            result.corrected, case.corrected,
            "corrected для {:?} (layout={:?})",
            case.text, case.layout
        );
        assert_eq!(
            result.target_layout,
            parse_layout(case.target.as_deref()),
            "target_layout для {:?} (layout={:?})",
            case.text,
            case.layout
        );
        assert!(
            (result.confidence - case.confidence).abs() < 1e-5,
            "confidence для {:?} (layout={:?}): {} != {}",
            case.text,
            case.layout,
            result.confidence,
            case.confidence
        );
    }

    assert!(
        cases.len() > 50,
        "корпус подозрительно мал: {}",
        cases.len()
    );
}
