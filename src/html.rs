use crate::{Page, RootViewData};

pub(crate) fn root_data(
    page: &Page,
    mount_id: &str,
    asset_tags: &str,
    head: &[String],
    body: &str,
) -> Result<RootViewData, serde_json::Error> {
    let data_page = serialize_for_html_script(page)?;
    let inertia_root = format!(
        "<script data-page=\"{}\" type=\"application/json\">{}</script><div id=\"{}\">{}</div>",
        escape_html_attribute(mount_id),
        data_page,
        escape_html_attribute(mount_id),
        body
    );
    Ok(RootViewData {
        page: page.clone(),
        mount_id: mount_id.to_owned(),
        asset_tags: asset_tags.to_owned(),
        inertia_root,
        inertia_head: head.join("\n"),
    })
}

pub(crate) fn fallback_shell(data: &RootViewData) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">{}{}</head><body>{}</body></html>",
        data.inertia_head, data.asset_tags, data.inertia_root
    )
}

/// Serialize page data as JSON safe for an `application/json` script element.
/// Escaping every slash prevents a prop from ending the script element.
fn serialize_for_html_script<T: serde::Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let json = serde_json::to_string(value)?;
    Ok(json
        .replace('/', "\\/")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029"))
}

fn escape_html_attribute(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn page() -> Page {
        Page {
            component: "Users".into(),
            props: serde_json::from_value(json!({ "text": "<&\"" })).unwrap(),
            url: "/users".into(),
            version: None,
            deferred_props: serde_json::Map::new(),
            merge_props: Vec::new(),
            prepend_props: Vec::new(),
            deep_merge_props: Vec::new(),
            match_props_on: Vec::new(),
            scroll_props: serde_json::Map::new(),
            once_props: serde_json::Map::new(),
            flash: serde_json::Map::new(),
            clear_history: false,
            encrypt_history: false,
        }
    }

    #[test]
    fn mount_id_is_escaped_for_quoted_attribute_context() {
        let data = root_data(&page(), "app\"<", "", &[], "").unwrap();
        assert!(data
            .inertia_root
            .starts_with("<script data-page=\"app&quot;&lt;\" type=\"application/json\">"));
    }

    #[test]
    fn page_json_is_script_safe_without_html_entity_encoding() {
        let mut page = page();
        page.props.insert(
            "injection".into(),
            Value::String("</script><script>alert(1)</script>".into()),
        );
        let data = root_data(&page, "app", "", &[], "").unwrap();
        let payload_start = data.inertia_root.find('>').unwrap() + 1;
        let payload_end = data.inertia_root[payload_start..]
            .find("</script>")
            .unwrap()
            + payload_start;
        let payload = &data.inertia_root[payload_start..payload_end];

        assert!(payload.contains("\\/"));
        assert!(payload.contains("\\u003c\\/script\\u003e"));
        assert!(!payload.contains("</script>"));
        assert!(!payload.contains("&lt;"));
        assert!(serde_json::from_str::<Page>(payload).is_ok());
        assert!(data.inertia_root.contains("<div id=\"app\"></div>"));
    }

    #[test]
    fn trusted_ssr_body_and_head_are_inserted_into_root_data() {
        let data = root_data(
            &page(),
            "app",
            "<script src=\"/app.js\"></script>",
            &["<title>Page</title>".into(), "<meta name=\"x\">".into()],
            "<main>SSR</main>",
        )
        .unwrap();

        assert!(data.inertia_root.ends_with("</div>"));
        assert!(data.inertia_root.contains("<main>SSR</main>"));
        assert_eq!(data.inertia_head, "<title>Page</title>\n<meta name=\"x\">");
    }

    #[test]
    fn fallback_shell_places_head_assets_and_root_in_document() {
        let data = root_data(
            &page(),
            "app",
            "<script></script>",
            &["<title>x</title>".into()],
            "",
        )
        .unwrap();
        let shell = fallback_shell(&data);

        assert!(shell.starts_with("<!doctype html><html><head>"));
        assert!(shell.contains("<title>x</title><script></script>"));
        assert!(shell.ends_with("</body></html>"));
    }
}
